pub mod scheduler;
use chrono::{prelude::*, Duration};
use rppal::pwm::Pwm;
pub use scheduler::Scheduler;
use std::{sync::mpsc, thread};

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub struct Strength(f64);
impl Strength {
    pub fn new(value: f64) -> Self {
        assert!(value < 1.0);
        assert!(value > 0.0);
        Self(value)
    }
}
#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub struct Transition {
    pub from: Strength,
    pub to: Strength,
    pub time: Duration,
}
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub enum Day {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}
impl Day {
    pub fn next(&self) -> Self {
        match self {
            Self::Monday => Self::Tuesday,
            Self::Tuesday => Self::Wednesday,
            Self::Wednesday => Self::Thursday,
            Self::Thursday => Self::Friday,
            Self::Friday => Self::Saturday,
            Self::Saturday => Self::Sunday,
            Self::Sunday => Self::Monday,
        }
    }
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub enum Command {
    Set(Strength),
    LinearIncrease(Transition),
    LinearDecrease(Transition),
    SineIncrease(Transition),
    SineDecrease(Transition),
    ChangeDayTimer(Day, NaiveTime),
    Finish,
}
pub enum Action {
    /// Thread sleep this amount and call me again
    Wait(std::time::Duration),
    /// Set the output to this strength
    Set(Strength),
    /// Stop execution of loop
    Break,
}

pub trait VariableOut {
    fn set(&mut self, value: Strength);
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
pub struct Controller<T: VariableOut + Send> {
    channel: mpsc::Sender<Command>,
    handle: thread::JoinHandle<T>,
}
impl<T: VariableOut + Send> Controller<T> {
    pub fn new(output: T) -> Self {
        let (sender, receiver) = mpsc::channel();
        // make channel
        let handle = thread::spawn(move || {
            let receiver = receiver;
            let mut state = scheduler::State::new();
            loop {
                let action = state.process(receiver.try_recv().ok());
                match action {
                    Action::Wait(dur) => thread::sleep(dur),
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
    pub fn finish(self) -> T {
        self.send(Command::Finish);
        self.handle.join().expect("child thread panic")
    }
}
