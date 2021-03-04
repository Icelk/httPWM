use pwm_dev::*;
use std::{thread::sleep, time::Duration};

fn main() {
    let in_three_minutes = chrono::Local::now().time() + chrono::Duration::minutes(3);
    let transition = Transition {
        from: Strength::new(0.0),
        to: Strength::new(1.0),
        time: Duration::from_secs(2),
        interpolation: TransitionInterpolation::Sine,
    };
    let scheduler = scheduler::WeekScheduler::same(in_three_minutes, transition.clone());
    let mut controller = Controller::new(PrintOut, scheduler);

    controller.send(Command::Set(Strength::new(0.75)));
    sleep(Duration::from_secs(2));

    println!("Sending transition!");
    controller.send(Command::SetTransition(transition));
    sleep(Duration::from_secs(1));

    println!("Sending set command");
    controller.send(Command::Set(Strength::new(0.1)));

    controller.finish();
}
