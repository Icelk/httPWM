pub mod scheduler;
use chrono::prelude::*;
use rppal::pwm::Pwm;
pub use scheduler::Scheduler;
use std::time::{Duration, Instant};
use std::{sync::mpsc, thread};

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub struct Strength(f64);
impl Strength {
    pub fn new(value: f64) -> Self {
        assert!(value < 1.0);
        assert!(value > 0.0);
        Self(value)
    }
    pub fn new_clamped(value: f64) -> Self {
        if value < 0.0 {
            Self(0.0)
        } else if value > 1.0 {
            Self(1.0)
        } else {
            Self(value)
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Hash)]
pub enum TransitionInterpolation {
    Linear,
    Sine,
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub struct Transition {
    pub from: Strength,
    pub to: Strength,
    pub time: Duration,
    pub interpolation: TransitionInterpolation,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Command {
    Set(Strength),
    SetTransition(Transition),
    ChangeDayTimer(Weekday, Option<NaiveTime>),
    Finish,
}
pub enum Action {
    /// Thread sleep this amount and call me again
    Wait(scheduler::SleepTime),
    /// Set the output to this strength
    Set(Strength),
    /// Stop execution of loop
    Break,
}

pub trait VariableOut {
    fn set(&mut self, value: Strength);
}
impl VariableOut for Pwm {
    fn set(&mut self, value: Strength) {
        self.set_pulse_width(Duration::from_micros((value.0 * 1000.0).round() as u64))
            .unwrap();
        self.set_period(Duration::from_micros(1000)).unwrap();
    }
}

pub struct PrintOut;
impl VariableOut for PrintOut {
    fn set(&mut self, value: Strength) {
        println!("Set dur! {:?}", value);
    }
}

/// # Main loop of thread
///
/// - check for new commands
/// > If got new, reset state of transition!
/// - check all schedulers
/// > Get minimum, and if any are due, cancel transition.
/// - check transition
/// > Progress state of transition or remove if complete
/// - if nothing happened, sleep 'till next scheduler
///
/// This allows the thread to be `unpark()`ed.
///
/// The handler's job is to handle [`Scheduler`]s and transitions.
///
/// This is done by spawning a thread and running all code on it.
pub struct Controller<T: VariableOut + Send + 'static> {
    channel: mpsc::Sender<Command>,
    handle: thread::JoinHandle<T>,
}
impl<T: VariableOut + Send + 'static> Controller<T> {
    pub fn new(mut output: T, scheduler: scheduler::WeekScheduler) -> Self {
        let (sender, receiver) = mpsc::channel();
        // make channel
        let handle = thread::spawn(move || {
            let receiver = receiver;
            let mut state = scheduler::State::new(scheduler);
            loop {
                let action = state.process(receiver.try_recv().ok());
                match action {
                    Action::Wait(sleep) => match sleep {
                        scheduler::SleepTime::Duration(dur) => thread::sleep(dur),
                        scheduler::SleepTime::Forever => thread::park(),
                    },
                    Action::Set(s) => output.set(s),
                    Action::Break => break,
                }
            }
            output
        });
        // spawn thread, moving `pwm`
        // return Self with the channel and JoinHandle
        Self {
            channel: sender,
            handle,
        }
    }

    pub fn send(&mut self, command: Command) {
        self.channel
            .send(command)
            .expect("failed to send message on channel");
        self.handle.thread().unpark();
    }

    /// Will wait on any transitions to conclude and then give back the underlying object
    pub fn finish(mut self) -> T {
        self.send(Command::Finish);
        self.handle.join().expect("child thread paniced")
    }
}
