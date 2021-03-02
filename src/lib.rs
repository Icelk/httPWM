pub mod scheduler;
use chrono::{prelude::*, Duration};
pub use scheduler::Scheduler;
use std::sync::{Arc, Mutex};

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub struct Strength(f32);
impl Strength {
    pub fn new(value: f32) -> Self {
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
pub struct Controller;
impl Controller {
    pub fn new(pwm: rppal::pwm::Pwm) -> Self {
        // make channel
        // spawn thread, moving `pwm`
        // return Self with the channel and JoinHandle
        Self
    }
}
