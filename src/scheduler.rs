use std::fmt::Debug;

use crate::{
    get_now, has_occurred, primitive_to_tz, Action, Command, Duration, Effect, Instant,
    SharedState, Strength, Transition, TransitionInterpolation, Weekday,
};
use std::convert::TryFrom;
use std::sync::{Arc, Mutex};
use time::{OffsetDateTime, Time};

pub enum Progress {
    Pending(Duration),
    Ready(Command),
    Error,
}
pub enum Keep {
    Keep,
    Remove,
}
pub enum Next {
    At(OffsetDateTime, Command),
    Unknown,
}
/// Now, represented as a [`time::OffsetDateTime`], being lazily evaluated.
/// Should not be used long after it's initiation, since `now` stays the same after the first call to [`LazyNow::now()`].
pub struct LazyNow {
    now: Option<OffsetDateTime>,
}
impl LazyNow {
    pub fn new() -> Self {
        Self { now: None }
    }
    pub fn now(&mut self) -> OffsetDateTime {
        match self.now {
            Some(now) => now,
            None => {
                self.now = Some(get_now());
                match self.now {
                    Some(now) => now,
                    None => panic!("we just replaced the value with Some"),
                }
            }
        }
    }
}

impl Default for LazyNow {
    fn default() -> Self {
        Self::new()
    }
}

pub trait Scheduler: Debug + Send + Sync {
    /// Advances the internal state when the scheduled time in [`Scheduler::get_next()`] is reached.
    /// You can specify if you want to persist in the list of schedulers or be removed.
    fn advance(&mut self) -> Keep;
    /// Main function. It gets the time to the next occurrence of this Scheduler.
    fn get_next(&self, now: &mut LazyNow) -> Next;
    /// A description to show the user. Should contain information about what this scheduler wakes up to do.
    /// Should only be used as a tip for users.
    fn description(&self) -> &str;
    /// Which type this scheduler is of.
    /// Should be used as a tip for users.
    fn kind(&self) -> &str;
}

#[derive(Debug, PartialEq, Clone)]
pub struct WeekScheduler {
    pub mon: Option<Time>,
    pub tue: Option<Time>,
    pub wed: Option<Time>,
    pub thu: Option<Time>,
    pub fri: Option<Time>,
    pub sat: Option<Time>,
    pub sun: Option<Time>,
    pub transition: Transition,
    last: Option<OffsetDateTime>,
}
impl WeekScheduler {
    pub fn empty(transition: Transition) -> Self {
        Self::same_with_day(None, transition)
    }
    fn same_with_day(time: Option<Time>, transition: Transition) -> Self {
        Self {
            mon: time,
            tue: time,
            wed: time,
            thu: time,
            fri: time,
            sat: time,
            sun: time,
            transition,
            last: None,
        }
    }

    pub fn same(time: Time, transition: Transition) -> Self {
        Self::same_with_day(Some(time), transition)
    }

    pub fn get_next_from_day(&self, day: Weekday) -> Option<(&time::Time, u8)> {
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
    pub fn get(&self, day: Weekday) -> &Option<time::Time> {
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
    pub fn get_mut(&mut self, day: Weekday) -> &mut Option<time::Time> {
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
        self.last = Some(get_now());
        Keep::Keep
    }
    fn get_next(&self, now: &mut LazyNow) -> Next {
        let now = now.now();
        let next_today = match self.get(now.weekday().into()) {
            Some(t) => *t,
            None => return Next::Unknown,
        };

        // Check if last was not today, then abort.
        let next = if now.time() < next_today
            && self
                .last
                .map(|l| l.date() < get_now().date())
                .unwrap_or(true)
        {
            now.date().with_time(next_today)
        } else {
            // Unwrap is ok; we checked the same function above and return if it was `None`.
            // If it returns `Some` for Weekday x, it will also return `Some` for Weekday x.succ(); it's a cycle.
            let (time, day) = self
                .get_next_from_day(Weekday::from(now.weekday()).succ())
                .unwrap();
            // Since we get the next day from function
            let day = day + 1;

            now.date().with_time(*time) + time::Duration::days(day as i64)
        };
        // if your transition time is larger than what std can handle, you have other problems
        let next =
            next - time::Duration::try_from(self.transition.time).unwrap_or(time::Duration::MAX);
        Next::At(
            primitive_to_tz(next),
            Command::SetTransition(Transition::clone(&self.transition)),
        )
    }

    fn description(&self) -> &str {
        "Can schedule once per weekday, repeating every week."
    }

    fn kind(&self) -> &str {
        "Weekly cycle"
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
    fn remap(zero_to_one: f64, zero: f64, one: f64) -> f64 {
        zero_to_one * (one - zero) + zero
    }
    fn remap_and_check_finish(&self, strength: f64, finish: f64) -> TransitionStateOut {
        let remapped = Self::remap(strength, self.transition.from.0, self.transition.to.0);
        if self.progress >= finish {
            TransitionStateOut::Finished(Strength::new_clamped(remapped))
        } else {
            TransitionStateOut::Ongoing(Strength::new(remapped))
        }
    }
    fn standard_interpolation<F: Fn(f64) -> f64>(
        &mut self,
        strength: F,
        delta_progress: f64,
    ) -> TransitionStateOut {
        self.progress += delta_progress;
        self.remap_and_check_finish(strength(self.progress), 1.0)
    }
    fn and_back_interpolation<F: Fn(f64) -> f64>(
        &mut self,
        function: F,
        delta_progress: f64,
        multiplier: f64,
    ) -> TransitionStateOut {
        self.progress += delta_progress;
        let progress = self.progress.clamp(0.0, multiplier + 1.0);
        let strength = if progress > 1.0 {
            function(1.0 - ((progress - 1.0) / multiplier))
        } else {
            function(progress)
        };
        self.remap_and_check_finish(strength, multiplier + 1.0)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub enum SleepTime {
    To(OffsetDateTime),
    Forever,
}

pub struct State {
    // Data
    shared: Arc<Mutex<SharedState>>,

    finish: bool,
    wake_up: Option<(OffsetDateTime, Command)>,
    transition: Option<TransitionState>,
    last_instance: Instant,
    last_scheduler: Option<String>,
    effect: Option<Effect>,
}
impl State {
    pub fn new(state: Arc<Mutex<SharedState>>) -> Self {
        Self {
            shared: state,
            finish: false,
            wake_up: None,
            transition: None,
            effect: None,
            last_instance: Instant::now(),
            last_scheduler: None,
        }
    }

    pub fn process(&mut self, command: Option<Command>) -> Action {
        match command {
            Some(Command::Finish) => {
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
            Some(Command::Set(strength)) => {
                // clear animation
                self.transition = None;
                self.effect = None;
                self.shared.lock().unwrap().set_strength(strength);
                // send back set
                Action::Set(strength)
            }
            Some(Command::ChangeDayTimer(day, time)) => {
                // change time of day
                {
                    let mut lock = self.shared.lock().unwrap();
                    *lock.mut_week_scheduler().get_mut(day) = time;
                    lock.mut_week_scheduler().last = None;
                }
                self.get_next()
            }
            Some(Command::ChangeDayTimerTransition(new_transition)) => {
                {
                    self.shared.lock().unwrap().mut_week_scheduler().transition = new_transition;
                }
                self.get_next()
            }
            Some(Command::AddReplaceScheduler(name, scheduler)) => {
                self.shared
                    .lock()
                    .unwrap()
                    .mut_schedulers()
                    .insert(name, scheduler);
                self.get_next()
            }
            Some(Command::RemoveScheduler(name)) => {
                self.shared.lock().unwrap().mut_schedulers().remove(&name);
                self.get_next()
            }
            Some(Command::ClearAllSchedulers) => {
                self.shared.lock().unwrap().mut_schedulers().clear();
                self.get_next()
            }
            Some(Command::SetTransition(transition)) => {
                self.shared
                    .lock()
                    .unwrap()
                    .set_transition(Some(Transition::clone(&transition)));
                self.transition = Some(TransitionState::new(transition));
                self.last_instance = Instant::now();
                // unwrap() is ok; we've just set transition to be `Some`
                Action::Set(self.get_transition_output().unwrap())
            }
            Some(Command::SetEffect(e)) => {
                self.effect = Some(e);
                Action::Set(self.get_transition_output().unwrap())
            }
            None | Some(Command::UpdateWake) => {
                // check wake up Option<>
                match self.wake() {
                    Some(command) => {
                        {
                            let mut lock = self.shared.lock().unwrap();
                            match self.last_scheduler.as_ref() {
                                Some(name) => match lock.mut_schedulers().get_mut(name) {
                                    Some(scheduler) => match scheduler.advance() {
                                        Keep::Keep => {}
                                        Keep::Remove => {
                                            lock.mut_schedulers().remove(name);
                                        }
                                    },
                                    None => {
                                        panic!("attempting to get scheduler not existing. Did you clear the list?");
                                    }
                                },
                                None => {
                                    // Discarding, because we know it'll want to continue.
                                    lock.mut_week_scheduler().advance();
                                }
                            }
                            self.last_scheduler = None;
                        }

                        self.process(Some(command))
                    }
                    // check internal transition state; get_output()
                    None => self.get_next(),
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
            return match transition.process(&delta_time) {
                TransitionStateOut::Finished(s) => {
                    self.shared
                        .lock()
                        .unwrap()
                        .set_strength(Strength::clone(&s));
                    self.transition = None;
                    Some(s)
                }
                TransitionStateOut::Ongoing(s) => Some(s),
            };
        }
        if let Some(effect) = &self.effect {
            let now = (get_now() - OffsetDateTime::UNIX_EPOCH).as_seconds_f64();
            let s = match effect {
                Effect::Radar { offset, speed } => {
                    // - offset because we are taking 1-↓, offset should always make a remote
                    // act in the future
                    let v = 1. - (((now - offset) / speed) % 1.);
                    v * v * v
                }
            };
            return Some(Strength(s));
        }
        None
    }
    fn queue_sleep(&mut self) -> SleepTime {
        self.last_scheduler = None;
        let (date_time, cmd, name) = {
            let lock = self.shared.lock().unwrap();

            let mut now = LazyNow::new();

            let schedulers_next = lock
                .ref_schedulers()
                .iter()
                .map(|(name, s)| (name, s.get_next(&mut now)))
                .min_by_key(|(_, next)| match next {
                    Next::At(d, _) => *d,
                    Next::Unknown => unreachable!(".retain() call above"),
                });

            let week_next = Scheduler::get_next(lock.ref_week_schedule(), &mut now);
            match week_next {
                Next::At(week_dur, week_cmd) => match schedulers_next {
                    Some((name, schedulers_next)) => match schedulers_next {
                        Next::At(schedulers_dur, schedulers_cmd) => match schedulers_dur < week_dur
                        {
                            true => (schedulers_dur, schedulers_cmd, Some(name.to_string())),
                            false => (week_dur, week_cmd, None),
                        },
                        Next::Unknown => unreachable!(".retain() call above"),
                    },
                    None => (week_dur, week_cmd, None),
                },
                Next::Unknown => match schedulers_next {
                    Some((name, next)) => match next {
                        Next::At(dur, cmd) => (dur, cmd, Some(name.to_string())),
                        Next::Unknown => unreachable!(".retain() call above"),
                    },
                    None => return SleepTime::Forever,
                },
            }
        };

        if let Some(name) = name {
            self.last_scheduler = Some(name);
        }

        self.wake_up = Some((date_time, cmd));
        SleepTime::To(date_time)
    }
    fn get_next(&mut self) -> Action {
        match self.get_transition_output() {
            Some(s) => Action::Set(s),
            None => match self.finish {
                true => Action::Break,
                false => Action::Wait(self.queue_sleep()),
            },
        }
    }
    fn wake(&mut self) -> Option<Command> {
        match has_occurred(self.wake_up.as_ref()?.0) {
            false => None,
            true => Some(self.wake_up.take().unwrap().1),
        }
    }
}
