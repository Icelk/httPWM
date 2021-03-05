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
    /// Main function. It gets the time to the next occurrence of this Scheduler.
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
                // The expect here should never happen, we checked above if now is less than next.
                let next = (next - now)
                    .to_std()
                    .expect("duration is negative")
                    .checked_sub(self.transition.time)
                    .unwrap_or(Duration::new(0, 0));
                Some((
                    next,
                    Command::SetTransition(Transition::clone(&self.transition)),
                ))
            }
            None => None,
        }
    }
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub enum TransitionStateOut {
    Ongoing(Strength),
    Finished(Strength),
}
impl TransitionStateOut {
    pub fn new(strength: f64, progress: f64, finish: f64) -> Self {
        if progress >= finish {
            Self::Finished(Strength::new_clamped(strength))
        } else {
            Self::Ongoing(Strength::new(strength))
        }
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

    pub fn process(&mut self, delta_time: Duration) -> TransitionStateOut {
        match self.transition.interpolation {
            TransitionInterpolation::Linear => {
                self.progress += delta_time.as_secs_f64() / self.transition.time.as_secs_f64();
                TransitionStateOut::new(self.progress, self.progress, 1.0)
            }
            TransitionInterpolation::Sine => {
                const HALF_PI: f64 = core::f64::consts::PI / 2.0;
                let advanced = delta_time.as_secs_f64() / self.transition.time.as_secs_f64();
                self.progress += advanced;
                let strength =
                    ((self.progress * core::f64::consts::PI - HALF_PI).sin() + 1.0) / 2.0;
                TransitionStateOut::new(strength, self.progress, 1.0)
            }

            TransitionInterpolation::LinearToAndBack(multiplier) => {
                self.progress += delta_time.as_secs_f64() / self.transition.time.as_secs_f64();
                let strength = if self.progress > 1.0 {
                    1.0 - ((self.progress - 1.0) / multiplier)
                } else {
                    self.progress
                };
                TransitionStateOut::new(strength, self.progress, multiplier + 1.0)
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
    wake_up: Option<(Instant, Command)>,
    transition: Option<TransitionState>,
    last_instance: Instant,
    last_scheduler: Option<usize>,
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
            last_scheduler: None,
        }
    }
    pub fn add_scheduler(&mut self, scheduler: Box<dyn Scheduler>) {
        self.schedulers.push(scheduler);
    }

    pub fn process(&mut self, command: Option<Command>) -> Action {
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
                match self.wake() {
                    Some(command) => {
                        match self.last_scheduler {
                            Some(index) => match self.schedulers.get_mut(index) {
                                Some(scheduler) => match scheduler.add() {
                                    Keep::Keep => {}
                                    Keep::Remove => {
                                        self.schedulers.remove(index);
                                    }
                                },
                                None => {
                                    panic!("attempting to get scheduler not existing. Did you clear the list?");
                                }
                            },
                            None => {
                                self.day_schedule.add();
                            }
                        }
                        let action = self.process(Some(command));

                        action
                    }
                    // check internal transition state; get_output()
                    None => match self.get_transition_output() {
                        Some(s) => Action::Set(s),
                        // check finish flag
                        None => match self.finish {
                            true => Action::Break,
                            // in ↓ make sure a variable is stored of what to do when you've been woken up.
                            // else, send sleep command 'till schedulers.iter().min()
                            false => Action::Wait(self.queue_sleep()),
                        },
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
            let transition = self.transition.as_mut().unwrap();
            match transition.process(delta_time) {
                TransitionStateOut::Finished(s) => {
                    self.transition = None;
                    Some(s)
                }
                TransitionStateOut::Ongoing(s) => Some(s),
            }
        } else {
            None
        }
    }
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
                self.wake_up = Some((Instant::now() + dur, cmd));
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
    fn wake(&mut self) -> Option<Command> {
        match self.wake_up.as_ref() {
            Some((when, _)) => match when.checked_duration_since(Instant::now()) {
                Some(_) => None,
                None => Some(self.wake_up.take().unwrap().1),
            },
            None => None,
        }
    }
}
