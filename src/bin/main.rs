use kvarn::prelude::*;
use pwm_dev::*;
use serde::Deserialize;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

fn main() {
    #[cfg(not(feature = "test"))]
    let pwm = rppal::pwm::Pwm::with_period(
        rppal::pwm::Channel::Pwm0,
        Duration::from_micros(1000),
        Duration::from_micros(0),
        rppal::pwm::Polarity::Normal,
        true,
    )
    .expect("failed to get PWM");

    #[cfg(feature = "test")]
    let pwm = PrintOut;

    // let time = chrono::Local::now().time() + chrono::Duration::seconds(10);
    let time = chrono::NaiveTime::from_hms(08, 47, 00);
    // let time = chrono::Local::now().time() + chrono::Duration::seconds(80);
    let day_transition = Transition {
        from: Strength::new(0.0),
        to: Strength::new(1.0),
        time: Duration::from_secs(30),
        // time: Duration::from_secs(60),
        interpolation: TransitionInterpolation::LinearToAndBack(0.5),
    };
    let startup_transition = Transition {
        from: Strength::new(0.0),
        to: Strength::new(1.0),
        time: Duration::from_millis(1000),
        interpolation: TransitionInterpolation::LinearToAndBack(0.5),
    };

    let scheduler = scheduler::WeekScheduler::same(time, day_transition);
    let controller = Controller::new(pwm, scheduler);

    let controller = Arc::new(Mutex::new(controller));

    controller
        .lock()
        .unwrap()
        .send(Command::SetTransition(startup_transition));

    create_server(controller).run();
}

fn create_server<T: VariableOut + Send>(controller: Arc<Mutex<Controller<T>>>) -> kvarn::Config {
    let mut bindings = FunctionBindings::new();

    let ctl = move || Arc::clone(&controller);
    let controller = ctl();
    bindings.bind_page("/clear-schedulers", move |_, _, _| {
        controller.lock().unwrap().send(Command::ClearAllSchedulers);

        (utility::ContentType::PlainText, Cached::Dynamic)
    });
    let controller = ctl();
    bindings.bind_page("/set-strength", move |buffer, req, cache| {
        let query = req.uri().query().map(|s| parse::format_query(s));
        let value = query.as_ref().and_then(|q| q.get("strength"));

        match value.and_then(|v| v.parse().ok()) {
            Some(f) => {
                controller
                    .lock()
                    .unwrap()
                    .send(Command::Set(Strength::new_clamped(f)));
            }
            None => {
                // Write err
                utility::write_error(buffer, 400, cache);
            }
        }
        (utility::ContentType::Html, Cached::Dynamic)
    });
    let controller = ctl();
    bindings.bind_page("/set-day-time", move |buffer, req, cache| {
        let command = serde_json::from_slice(req.body())
            .ok()
            .and_then(|set_day: SetDay| set_day.to_command());

        match command {
            Some(command) => {
                println!("Changed time of day to {:?}", command);
                controller.lock().unwrap().send(command);
            }
            None => {
                utility::write_error(buffer, 400, cache);
            }
        }
        (utility::ContentType::Html, Cached::Dynamic)
    });
    let controller = ctl();
    bindings.bind_page("/set-transition", move |buffer, req, cache| {
        let command = serde_json::from_slice(req.body())
            .ok()
            .and_then(|set_transition: SetTransition| set_transition.to_command());

        match command {
            Some(command) => {
                println!("Applying transition.");
                controller.lock().unwrap().send(command)
            }
            None => {
                utility::write_error(buffer, 400, cache);
            }
        }
        (utility::ContentType::Html, Cached::Dynamic)
    });

    let localhost = Host::no_certification("web", Some(bindings));
    let hosts = HostData::builder(localhost).build();
    let ports = vec![(8080, ConnectionSecurity::http1(), hosts)];

    Config::new(ports)
}

#[derive(Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SetDay {
    day: String,
    time: Option<String>,
}
impl SetDay {
    pub fn to_command(self) -> Option<Command> {
        let day: chrono::Weekday = self.day.parse().ok()?;
        let time = match self.time {
            Some(time) => Some(chrono::NaiveTime::parse_from_str(time.as_str(), "%H:%M:%S").ok()?),
            None => None,
        };
        Some(Command::ChangeDayTimer(day, time))
    }
}

#[derive(Deserialize, Debug)]
struct SetTransition {
    from: f64,
    to: f64,
    time: f64,
    interpolation: String,
    extras: Vec<String>,
}
impl SetTransition {
    pub fn to_command(self) -> Option<Command> {
        let from = Strength::new_clamped(self.from);
        let to = Strength::new_clamped(self.to);
        let time = Duration::from_secs_f64(self.time);

        let interpolation = match self.interpolation.as_str() {
            "linear" => TransitionInterpolation::Linear,
            "sine" => TransitionInterpolation::Sine,
            "linear-extra" if self.extras.len() == 1 => match self.extras[0].parse() {
                Ok(multiplier) => TransitionInterpolation::LinearToAndBack(multiplier),
                Err(_) => return None,
            },
            _ => return None,
        };
        Some(Command::SetTransition(Transition {
            from,
            to,
            time,
            interpolation,
        }))
    }
}
