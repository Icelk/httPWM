pub mod scheduler;
use chrono::prelude::*;
use rppal::{gpio::OutputPin, pwm::Pwm};
pub use scheduler::Scheduler;
use std::time::{Duration, Instant};
use std::{sync::mpsc, thread};

#[derive(Debug, PartialEq, PartialOrd, Clone, Copy)]
pub struct Strength(f64);
impl Strength {
    pub fn new(value: f64) -> Self {
        assert!(value <= 1.0);
        assert!(value >= 0.0);
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
    pub fn off() -> Self {
        Self(0.0)
    }
    pub fn is_off(&self) -> bool {
        self.0 == 0.0
    }
    pub fn into_inner(self) -> f64 {
        self.0
    }
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub enum TransitionInterpolation {
    Linear,
    Sine,
    /// Works same as [`TransitionInterpolation::Linear`], except it fades back to [`Transition::from`] value
    /// after reaching [`Transition::to`], for [`Transition::time`] * the `f64` supplied.
    LinearToAndBack(f64),
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub struct Transition {
    pub from: Strength,
    pub to: Strength,
    pub time: Duration,
    pub interpolation: TransitionInterpolation,
}

#[derive(Debug)]
pub enum Command {
    Set(Strength),
    SetTransition(Transition),
    ChangeDayTimer(Weekday, Option<NaiveTime>),
    ChangeDayTimerTransition(Transition),
    AddScheduler(Box<dyn Scheduler>),
    ClearAllSchedulers,
    Finish,
}
#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub enum Action {
    /// Thread sleep this amount and call me again.
    Wait(scheduler::SleepTime),
    /// Set the output to this strength.
    Set(Strength),
    /// Stop execution of loop
    Break,
}

pub trait VariableOut {
    /// Main function. Used to set output.
    fn set(&mut self, value: Strength);

    /// Enable the output when activating. Here for optimization of power usage when using PWM.
    fn enable(&mut self);
    /// Disable the output when not active. Here for optimization of power usage when using PWM.
    fn disable(&mut self);
}
impl VariableOut for Pwm {
    fn set(&mut self, value: Strength) {
        self.set_pulse_width(Duration::from_micros((value.0 * 1000.0).round() as u64))
            .unwrap();
        self.set_period(Duration::from_micros(1000)).unwrap();
        thread::sleep(Duration::from_millis(10));
    }
    fn enable(&mut self) {
        println!("Enabling hardware PWM.");
        Pwm::enable(self).expect("failed to enable hardware PWM");
    }
    fn disable(&mut self) {
        println!("Disabling hardware PWM.");
        Pwm::disable(self).expect("failed to disable hardware PWM");
    }
}
impl VariableOut for OutputPin {
    fn set(&mut self, value: Strength) {
        self.set_pwm(
            Duration::from_micros(1000),
            Duration::from_micros((value.0 * 1000.0).round() as u64),
        )
        .unwrap();
    }
    fn enable(&mut self) {}
    fn disable(&mut self) {
        OutputPin::clear_pwm(self).expect("failed to stop software PWM")
    }
}

pub struct PrintOut;
impl VariableOut for PrintOut {
    fn set(&mut self, value: Strength) {
        println!("Got strength {:?}", value);
        thread::sleep(Duration::from_millis(100));
    }
    fn enable(&mut self) {
        println!("Enabling output");
    }
    fn disable(&mut self) {
        println!("Disabling output");
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
enum Sleeping {
    Forever,
    To(Instant),
    Wake,
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
#[derive(Debug)]
pub struct Controller<T: VariableOut + Send + 'static> {
    channel: mpsc::SyncSender<Command>,
    handle: thread::JoinHandle<T>,
}
impl<T: VariableOut + Send + 'static> Controller<T> {
    pub fn new(mut output: T, scheduler: scheduler::WeekScheduler) -> Self {
        let (sender, receiver) = mpsc::sync_channel(2);
        // make channel
        let handle = thread::spawn(move || {
            let receiver = receiver;
            let mut state = scheduler::State::new(scheduler);
            let mut sleeping: Sleeping = Sleeping::Wake;
            let mut enabled = None;
            loop {
                let command = match receiver.try_recv().ok() {
                    Some(r) => {
                        sleeping = Sleeping::Wake;
                        Some(r)
                    }
                    None => match sleeping {
                        Sleeping::To(instant) => {
                            match instant.checked_duration_since(Instant::now()) {
                                Some(_) => {
                                    thread::sleep(Duration::from_millis(1));
                                    continue;
                                }
                                None => None,
                            }
                        }
                        Sleeping::Forever => {
                            thread::sleep(Duration::from_millis(1));
                            continue;
                        }
                        Sleeping::Wake => None,
                    },
                };
                let action = state.process(command);
                match action {
                    Action::Wait(sleep) => {
                        if enabled == Some(0.0) {
                            output.disable();
                            enabled = None;
                        }
                        match sleep {
                            scheduler::SleepTime::Duration(dur) => {
                                sleeping = Sleeping::To(Instant::now() + dur)
                            }
                            scheduler::SleepTime::Forever => sleeping = Sleeping::Forever,
                        }
                    }
                    Action::Set(s) => {
                        if enabled.unwrap_or(0.0) == 0.0 {
                            output.enable();
                        }
                        output.set(s);
                        enabled = Some(s.into_inner());
                    }
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

    pub fn send(&self, command: Command) {
        match &command {
            Command::Set(_) => {
                let _ = self.channel.try_send(command);
            }
            _ => self
                .channel
                .send(command)
                .expect("failed to send message on channel"),
        }
    }

    /// Will wait on any transitions to conclude and then give back the underlying object
    pub fn finish(self) -> T {
        self.send(Command::Finish);
        self.handle.join().expect("child thread paniced")
    }
}
