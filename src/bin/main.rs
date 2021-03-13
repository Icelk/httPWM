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

    // todo!("Use chrono::Native");
    let controller = ctl();
    bindings.bind_page("/add-scheduler", move |buffer, req, cache| {
        let command = serde_json::from_slice(req.body())
            .ok()
            .and_then(|data: AddSchedulerData| data.to_command());

        match command {
            Some(command) => controller.lock().unwrap().send(command),
            None => {
                utility::write_error(buffer, 400, cache);
            }
        }

        (utility::ContentType::Html, Cached::Dynamic)
    });

    let localhost = Host::no_certification("web", Some(bindings));
    let hosts = HostData::builder(localhost).build();
    let ports = vec![(8080, ConnectionSecurity::http1(), hosts)];

    let mut config = Config::new(ports);
    config.mount_extension(kvarn_extensions::cache);
    config
}

pub fn parse_time(string: &str) -> Option<chrono::NaiveTime> {
    chrono::NaiveTime::parse_from_str(string, "%H:%M:%S")
        .or_else(|_| chrono::NaiveTime::parse_from_str(string, "%H:%M"))
        .ok()
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
            Some(time) => Some(parse_time(&time)?),
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
pub mod extra_schedulers {
    use chrono::Datelike;
    use pwm_dev::scheduler::Keep;

    use super::*;

    pub(crate) fn get_next_day<F: Fn(chrono::Weekday) -> Option<chrono::NaiveTime>>(
        from: chrono::Weekday,
        get: F,
    ) -> Option<(chrono::NaiveTime, u8)> {
        let mut day = from;

        for passed in 0..7 {
            day = day.succ();
            let time = get(day);
            if time.is_some() {
                return time.map(|t| (t, passed + 1));
            }
        }
        None
    }
    pub(crate) fn apply_day_offset(
        date_time: chrono::DateTime<chrono::Local>,
        offset: i64,
    ) -> chrono::DateTime<chrono::Local> {
        date_time + chrono::Duration::days(offset)
    }

    #[derive(Debug)]
    pub struct Common {
        description: String,
        command: Command,
    }
    impl Common {
        /// Returns `Err` when command is not clonable
        pub fn new(description: String, command: Command) -> Result<Self, ()> {
            if !command.can_clone() {
                Err(())
            } else {
                Ok(Self {
                    description,
                    command,
                })
            }
        }
        pub fn get_command(&self) -> Command {
            // Ok, since it's guaranteed the command in `Common` is clonable.
            self.command.try_clone().unwrap()
        }
    }

    #[derive(Debug)]
    pub struct At {
        common: Common,
        moment: chrono::NaiveDateTime,
    }
    impl At {
        pub fn new(common: Common, moment: chrono::NaiveDateTime) -> Self {
            Self { common, moment }
        }
    }
    impl Scheduler for At {
        fn get_next(&self) -> Option<(Duration, Command)> {
            let now = chrono::Local::now();
            let fixed_now = now.date().naive_local().and_time(now.time());
            match (self.moment - fixed_now).to_std() {
                Ok(dur) => Some((dur, Command::clone(&self.common.command))),
                Err(_) => None,
            }
        }
        fn advance(&mut self) -> Keep {
            Keep::Remove
        }
        fn description(&self) -> &str {
            self.common.description.as_str()
        }
        fn kind(&self) -> Option<&str> {
            Some("AtDateTime")
        }
    }
    #[derive(Debug)]
    pub struct EveryWeek {
        common: Common,
        time: chrono::NaiveTime,
        day: chrono::Weekday,
    }
    impl EveryWeek {
        pub fn new(common: Common, time: chrono::NaiveTime, day: chrono::Weekday) -> Self {
            Self { common, time, day }
        }
    }
    impl Scheduler for EveryWeek {
        fn get_next(&self) -> Option<(Duration, Command)> {
            let now = chrono::Local::now();
            if self.day == now.weekday() && now.time() < self.time {
                // Unwrap is OK, now will never be over self.time.
                Some((
                    (self.time - now.time()).to_std().unwrap(),
                    self.common.get_command(),
                ))
            } else {
                get_next_day(now.weekday(), |day| {
                    if day == self.day {
                        Some(self.time)
                    } else {
                        None
                    }
                })
                .map(|(time, offset)| {
                    // unwrap is OK, since date is always `.succ()`
                    (apply_day_offset(
                        now.date()
                            .and_time(time)
                            .expect("invalid DateTime in EveryWeek scheduler"),
                        offset as i64,
                    ) - now)
                        .to_std()
                        .unwrap()
                })
                .map(|dur| (dur, self.common.get_command()))
            }
        }
        fn advance(&mut self) -> Keep {
            Keep::Keep
        }
        fn description(&self) -> &str {
            self.common.description.as_str()
        }
        fn kind(&self) -> Option<&str> {
            Some("EveryWeekAt")
        }
    }
    #[derive(Debug)]
    pub struct EveryDay {
        common: Common,
        time: chrono::NaiveTime,
    }
    impl EveryDay {
        pub fn new(common: Common, time: chrono::NaiveTime) -> Self {
            Self { common, time }
        }
    }
    impl Scheduler for EveryDay {
        fn get_next(&self) -> Option<(Duration, Command)> {
            let now = chrono::Local::now();
            Some(if now.time() < self.time {
                // Unwrap is OK, now will never be over self.time.
                (
                    (self.time - now.time()).to_std().unwrap(),
                    self.common.get_command(),
                )
            } else {
                // Unwrap is OK, it's one day ahead!
                (
                    ((self.time - now.time()) + chrono::Duration::days(1))
                        .to_std()
                        .unwrap(),
                    self.common.get_command(),
                )
            })
        }
        fn advance(&mut self) -> Keep {
            Keep::Keep
        }
        fn description(&self) -> &str {
            self.common.description.as_str()
        }
        fn kind(&self) -> Option<&str> {
            Some("EveryDayAt")
        }
    }
}

#[derive(Debug, Deserialize)]
struct AddSchedulerData {
    kind: String,
    time: String,
    name: String,
    description: String,
    extras: Vec<String>,
    transition: TransitionData,
}
impl AddSchedulerData {
    pub fn to_command(self) -> Option<Command> {
        let transition = self.transition.to_transition()?;
        let time = parse_time(&self.time)?;
        let run_command = Command::SetTransition(transition);
        let common = extra_schedulers::Common::new(self.description, run_command).unwrap();

        let scheduler: Arc<Mutex<dyn Scheduler>> = match self.kind.as_str() {
            "at" if self.extras.len() == 1 => Arc::new(Mutex::new(extra_schedulers::At::new(
                common,
                chrono::NaiveDate::parse_from_str(self.extras[0].as_str(), "%Y-%m-%d")
                    .ok()?
                    .and_time(time),
            ))),
            "every-week" if self.extras.len() == 1 => Arc::new(Mutex::new(
                extra_schedulers::EveryWeek::new(common, time, self.extras[0].parse().ok()?),
            )),
            "every-day" => Arc::new(Mutex::new(extra_schedulers::EveryDay::new(common, time))),
            _ => return None,
        };
        Some(Command::AddReplaceScheduler(self.name, scheduler))
    }
}
