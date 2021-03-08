pub mod scheduler;
use chrono::prelude::*;
use rppal::{gpio::OutputPin, pwm::Pwm};
pub use scheduler::Scheduler;
use std::time::{Duration, Instant};
use std::{
    sync::{mpsc, Arc, Mutex},
    thread,
};

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
    /// Same as above, but with sine interpolation
    SineToAndBack(f64),
}
impl TransitionInterpolation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Sine => "sine",
            Self::LinearToAndBack(_) => "linear-extra",
            Self::SineToAndBack(_) => "sine-extra",
        }
    }
    pub fn from_str<S: AsRef<str>>(string: &str, extras: &[S]) -> Option<Self> {
        Some(match string.as_ref() {
            "linear" => Self::Linear,
            "sine" => Self::Sine,
            "linear-extra" if extras.len() == 1 => {
                Self::LinearToAndBack(extras[0].as_ref().parse().ok()?)
            }
            "sine-extra" if extras.len() == 1 => {
                Self::SineToAndBack(extras[0].as_ref().parse().ok()?)
            }
            _ => return None,
        })
    }
    pub fn apply_extras(&self, extras: &mut Vec<String>) {
        match self {
            Self::Linear | Self::Sine => {}
            Self::LinearToAndBack(extra) | Self::SineToAndBack(extra) => {
                extras.push(extra.to_string())
            }
        }
    }
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub struct Transition {
    pub from: Strength,
    pub to: Strength,
    pub time: Duration,
    pub interpolation: TransitionInterpolation,
}
impl Default for Transition {
    fn default() -> Self {
        Self {
            from: Strength::new(0.0),
            to: Strength::new(1.0),
            time: Duration::from_secs(15 * 60),
            interpolation: TransitionInterpolation::LinearToAndBack(0.5),
        }
    }
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

#[derive(Debug)]
pub struct SharedState {
    pub strength: Strength,
    pub week_schedule: scheduler::WeekScheduler,
}
impl SharedState {
    pub fn new(scheduler: &scheduler::WeekScheduler) -> Self {
        Self {
            strength: Strength::new(0.0),
            week_schedule: scheduler::WeekScheduler::clone(scheduler),
        }
    }
}

pub fn weekday_to_lowercase_str(weekday: &Weekday) -> &'static str {
    match *weekday {
        Weekday::Mon => "mon",
        Weekday::Tue => "tue",
        Weekday::Wed => "wed",
        Weekday::Thu => "thu",
        Weekday::Fri => "fri",
        Weekday::Sat => "sat",
        Weekday::Sun => "sun",
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
    shared_state: Arc<Mutex<SharedState>>,
}
impl<T: VariableOut + Send + 'static> Controller<T> {
    pub fn new(mut output: T, scheduler: scheduler::WeekScheduler) -> Self {
        // make channel
        let (sender, receiver) = mpsc::sync_channel(2);

        let shared_state = Arc::new(Mutex::new(SharedState::new(&scheduler)));

        let shared = Arc::clone(&shared_state);

        let handle = thread::spawn(move || {
            let receiver = receiver;
            let mut state = scheduler::State::new(scheduler, shared);
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
                        if enabled.map(|value| value < 0.01).unwrap_or(false) {
                            output.disable();
                            enabled = None;
                        }
                        match sleep {
                            scheduler::SleepTime::Duration(dur) => {
                                match chrono::Duration::from_std(dur) {
                                    Ok(dur) => println!("Sleeping to {:?}", Local::now() + dur),
                                    Err(_) => eprintln!("Sleeping to unknown date, failed to convert std::Duration to chrono::Duration when printing"),
                                }
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
            shared_state,
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

    /// Gets a reference counted [`SharedState`]
    /// The value should not be mutated, since it'll be overriden by the other thread.
    pub fn get_state(&self) -> Arc<Mutex<SharedState>> {
        Arc::clone(&self.shared_state)
    }
}
