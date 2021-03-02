use crate::Day;
use chrono::prelude::*;
use chrono::Duration;

pub trait Scheduler {
    fn add(self: Box<Self>) -> Option<Box<Self>> {
        None
    }
    fn get_next(&self) -> Duration;
}

pub struct WeekScheduler {
    pub mon: NaiveTime,
    pub tue: NaiveTime,
    pub wed: NaiveTime,
    pub thu: NaiveTime,
    pub fri: NaiveTime,
    pub sat: NaiveTime,
    pub sun: NaiveTime,
    current: Day,
}
impl WeekScheduler {
    pub fn get_next(&self) -> &NaiveTime {
        self.get(self.current.next())
    }
    pub fn get(&self, day: Day) -> &NaiveTime {
        match day {
            Day::Monday => &self.mon,
            Day::Tuesday => &self.tue,
            Day::Wednesday => &self.wed,
            Day::Thursday => &self.thu,
            Day::Friday => &self.fri,
            Day::Saturday => &self.sat,
            Day::Sunday => &self.sun,
        }
    }
}
impl Scheduler for WeekScheduler {
    fn add(mut self: Box<Self>) -> Option<Box<Self>> {
        self.current = self.current.next();
        Some(self)
    }
    fn get_next(&self) -> Duration {
        let now = Local::now();
        let next = *self.get_next();
        let next = if now.time() < next {
            now.date().and_time(next).unwrap()
        } else {
            now.date().succ().and_time(next).unwrap()
        };
        next - now
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub struct RepeatingScheduler(pub NaiveTime);
impl Scheduler for RepeatingScheduler {
    fn get_next(&self) -> Duration {
        let now = Local::now();
        let next = self.0;
        let next = if now.time() < next {
            now.date().and_time(next).unwrap()
        } else {
            now.date().succ().and_time(next).unwrap()
        };
        next - now
    }
}

/// The handler's job is to handle [`Scheduler`]s and transitions.
///
/// This is done by spawning a thread and running all code on it.
pub struct Handler;
