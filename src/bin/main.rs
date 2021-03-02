use chrono::prelude::*;
use pwm_dev::*;

fn main() {
    let scheduler = scheduler::RepeatingScheduler(NaiveTime::from_hms(7, 0, 0));
    let next = scheduler.get_next();
    println!(
        "Time: {}:{}",
        next.num_hours(),
        next.num_minutes() - next.num_hours() * 60
    );
}
