use std::fmt::Debug;

use crate::{
    Action, Command, Duration, Instant, SharedState, Strength, Transition, TransitionInterpolation,
};
use chrono::prelude::*;
use std::sync::{Arc, Mutex};

pub enum Keep {
    Keep,
    Remove,
}
pub trait Scheduler: Debug + Send {
    /// Advances the internal state when the scheduled time in [`Scheduler::get_next()`] is reached.
    /// You can specify if you want to persist in the list of schedulers or be removed.
    fn advance(&mut self) -> Keep;
    /// Main function. It gets the time to the next occurrence of this Scheduler.
    fn get_next(&self) -> Option<(Duration, Command)>;
    /// A description to show the user. Should contain information about what this scheduler wakes up to do.
    fn description(&self) -> &str;
}

#[derive(Debug, PartialEq, Clone)]
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

    pub fn get_next_from_day(&self, day: Weekday) -> Option<(&NaiveTime, u8)> {
        let mut day = day.pred();
        for passed in 0..7 {
            day = day.succ();
            let time = self.get(day);
            if time.is_some() {
                return time.as_ref().map(|t| (t, passed));
            }
        }
        None
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
    fn advance(&mut self) -> Keep {
        self.current = self.current.succ();
        Keep::Keep
    }
    fn get_next(&self) -> Option<(Duration, Command)> {
        let now = Local::now();
        let next = self.get_next_from_day(now.weekday()).map(|(t, _)| *t);
        match next {
            Some(next) => {
                let next = if now.time()
                    < next
                        - chrono::Duration::from_std(self.transition.time)
                            .unwrap_or(chrono::Duration::zero())
                {
                    now.date().and_time(next).expect("got invalid DateTime")
                } else {
                    let (time, day) = self.get_next_from_day(now.weekday().succ())?;
                    // Since we get the next day from function
                    let day = day + 1;

                    now.date().and_time(*time).expect("got invalid DateTime")
                        + chrono::Duration::days(day as i64)
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

    fn description(&self) -> &str {
        "Can schedule once per weekday, repeating every week."
    }
}
impl Default for WeekScheduler {
    fn default() -> Self {
        Self::empty(Transition::default())
    }
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub enum TransitionStateOut {
    Ongoing(Strength),
    Finished(Strength),
}
impl TransitionStateOut {
    pub fn remap_and_check_finish(
        transition: &Transition,
        strength: f64,
        progress: f64,
        finish: f64,
    ) -> Self {
        let remapped = Self::remap(strength, transition.from.0, transition.to.0);
        if progress >= finish {
            Self::Finished(Strength::new_clamped(remapped))
        } else {
            Self::Ongoing(Strength::new(remapped))
        }
    }
    fn remap(zero_to_one: f64, zero: f64, one: f64) -> f64 {
        // 0, 0, 1 => 0
        // 0.25, 0, 1 => 0.25
        // 1, 0, 1 => 1
        // 0, 2, 0 => 2
        // 0.25, 2, 0 => 1.5
        // 1, 2, 0 => 0
        // range = one-zero
        // add zero?
        // 0 * (1-0) + 0 = 0
        // 0.25 * (1-0) + 0 = 0.25
        // 1 * (1-0) + 0 = 1
        // 0 * (0-2) + 2 = 2
        // 0.25 * (0-2) + 2 = 1.5
        // 1 * (0-2) + 2 = 0
        // ↑ f64 = zero_to_one * (one-zero) + zero
        zero_to_one * (one - zero) + zero
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

    pub fn process(&mut self, delta_time: &Duration) -> TransitionStateOut {
        let delta_progress = self.calculate_delta_progress(delta_time);
        const HALF_PI: f64 = core::f64::consts::PI / 2.0;
        const PI: f64 = core::f64::consts::PI;

        match self.transition.interpolation {
            TransitionInterpolation::Linear => {
                self.standard_interpolation(|progress| progress, delta_progress)
            }
            TransitionInterpolation::Sine => self.standard_interpolation(
                |progress| ((progress * PI - HALF_PI).sin() + 1.0) / 2.0,
                delta_progress,
            ),

            TransitionInterpolation::LinearToAndBack(multiplier) => {
                self.and_back_interpolation(|zero_to_one| zero_to_one, delta_progress, multiplier)
            }
            TransitionInterpolation::SineToAndBack(multiplier) => self.and_back_interpolation(
                |zero_to_one| ((zero_to_one * PI - HALF_PI).sin() + 1.0) / 2.0,
                delta_progress,
                multiplier,
            ),
        }
    }
    fn calculate_delta_progress(&self, delta_time: &Duration) -> f64 {
        delta_time.as_secs_f64() / self.transition.time.as_secs_f64()
    }
    fn standard_interpolation<F: Fn(f64) -> f64>(
        &mut self,
        strength: F,
        delta_progress: f64,
    ) -> TransitionStateOut {
        self.progress += delta_progress;
        TransitionStateOut::remap_and_check_finish(
            &self.transition,
            strength(self.progress),
            self.progress,
            1.0,
        )
    }
    fn and_back_interpolation<F: Fn(f64) -> f64>(
        &mut self,
        function: F,
        delta_progress: f64,
        multiplier: f64,
    ) -> TransitionStateOut {
        self.progress += delta_progress;
        let strength = if self.progress > 1.0 {
            function(1.0 - ((self.progress - 1.0) / multiplier))
        } else {
            function(self.progress)
        };
        TransitionStateOut::remap_and_check_finish(
            &self.transition,
            strength,
            self.progress,
            multiplier + 1.0,
        )
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub enum SleepTime {
    Duration(Duration),
    Forever,
}

pub struct State {
    // Data
    shared: Arc<Mutex<SharedState>>,

    finish: bool,
    wake_up: Option<(Instant, Command)>,
    transition: Option<TransitionState>,
    last_instance: Instant,
    last_scheduler: Option<String>,
}
impl State {
    pub fn new(state: Arc<Mutex<SharedState>>) -> Self {
        Self {
            shared: state,
            finish: false,
            wake_up: None,
            transition: None,
            last_instance: Instant::now(),
            last_scheduler: None,
        }
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
                    self.shared.lock().unwrap().strength = Strength::clone(&strength);
                    // send back set
                    Action::Set(strength)
                }
                Command::ChangeDayTimer(day, time) => {
                    // change time of day
                    {
                        *self.shared.lock().unwrap().week_schedule.get_mut(day) = time;
                    }
                    self.get_next()
                }
                Command::ChangeDayTimerTransition(new_transition) => {
                    {
                        self.shared.lock().unwrap().week_schedule.transition = new_transition;
                    }
                    self.get_next()
                }
                Command::AddReplaceScheduler(name, scheduler) => {
                    self.shared
                        .lock()
                        .unwrap()
                        .schedulers
                        .insert(name, scheduler);
                    self.get_next()
                }
                Command::RemoveScheduler(name) => {
                    self.shared.lock().unwrap().schedulers.remove(&name);
                    self.get_next()
                }
                Command::ClearAllSchedulers => {
                    self.shared.lock().unwrap().schedulers.clear();
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
                        {
                            let mut lock = self.shared.lock().unwrap();
                            match self.last_scheduler.as_ref() {
                                Some(name) => match lock.schedulers.get_mut(name) {
                                    Some(scheduler) => match scheduler.advance() {
                                        Keep::Keep => {}
                                        Keep::Remove => {
                                            lock.schedulers.remove(name);
                                        }
                                    },
                                    None => {
                                        panic!("attempting to get scheduler not existing. Did you clear the list?");
                                    }
                                },
                                None => {
                                    // Discarding, because we know it'll want to continue.
                                    lock.week_schedule.advance();
                                }
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
            match transition.process(&delta_time) {
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
        let next = {
            let lock = self.shared.lock().unwrap();
            let next = lock
                .schedulers
                .iter()
                .filter_map(|(_name, s)| s.get_next())
                .min_by_key(|(d, _)| *d);
            match Scheduler::get_next(&lock.week_schedule) {
                Some((dur, cmd)) => match next {
                    Some((next_dur, _)) => match dur < next_dur {
                        true => Some((dur, cmd)),
                        false => next,
                    },
                    None => Some((dur, cmd)),
                },
                None => next,
            }
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
