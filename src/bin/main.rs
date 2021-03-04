use pwm_dev::*;
use std::{thread::sleep, time::Duration};

fn main() {
    #[cfg(not(feature = "test"))]
    let mut pwm = rppal::pwm::Pwm::with_period(
        rppal::pwm::Channel::Pwm0,
        Duration::from_micros(1000),
        Duration::from_micros(0),
        rppal::pwm::Polarity::Normal,
        true,
    )
    .expect("failed to get PWM");
    #[cfg(not(feature = "test"))]
    pwm.enable().unwrap();

    // let time = chrono::Local::now().time() + chrono::Duration::seconds(20);
    let time = chrono::NaiveTime::from_hms(07, 02, 00);
    let transition = Transition {
        from: Strength::new(0.0),
        to: Strength::new(1.0),
        // time: Duration::from_secs(2),
        time: Duration::from_secs(60),
        interpolation: TransitionInterpolation::Linear,
    };
    let scheduler = scheduler::WeekScheduler::same(time, transition.clone());
    #[cfg(feature = "test")]
    let mut controller = Controller::new(PrintOut, scheduler);
    #[cfg(not(feature = "test"))]
    let mut controller = Controller::new(pwm, scheduler);

    controller.send(Command::Set(Strength::new(0.75)));
    sleep(Duration::from_secs(2));

    println!("Sending transition!");
    controller.send(Command::SetTransition(transition));
    sleep(Duration::from_secs(1));

    println!("Sending set command");
    controller.send(Command::Set(Strength::new(0.0)));

    // sleep(Duration::from_secs(25));
    // controller.finish();

    std::thread::park();
}
