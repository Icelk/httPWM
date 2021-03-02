mod lib;
use chrono::prelude::*;
use kvarn;
use lib::scheduler::Scheduler;

fn main() {
    let scheduler = lib::scheduler::RepeatingScheduler(NaiveTime::from_hms(7, 0, 0));
    let next = scheduler.get_next();
    println!(
        "Time: {}:{}",
        next.num_hours(),
        next.num_minutes() - next.num_hours() * 60
    );
}
