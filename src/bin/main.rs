use httpwm::*;
use kvarn::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

fn main() {
    #[cfg(not(feature = "test"))]
    let pwm = rppal::pwm::Pwm::with_period(
        rppal::pwm::Channel::Pwm0,
        Duration::from_millis(1),
        Duration::from_millis(0),
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
        interpolation: TransitionInterpolation::SineToAndBack(0.5),
    };
    let startup_transition = Transition {
        from: Strength::new(0.0),
        to: Strength::new(1.0),
        time: Duration::from_millis(1000),
        interpolation: TransitionInterpolation::SineToAndBack(0.5),
    };

    let scheduler = scheduler::WeekScheduler::same(time, day_transition);
    let controller = Controller::new(pwm, scheduler);

    controller.send(Command::SetTransition(startup_transition));

    let controller = Arc::new(Mutex::new(controller));

    create_server(controller).run();
}

fn get_query_value<'a>(
    req: &'a http::Request<&[u8]>,
    buffer: &mut Vec<u8>,
    cache: &mut kvarn::cache::types::FsCache,
    query: &str,
) -> Option<&'a str> {
    let queries = req.uri().query().map(|s| parse::format_query(s));
    let value = queries.as_ref().and_then(|q| q.get(query));

    match value {
        Some(value) => Some(*value),
        None => {
            // Write err
            utility::write_error(buffer, 400, cache);
            None
        }
    }
}

fn create_server<T: VariableOut + Send>(controller: Arc<Mutex<Controller<T>>>) -> kvarn::Config {
    let mut bindings = FunctionBindings::new();

    let state = {
        let state = controller.lock().unwrap().get_state();
        move || Arc::clone(&state)
    };
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
        get_query_value(req, buffer, cache, "strength")
            .and_then(|value| value.parse().ok())
            .map(|f| {
                controller
                    .lock()
                    .unwrap()
                    .send(Command::Set(Strength::new_clamped(f)))
            });
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

    let local_state = state();
    bindings.bind_page("/get-state", move |buffer, _, _| {
        let state = StateData::from_shared_state(&*local_state.lock().unwrap());
        serde_json::to_writer(buffer, &state).expect("failed to parse shared state");
        (utility::ContentType::JSON, Cached::Dynamic)
    });

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

    let local_state = state();
    bindings.bind_page("/get-schedulers", move |buffer, _, _| {
        let mut schedulers: Vec<(SchedulerData, Option<Duration>)> = local_state
            .lock()
            .unwrap()
            .get_schedulers()
            .iter()
            .map(|(name, scheduler)| {
                (
                    SchedulerData::from_scheduler(scheduler.as_ref(), name.to_string()),
                    match scheduler.get_next(false) {
                        Next::Immediately(_) => Some(Duration::new(0, 0)),
                        Next::In(dur, _) => Some(dur),
                        Next::Unknown => None,
                    },
                )
            })
            .collect();

        schedulers.sort_by(|(_, d1), (_, d2)| d1.cmp(d2));

        let schedulers: Vec<SchedulerData> = schedulers.into_iter().map(|(data, _)| data).collect();

        serde_json::to_writer(buffer, &schedulers).expect("failed to write to Vec?");
        (utility::ContentType::PlainText, Cached::Dynamic)
    });

    let controller = ctl();
    bindings.bind_page("/remove-scheduler", move |buffer, req, cache| {
        get_query_value(req, buffer, cache, "name").map(|name| {
            controller
                .lock()
                .unwrap()
                .send(Command::RemoveScheduler(name.to_string()))
        });
        (utility::ContentType::PlainText, Cached::Dynamic)
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
    use httpwm::scheduler::Keep;

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

    #[derive(Debug)]
    pub struct Common {
        description: String,
        command: ClonableCommand,
    }
    impl Common {
        /// Returns `Err` when command is not clonable
        pub fn new(description: String, command: ClonableCommand) -> Self {
            Self {
                description,
                command,
            }
        }
        pub fn get_command(&self) -> ClonableCommand {
            // Ok, since it's guaranteed the command in `Common` is clonable.
            ClonableCommand::clone(&self.command)
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
        fn get_next(&self, _: bool) -> Next {
            let now = get_naive_now();
            Next::In(
                (self.moment - now).to_std().unwrap_or(Duration::new(0, 0)),
                self.common.get_command().into_inner(),
            )
        }
        fn advance(&mut self) -> Keep {
            Keep::Remove
        }
        fn description(&self) -> &str {
            self.common.description.as_str()
        }
        fn kind(&self) -> &str {
            "At"
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
        fn get_next(&self, _: bool) -> Next {
            let now = get_naive_now();
            if self.day == now.weekday() && now.time() < self.time {
                // Unwrap is OK, now will never be over self.time.
                Next::In(
                    (self.time - now.time()).to_std().unwrap(),
                    self.common.get_command().into_inner(),
                )
            } else {
                // Unwrap is ok, we must have one day containing a date.
                let (time, offset) = get_next_day(now.weekday(), |day| {
                    if day == self.day {
                        Some(self.time)
                    } else {
                        None
                    }
                })
                .unwrap();
                // unwrap is OK, since date is always `.succ()`
                let dur = ((now.date().and_time(time) + chrono::Duration::days(offset as i64))
                    - now)
                    .to_std()
                    .unwrap();
                Next::In(dur, self.common.get_command().into_inner())
            }
        }
        fn advance(&mut self) -> Keep {
            Keep::Keep
        }
        fn description(&self) -> &str {
            self.common.description.as_str()
        }
        fn kind(&self) -> &str {
            "Every week at"
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
        fn get_next(&self, _: bool) -> Next {
            let now = get_naive_now();
            if now.time() < self.time {
                // Unwrap is OK, now will never be over self.time.
                Next::In(
                    (self.time - now.time()).to_std().unwrap(),
                    self.common.get_command().into_inner(),
                )
            } else {
                // Unwrap is OK, it's one day ahead!
                Next::In(
                    ((self.time - now.time()) + chrono::Duration::days(1))
                        .to_std()
                        .unwrap(),
                    self.common.get_command().into_inner(),
                )
            }
        }
        fn advance(&mut self) -> Keep {
            Keep::Keep
        }
        fn description(&self) -> &str {
            self.common.description.as_str()
        }
        fn kind(&self) -> &str {
            "Every day at"
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
        // Unwrap is ok, since we know `SetTransition` is clonable
        let run_command = ClonableCommand::new(Command::SetTransition(transition)).unwrap();
        let common = extra_schedulers::Common::new(self.description, run_command);

        let scheduler: Box<dyn Scheduler> =
            match self.kind.as_str() {
                "at" if self.extras.len() == 1 => Box::new(extra_schedulers::At::new(
                    common,
                    chrono::NaiveDate::parse_from_str(self.extras[0].as_str(), "%Y-%m-%d")
                        .ok()?
                        .and_time(time),
                )),
                "every-week" if self.extras.len() == 1 => Box::new(
                    extra_schedulers::EveryWeek::new(common, time, self.extras[0].parse().ok()?),
                ),
                "every-day" => Box::new(extra_schedulers::EveryDay::new(common, time)),
                _ => return None,
            };
        Some(Command::AddReplaceScheduler(self.name, scheduler))
    }
}
#[derive(Debug, Serialize)]
struct SchedulerData {
    name: String,
    description: String,
    kind: String,
    next_occurrence: String,
}
impl SchedulerData {
    pub fn from_scheduler(scheduler: &dyn Scheduler, name: String) -> Self {
        let dur = scheduler.get_next(false);

        let next_occurrence = match dur {
            Next::In(dur, _) => {
                let dur = chrono::Duration::from_std(dur).expect("std duration overflowed!");
                if dur.num_days() > 0 {
                    (get_naive_now() + dur)
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string()
                } else if dur.num_hours() > 0 {
                    format!("In {} hours", dur.num_hours())
                } else if dur.num_minutes() > 0 {
                    format!("In {} minutes", dur.num_minutes())
                } else {
                    format!("In {} seconds", dur.num_seconds())
                }
            }
            Next::Immediately(_) => "immediately".to_string(),
            Next::Unknown => "unknown".to_string(),
        };

        // let next_occurrence = (chrono::Local::now()
        //     + chrono::Duration::from_std(scheduler.get_next().0)
        //         .expect("std duration overflowed!"))
        // .to_rfc3339();

        Self {
            name,
            description: scheduler.description().to_string(),
            kind: scheduler.kind().to_string(),
            next_occurrence,
        }
    }
}
