use kvarn::prelude::*;
use pwm_dev::*;
use serde::{Deserialize, Serialize};
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

    let time = chrono::NaiveTime::from_hms(07, 00, 00);
    let day_transition = Transition {
        from: Strength::new(0.0),
        to: Strength::new(1.0),
        time: Duration::from_secs(15 * 60),
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

    controller.send(Command::SetTransition(startup_transition));

    let controller = Arc::new(Mutex::new(controller));

    create_server(controller).run();
}

fn create_server<T: VariableOut + Send>(controller: Arc<Mutex<Controller<T>>>) -> kvarn::Config {
    let mut bindings = FunctionBindings::new();

    let state = { controller.lock().unwrap().get_state() };

    let ctl = move || Arc::clone(&controller);
    let controller = ctl();
    bindings.bind_page("/clear-schedulers", move |_, _, _| {
        {
            controller.lock().unwrap().send(Command::ClearAllSchedulers);
        }

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
            .and_then(|set_day: DayData| set_day.to_command());

        match command {
            Some(command) => {
                println!("Changed time of day to {:?}", command);
                {
                    controller.lock().unwrap().send(command);
                }
            }
            None => {
                utility::write_error(buffer, 400, cache);
            }
        }
        (utility::ContentType::Html, Cached::Dynamic)
    });
    let controller = ctl();
    bindings.bind_page("/transition", move |buffer, req, cache| {
        let queries = req.uri().query().map(|q| parse::format_query(q));
        let action = queries.as_ref().and_then(|q| q.get("action")).map(|a| *a);

        let transition = serde_json::from_slice(req.body())
            .ok()
            .and_then(|set_transition: TransitionData| set_transition.to_transition());
        let transition = match transition {
            Some(command) => command,
            None => {
                utility::write_error(buffer, 400, cache);
                return (utility::ContentType::Html, Cached::Dynamic);
            }
        };

        match action {
            Some("set") => {
                println!("Setting default transition.");
                {
                    controller
                        .lock()
                        .unwrap()
                        .send(Command::ChangeDayTimerTransition(transition));
                }
            }
            Some("preview") => {
                println!("Applying transition.");
                {
                    controller
                        .lock()
                        .unwrap()
                        .send(Command::SetTransition(transition));
                }
            }
            _ => {
                utility::write_error(buffer, 400, cache);
            }
        }

        (utility::ContentType::Html, Cached::Dynamic)
    });
    bindings.bind_page("/get-state", move |buffer, _, _| {
        let state = StateData::from_shared_state(&*state.lock().unwrap());
        serde_json::to_writer(buffer, &state).expect("failed to parse shared state");
        (utility::ContentType::JSON, Cached::Dynamic)
    });

    let localhost = Host::no_certification("web", Some(bindings));
    let hosts = HostData::builder(localhost).build();
    let ports = vec![(8080, ConnectionSecurity::http1(), hosts)];

    Config::new(ports)
}

#[derive(Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DayData {
    day: String,
    time: Option<String>,
}
impl DayData {
    pub fn to_command(self) -> Option<Command> {
        let day: chrono::Weekday = self.day.parse().ok()?;
        let time = match self.time {
            Some(time) => Some(
                chrono::NaiveTime::parse_from_str(time.as_str(), "%H:%M:%S")
                    .or_else(|_| chrono::NaiveTime::parse_from_str(time.as_str(), "%H:%M"))
                    .ok()?,
            ),
            None => None,
        };
        Some(Command::ChangeDayTimer(day, time))
    }
}

#[derive(Deserialize, Serialize, Debug)]
struct TransitionData {
    from: f64,
    to: f64,
    time: f64,
    interpolation: String,
    extras: Vec<String>,
}
impl TransitionData {
    pub fn to_transition(self) -> Option<Transition> {
        let from = Strength::new_clamped(self.from);
        let to = Strength::new_clamped(self.to);
        let time = Duration::from_secs_f64(self.time);

        let interpolation = TransitionInterpolation::from_str(&self.interpolation, &self.extras)?;
        Some(Transition {
            from,
            to,
            time,
            interpolation,
        })
    }

    pub fn from_transition(transition: &Transition) -> Self {
        let mut extras = Vec::with_capacity(4);

        transition.interpolation.apply_extras(&mut extras);

        Self {
            from: Strength::clone(&transition.from).into_inner(),
            to: Strength::clone(&transition.to).into_inner(),
            time: transition.time.as_secs_f64(),
            interpolation: transition.interpolation.as_str().to_string(),
            extras,
        }
    }
}

#[derive(Debug, Serialize)]
struct StateData {
    strength: f64,
    days: HashMap<String, Option<String>>,
    transition: TransitionData,
}
impl StateData {
    pub fn from_shared_state(state: &SharedState) -> Self {
        let mut days = HashMap::with_capacity(7);
        let mut day = chrono::Weekday::Mon;
        for _ in 0..7 {
            days.insert(
                weekday_to_lowercase_str(&day).to_string(),
                state
                    .get_week_schedule()
                    .get(day)
                    .map(|time| time.to_string()),
            );
            day = day.succ();
        }

        Self {
            strength: Strength::clone(state.get_strength()).into_inner(),
            days,
            transition: TransitionData::from_transition(&state.get_week_schedule().transition),
        }
    }
}
