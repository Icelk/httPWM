use std::time::Duration;

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
    All,
}
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub struct DayTime(Duration);
impl DayTime {
    /// Converts hours and minutes to a duration (from 00:00).
    pub fn from_hour_minutes(hours: u8, minuts: u8) -> Self {
        assert!(hours < 24);
        assert!(minuts < 60);

        let seconds = hours as u64 * 3600 + minuts as u64 * 60;
        Self(Duration::new(seconds, 0))
    }
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub enum Command {
    Set(Strength),
    LinearIncrease(Transition),
    LinearDecrease(Transition),
    SineIncrease(Transition),
    SineDecrease(Transition),
    ChangeDayTimer(Day, DayTime),
}
