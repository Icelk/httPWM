use std::fmt::Debug;

use crate::{Action, Command, Duration, Instant, Strength, Transition, TransitionInterpolation};
use chrono::prelude::*;

pub enum Keep {
    Keep,
    Remove,
}
pub trait Scheduler: Debug + Send {
    fn add(&mut self) -> Keep {
        Keep::Remove
    }
    fn get_next(&self) -> Option<(Duration, Command)>;
}

#[derive(Debug, PartialEq)]
pub struct WeekScheduler {
    pub mon: Option<NaiveTime>,
    pub tue: Option<NaiveTime>,
    pub wed: Option<NaiveTime>,
    pub thu: Option<NaiveTime>,
    pub fri: Option<NaiveTime>,
    pub sat: Option<NaiveTime>,
    pub sun: Option<NaiveTime>,
    current: Weekday,
    pub transition: Transition,
}
impl WeekScheduler {
    pub fn empty(transition: Transition) -> Self {
        Self::same_with_day(Local::today().weekday(), None, transition)
    }
    fn same_with_day(day: Weekday, time: Option<NaiveTime>, transition: Transition) -> Self {
        Self {
            mon: time,
            tue: time,
            wed: time,
            thu: time,
            fri: time,
            sat: time,
            sun: time,
            current: day,
            transition,
        }
    }

    pub fn same(time: NaiveTime, transition: Transition) -> Self {
        Self::same_with_day(Local::today().weekday(), Some(time), transition)
    }

    pub fn get_next(&self) -> &Option<NaiveTime> {
        let day = self.current;
        for _ in 0..7 {
            let time = self.get(day.succ());
            if time.is_some() {
                return time;
            }
        }
        &None
    }
    pub fn get(&self, day: Weekday) -> &Option<NaiveTime> {
        match day {
            Weekday::Mon => &self.mon,
            Weekday::Tue => &self.tue,
            Weekday::Wed => &self.wed,
            Weekday::Thu => &self.thu,
            Weekday::Fri => &self.fri,
            Weekday::Sat => &self.sat,
            Weekday::Sun => &self.sun,
        }
    }
    pub fn get_mut(&mut self, day: Weekday) -> &mut Option<NaiveTime> {
        match day {
            Weekday::Mon => &mut self.mon,
            Weekday::Tue => &mut self.tue,
            Weekday::Wed => &mut self.wed,
            Weekday::Thu => &mut self.thu,
            Weekday::Fri => &mut self.fri,
            Weekday::Sat => &mut self.sat,
            Weekday::Sun => &mut self.sun,
        }
    }
}
impl Scheduler for WeekScheduler {
    fn add(&mut self) -> Keep {
        self.current = self.current.succ();
        Keep::Keep
    }
    fn get_next(&self) -> Option<(Duration, Command)> {
        let now = Local::now();
        let next = *self.get_next();
        match next {
            Some(next) => {
                let next = if now.time() < next {
                    now.date().and_time(next).unwrap()
                } else {
                    now.date().succ().and_time(next).unwrap()
                };
                Some((
                    (next - now).to_std().expect("duration is negative"),
                    Command::SetTransition(Transition::clone(&self.transition)),
                ))
            }
            None => None,
        }
    }
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub struct RepeatingScheduler(pub NaiveTime, pub Transition);
impl Scheduler for RepeatingScheduler {
    fn get_next(&self) -> Option<(Duration, Command)> {
        let now = Local::now();
        let next = self.0;
        let next = if now.time() < next {
            now.date().and_time(next).unwrap()
        } else {
            now.date().succ().and_time(next).unwrap()
        };
        Some((
            (next - now).to_std().expect("duration is negative"),
            Command::SetTransition(Transition::clone(&self.1)),
        ))
    }
    fn add(&mut self) -> Keep {
        Keep::Keep
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct TransitionState {
    transition: Transition,
    progress: f64,
}
impl TransitionState {
    pub fn new(transition: Transition) -> Self {
        Self {
            transition,
            progress: 0.0,
        }
    }

    pub fn process(&mut self, delta_time: Duration) -> Strength {
        match self.transition.interpolation {
            TransitionInterpolation::Linear => {
                self.progress += self.transition.time.as_secs_f64() / delta_time.as_secs_f64();
                Strength::new_clamped(self.progress)
            }
            TransitionInterpolation::Sine => {
                self.progress += self.transition.time.as_secs_f64() / delta_time.as_secs_f64();
                // if progress is 1, then output should be 1. a range of 0-PI yields 0-1-0, therefore 0-2/PI yields 0-1
                Strength::new_clamped((self.progress * core::f64::consts::PI / 2.0).sin())
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub enum SleepTime {
    Duration(Duration),
    Forever,
}

pub struct State {
    // Data
    day_schedule: WeekScheduler,
    schedulers: Vec<Box<dyn Scheduler>>,

    finish: bool,
    wake_up: Option<Command>,
    transition: Option<TransitionState>,
    last_instance: Instant,
}
impl State {
    pub fn new(scheduler: WeekScheduler) -> Self {
        Self {
            day_schedule: scheduler,
            schedulers: Vec::new(),
            finish: false,
            wake_up: None,
            transition: None,
            last_instance: Instant::now(),
        }
    }
    pub fn add_scheduler(&mut self, scheduler: Box<dyn Scheduler>) {
        self.schedulers.push(scheduler);
    }

    pub fn process(&mut self, command: Option<Command>) -> Action {
        println!("Processing {:?}", command);
        match command {
            Some(command) => match command {
                Command::Finish => {
                    // set finish flag
                    self.finish = true;
                    match self.get_transition_output() {
                        // if no animation is going, return break
                        None => Action::Break,
                        // else return get_output
                        // Ok, because a transition exists, which will yield a value
                        Some(s) => Action::Set(s),
                    }
                }
                Command::Set(strength) => {
                    // clear animation
                    self.transition = None;
                    // send back set
                    Action::Set(strength)
                }
                Command::ChangeDayTimer(day, time) => {
                    // change time of day
                    *self.day_schedule.get_mut(day) = time;
                    // get_output
                    match self.get_transition_output() {
                        Some(s) => Action::Set(s),
                        // get_sleep
                        None => Action::Wait(self.queue_sleep()),
                    }
                }
                Command::ChangeDayTimerTransition(new_transition) => {
                    self.day_schedule.transition = new_transition;
                    self.get_next()
                }
                Command::AddScheduler(scheduler) => {
                    self.schedulers.push(scheduler);
                    self.get_next()
                }
                Command::ClearAllSchedulers => {
                    self.schedulers.clear();
                    self.get_next()
                }
                Command::SetTransition(transition) => {
                    self.transition = Some(TransitionState::new(transition));
                    self.last_instance = Instant::now();
                    // unwrap() is ok; we've just set transition to be `Some`
                    Action::Set(self.get_transition_output().unwrap())
                }
            },
            None => {
                // check wake up Option<>
                match self.wake_up.take() {
                    Some(command) => self.process(Some(command)),
                    // check internal transition state; get_output()
                    None => match self.get_transition_output() {
                        Some(s) => Action::Set(s),
                        None => {
                            // check finish flag
                            match self.finish {
                                true => Action::Break,
                                // in ↓ make sure a variable is stored of what to do when you've been woken up.
                                // else, send sleep command 'till schedulers.iter().min()
                                false => Action::Wait(self.queue_sleep()),
                            }
                        }
                    },
                }
            }
        }
    }

    fn get_delta_time(&mut self) -> Duration {
        let now = Instant::now();
        let difference = now - self.last_instance;
        self.last_instance = now;
        difference
    }

    fn get_transition_output(&mut self) -> Option<Strength> {
        if self.transition.is_some() {
            let delta_time = self.get_delta_time();
            // unwrap() is ok, since transition.is_some()
            Some(self.transition.as_mut().unwrap().process(delta_time))
        } else {
            None
        }
    }
    /// If None
    fn queue_sleep(&mut self) -> SleepTime {
        let next = self
            .schedulers
            .iter()
            .filter_map(|s| s.get_next())
            .min_by_key(|(d, _)| *d);
        let next = match Scheduler::get_next(&self.day_schedule) {
            Some((dur, cmd)) => match next {
                Some((next_dur, _)) => match dur < next_dur {
                    true => Some((dur, cmd)),
                    false => next,
                },
                None => Some((dur, cmd)),
            },
            None => next,
        };
        match next {
            Some((dur, cmd)) => {
                self.wake_up = Some(cmd);
                SleepTime::Duration(dur)
            }
            None => SleepTime::Forever,
        }
    }
    fn get_next(&mut self) -> Action {
        match self.get_transition_output() {
            Some(s) => Action::Set(s),
            // get_sleep
            None => Action::Wait(self.queue_sleep()),
        }
    }
}
