pub mod scheduler;

use rppal::{gpio::OutputPin, pwm::Pwm};
pub use scheduler::{Next, Scheduler, WeekScheduler};
use std::collections::HashMap;
use std::str::FromStr;
use std::time::{Duration, Instant};
use std::{
    sync::{mpsc, Arc, Mutex},
    thread,
};
use time::OffsetDateTime;
use time_tz::{OffsetDateTimeExt, PrimitiveDateTimeExt};

lazy_static::lazy_static! {
    static ref TIMEZONE: Option<&'static time_tz::Tz> = time_tz::system::get_timezone().ok();
}
static DATE_TIME_TZ_FORMAT: &[time::format_description::FormatItem] = time::macros::format_description!(
    "[year]-[month]-[day] [hour]:[minute]:[second] [offset_hour sign:mandatory]:[offset_minute]"
);

pub fn get_timezone() -> Option<&'static time_tz::Tz> {
    *TIMEZONE
}
pub fn primitive_to_tz(datetime: time::PrimitiveDateTime) -> time::OffsetDateTime {
    let v = if let Some(tz) = get_timezone() {
        datetime.assume_timezone(tz).take_first()
    } else {
        None
    };
    v.unwrap_or_else(|| datetime.assume_utc())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}
impl Weekday {
    pub fn pred(self) -> Self {
        match self {
            Weekday::Mon => Self::Sun,
            Weekday::Tue => Self::Mon,
            Weekday::Wed => Self::Tue,
            Weekday::Thu => Self::Wed,
            Weekday::Fri => Self::Thu,
            Weekday::Sat => Self::Fri,
            Weekday::Sun => Self::Sat,
        }
    }
    pub fn succ(self) -> Self {
        match self {
            Weekday::Mon => Self::Tue,
            Weekday::Tue => Self::Wed,
            Weekday::Wed => Self::Thu,
            Weekday::Thu => Self::Fri,
            Weekday::Fri => Self::Sat,
            Weekday::Sat => Self::Sun,
            Weekday::Sun => Self::Mon,
        }
    }
}
impl FromStr for Weekday {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "mon" => Self::Mon,
            "tue" => Self::Tue,
            "wed" => Self::Wed,
            "thu" => Self::Thu,
            "fri" => Self::Fri,
            "sat" => Self::Sat,
            "sun" => Self::Sun,
            _ => return Err(()),
        })
    }
}
impl From<time::Weekday> for Weekday {
    fn from(w: time::Weekday) -> Self {
        match w {
            time::Weekday::Monday => Self::Mon,
            time::Weekday::Tuesday => Self::Tue,
            time::Weekday::Wednesday => Self::Wed,
            time::Weekday::Thursday => Self::Thu,
            time::Weekday::Friday => Self::Fri,
            time::Weekday::Saturday => Self::Sat,
            time::Weekday::Sunday => Self::Sun,
        }
    }
}

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
        Some(match string {
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
            interpolation: TransitionInterpolation::SineToAndBack(0.5),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum Effect {
    Radar { offset: f64, speed: f64 },
}

#[derive(Debug)]
pub enum Command {
    Set(Strength),
    SetTransition(Transition),
    ChangeDayTimer(Weekday, Option<time::Time>),
    ChangeDayTimerTransition(Transition),
    AddReplaceScheduler(String, Box<dyn Scheduler>),
    RemoveScheduler(String),
    ClearAllSchedulers,
    SetEffect(Effect),
    Finish,
}
impl Command {
    pub fn can_clone(&self) -> bool {
        match self {
            Self::Set(_)
            | Self::SetTransition(_)
            | Self::ChangeDayTimer(_, _)
            | Self::ChangeDayTimerTransition(_)
            | Self::RemoveScheduler(_)
            | Self::ClearAllSchedulers
            | Self::SetEffect(_)
            | Self::Finish => true,
            Self::AddReplaceScheduler(_, _) => false,
        }
    }
}

#[derive(Debug)]
pub struct ClonableCommand(Command);
impl ClonableCommand {
    pub fn new(command: Command) -> Option<Self> {
        if command.can_clone() {
            Some(Self(command))
        } else {
            None
        }
    }
    pub fn into_inner(self) -> Command {
        self.0
    }
}
impl Clone for ClonableCommand {
    fn clone(&self) -> Self {
        Self(match &self.0 {
            Command::Set(s) => Command::Set(Strength::clone(s)),
            Command::SetTransition(t) => Command::SetTransition(Transition::clone(t)),
            Command::ChangeDayTimer(d, t) => Command::ChangeDayTimer(*d, *t),
            Command::ChangeDayTimerTransition(t) => {
                Command::ChangeDayTimerTransition(Transition::clone(t))
            }
            Command::RemoveScheduler(s) => Command::RemoveScheduler(String::clone(s)),
            Command::ClearAllSchedulers => Command::ClearAllSchedulers,
            Command::SetEffect(e) => Command::SetEffect(e.clone()),
            Command::Finish => Command::Finish,

            Command::AddReplaceScheduler(_, _) => {
                unreachable!("should have been checked when creating `ClonableCommand`")
            }
        })
    }
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

    /// Used to prepare the out device. Used for optimizing; internal guarantees.
    fn prepare(&mut self);
}
impl VariableOut for Pwm {
    fn set(&mut self, value: Strength) {
        self.set_pulse_width(Duration::from_nanos((value.0 * 1000000.0).round() as u64))
            .unwrap();
    }
    fn enable(&mut self) {
        println!("Enabling hardware PWM.");
        Pwm::enable(self).expect("failed to enable hardware PWM");
    }
    fn disable(&mut self) {
        println!("Disabling hardware PWM.");
        Pwm::disable(self).expect("failed to disable hardware PWM");
    }
    fn prepare(&mut self) {
        self.set_pulse_width(Duration::new(0, 0))
            .expect("failed to set pulse width to guarantee period");
        self.set_period(Duration::new(0, 1000000))
            .expect("failed to set period in `enable()`");
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
    fn prepare(&mut self) {}
}

pub struct PrintOut(pub mpsc::SyncSender<f64>);
impl VariableOut for PrintOut {
    fn set(&mut self, value: Strength) {
        if self.0.send(value.0).is_err() {
            println!("Got strength {:?}", value);
        }
    }
    fn enable(&mut self) {
        println!("Enabling output");
    }
    fn disable(&mut self) {
        println!("Disabling output");
    }
    fn prepare(&mut self) {
        println!("Preparing device");
    }
}

pub fn get_now() -> time::OffsetDateTime {
    let time = time::OffsetDateTime::now_utc();
    get_timezone().map_or(time, |offset| time.to_timezone(offset))
}

#[derive(Debug)]
pub struct SharedState {
    strength: Strength,
    transition: Option<Transition>,
    week_scheduler: WeekScheduler,
    schedulers: HashMap<String, Box<dyn Scheduler>>,
}
impl SharedState {
    pub fn new(scheduler: WeekScheduler) -> Self {
        Self {
            strength: Strength::new(0.0),
            transition: None,
            week_scheduler: scheduler,
            schedulers: HashMap::new(),
        }
    }

    pub fn get_strength(&self) -> &Strength {
        &self.strength
    }
    pub fn set_strength(&mut self, strength: Strength) {
        self.strength = strength;
        self.transition = None;
    }

    pub fn get_transition(&self) -> Option<&Transition> {
        self.transition.as_ref()
    }
    pub fn set_transition(&mut self, transition: Option<Transition>) {
        self.transition = transition;
    }

    pub fn ref_week_schedule(&self) -> &WeekScheduler {
        &self.week_scheduler
    }
    pub fn mut_week_scheduler(&mut self) -> &mut WeekScheduler {
        &mut self.week_scheduler
    }

    pub fn ref_schedulers(&self) -> &HashMap<String, Box<dyn Scheduler>> {
        &self.schedulers
    }
    pub fn mut_schedulers(&mut self) -> &mut HashMap<String, Box<dyn Scheduler>> {
        &mut self.schedulers
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
    To(OffsetDateTime),
    Wake,
    Forever,
}

pub fn has_occurred(date_time: OffsetDateTime) -> bool {
    let now = get_now();
    (date_time - now) < time::Duration::ZERO
}

/// Subset of [`Controller`] which can send.
#[derive(Debug)]
pub struct ControllerSender {
    channel: mpsc::SyncSender<Command>,
}
impl ControllerSender {
    pub fn send(&self, command: Command) {
        match &command {
            Command::Set(_) => {
                self.channel.send(command).unwrap();
            }
            _ => self
                .channel
                .send(command)
                .expect("failed to send message on channel"),
        }
    }
}
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
    pub fn new(mut output: T, scheduler: WeekScheduler) -> Self {
        // make channel
        let (sender, receiver) = mpsc::sync_channel(128);

        let shared_state = Arc::new(Mutex::new(SharedState::new(scheduler)));

        let shared = Arc::clone(&shared_state);

        let handle = thread::spawn(move || {
            let receiver = receiver;
            let mut state = scheduler::State::new(shared);
            let mut sleeping: Sleeping = Sleeping::Wake;
            let mut enabled = None;

            output.prepare();

            loop {
                let command = receiver.try_recv().ok();
                #[cfg(not(feature = "test"))]
                let transition_sleep_duration = Duration::from_millis(10);
                #[cfg(feature = "test")]
                let transition_sleep_duration = Duration::from_millis(100);
                let sleep = match sleeping {
                    Sleeping::To(date_time) => {
                        (date_time - get_now() - time::Duration::milliseconds(2))
                            .max(time::Duration::ZERO)
                            .unsigned_abs()
                    }
                    Sleeping::Forever => Duration::MAX,
                    // this is part of a transition
                    Sleeping::Wake if command.is_none() => transition_sleep_duration,
                    Sleeping::Wake => Duration::ZERO,
                };

                let rx = command.or_else(|| {
                    if sleep.is_zero() {
                        // we already tried to recv
                        None
                    } else {
                        receiver.recv_timeout(sleep).ok()
                    }
                });
                let command = match rx {
                    Some(r) => {
                        sleeping = Sleeping::Wake;
                        Some(r)
                    }
                    None => match sleeping {
                        Sleeping::Wake => None,
                        _ => continue,
                    },
                };
                let action = state.process(command);
                match action {
                    Action::Wait(sleep_time) => match sleep_time {
                        scheduler::SleepTime::To(date_time) => {
                            if enabled.map(|value| value == 0.0).unwrap_or(false) {
                                output.disable();
                                enabled = None;
                            }
                            println!(
                                "Sleeping to {}",
                                date_time.format(&DATE_TIME_TZ_FORMAT).unwrap()
                            );
                            sleeping = Sleeping::To(date_time)
                        }
                        scheduler::SleepTime::Forever => sleeping = Sleeping::Forever,
                    },
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

    pub fn to_sender(&self) -> ControllerSender {
        ControllerSender {
            channel: self.channel.clone(),
        }
    }

    pub fn send(&self, command: Command) {
        match &command {
            Command::Set(_) => {
                let _ = self.channel.send(command);
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
