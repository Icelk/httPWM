use chrono::{NaiveTime, Weekday};
use httpwm::*;
use kvarn::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

const SAVE_PATH: &'static str = "state.ron";

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
    let day_transition = Transition::default();

    let startup_multiplier = Some(0.5);
    let startup_duration = 1.0;
    let startup_transition = Transition {
        from: Strength::new(0.0),
        to: Strength::new(1.0),
        time: Duration::from_secs_f64(startup_duration),
        interpolation: TransitionInterpolation::SineToAndBack(startup_multiplier.unwrap()),
    };

    let scheduler = scheduler::WeekScheduler::same(time, day_transition);

    let (saved_state, week_scheduler) = {
        let saved_state = save_state::Data::read_from_file(SAVE_PATH, &scheduler);

        match saved_state.ok().and_then(|state| {
            state
                .ref_week_scheduler()
                .to_scheduler()
                .map(|scheduler| (scheduler, state))
        }) {
            Some((scheduler, data)) => (data, scheduler),
            None => {
                eprintln!("Failed to parse state file. Using defaults.");
                (save_state::Data::from_week_scheduler(&scheduler), scheduler)
            }
        }
    };
    let controller = Controller::new(pwm, week_scheduler);

    controller.send(Command::SetTransition(startup_transition));

    let shared = controller.get_state();

    let controller = Arc::new(Mutex::new(controller));
    let saved_state = Arc::new(Mutex::new(save_state::DataWrapper::new(saved_state)));
    {
        let shared = Arc::clone(&shared);
        let saved = Arc::clone(&saved_state);
        let controller = Arc::clone(&controller);
        thread::spawn(move || {
            thread::sleep(Duration::from_secs_f64(
                startup_duration * (startup_multiplier.unwrap_or(0.0) + 1.0),
            ));
            saved
                .lock()
                .unwrap()
                .get_ref()
                .apply(&*controller.lock().unwrap());

            thread::spawn(move || loop {
                thread::sleep(Duration::from_millis(1000));
                let mut saved = saved.lock().unwrap();

                let mut changed = false;
                {
                    let lock = shared.lock().unwrap();
                    let present_schedulers = lock.ref_schedulers();

                    let schedulers = saved.no_save_mut().mut_schedulers();

                    let len = schedulers.len();
                    schedulers.retain(|scheduler| present_schedulers.contains_key(&scheduler.name));
                    if len != schedulers.len() {
                        changed = true;
                    }
                }
                {
                    let shared = shared.lock().unwrap();
                    match saved.get_ref().eq_transition(shared.get_transition()) {
                        // Do nothing; they match
                        true => {}
                        false => {
                            saved.no_save_mut().set_transition(shared.get_transition());
                            changed = true;
                        }
                    }
                }

                if saved.save() || changed {
                    println!("Saving state!");

                    let data = {
                        let config = ron::ser::PrettyConfig::default()
                            .with_enumerate_arrays(true)
                            .with_decimal_floats(true)
                            .with_extensions(ron::extensions::Extensions::IMPLICIT_SOME);
                        match ron::ser::to_string_pretty(saved.get_ref(), config) {
                            Err(err) => {
                                eprintln!("Failed to save state {}", err);
                                continue;
                            }
                            Ok(s) => s,
                        }
                    };
                    drop(saved);

                    let mut file = match fs::File::create(SAVE_PATH) {
                        Err(err) => {
                            eprintln!("Failed to create file {}", err);
                            continue;
                        }
                        Ok(f) => f,
                    };
                    if let Err(err) = file.write_all(data.as_bytes()) {
                        eprintln!("Failed to write data to file {}", err);
                    }
                }
            });
        });
    }

    create_server(controller, saved_state, shared).run();
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

fn create_server<T: VariableOut + Send>(
    controller: Arc<Mutex<Controller<T>>>,
    save_state: Arc<Mutex<save_state::DataWrapper>>,
    shared: Arc<Mutex<SharedState>>,
) -> kvarn::Config {
    let mut bindings = FunctionBindings::new();

    let state = { move || Arc::clone(&shared) };
    let ctl = move || Arc::clone(&controller);

    let saved = move || Arc::clone(&save_state);

    let controller = ctl();
    let save = saved();
    bindings.bind_page("/clear-schedulers", move |_, _, _| {
        {
            controller.lock().unwrap().send(Command::ClearAllSchedulers);
        }
        save.lock().unwrap().get_mut().mut_schedulers().clear();

        (utility::ContentType::PlainText, Cached::Dynamic)
    });

    let controller = ctl();
    let save = saved();
    bindings.bind_page("/set-strength", move |buffer, req, cache| {
        get_query_value(req, buffer, cache, "strength")
            .and_then(|value| value.parse().ok())
            .map(|f| {
                controller
                    .lock()
                    .unwrap()
                    .send(Command::Set(Strength::new_clamped(f)));
                save.lock()
                    .unwrap()
                    .get_mut()
                    .set_strength(Strength::new_clamped(f));
            });
        (utility::ContentType::Html, Cached::Dynamic)
    });
    let controller = ctl();
    let save = saved();
    bindings.bind_page("/set-day-time", move |buffer, req, cache| {
        let day_data = serde_json::from_slice(req.body()).ok();
        let command = day_data
            .as_ref()
            .and_then(|set_day: &DayData| set_day.parse());

        match command {
            Some((day, time)) => {
                println!("Changed time of {} to {:?}", day, time);

                {
                    let mut lock = save.lock().unwrap();
                    let week_scheduler = lock.get_mut().mut_week_scheduler();
                    *week_scheduler.get_mut(day) =
                        time.map(|time| time.format("%H:%M:%S").to_string());
                }
                {
                    let lock = controller.lock().unwrap();
                    lock.send(Command::ChangeDayTimer(day, time));
                }
            }
            None => {
                utility::write_error(buffer, 400, cache);
            }
        }
        (utility::ContentType::Html, Cached::Dynamic)
    });

    let controller = ctl();
    let save = saved();
    bindings.bind_page("/transition", move |buffer, req, cache| {
        let queries = req.uri().query().map(|q| parse::format_query(q));
        let action = queries.as_ref().and_then(|q| q.get("action")).map(|a| *a);

        let transition = serde_json::from_slice(req.body())
            .ok()
            .and_then(|set_transition: TransitionData| set_transition.to_transition());
        let transition = match transition {
            Some(transition) => transition,
            None => {
                utility::write_error(buffer, 400, cache);
                return (utility::ContentType::Html, Cached::Dynamic);
            }
        };

        match action {
            Some("set") => {
                save.lock()
                    .unwrap()
                    .get_mut()
                    .mut_week_scheduler()
                    .transition = TransitionData::from_transition(&transition);
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
    let save = saved();
    bindings.bind_page("/add-scheduler", move |buffer, req, cache| {
        let data = serde_json::from_slice(req.body()).ok();
        let command = data.and_then(|data: AddSchedulerData| {
            let data_clone = data.clone();
            data.into_command().map(|cmd| (data_clone, cmd))
        });

        match command {
            Some((data, cmd)) => {
                {
                    controller.lock().unwrap().send(cmd);
                }
                save.lock().unwrap().get_mut().mut_schedulers().push(data);
            }
            None => {
                utility::write_error(buffer, 400, cache);
            }
        }

        (utility::ContentType::Html, Cached::Dynamic)
    });

    let local_state = state();
    bindings.bind_page("/get-schedulers", move |buffer, _, _| {
        let mut now = scheduler::LazyNow::new();

        let mut schedulers: Vec<(SchedulerData, Option<Duration>)> = local_state
            .lock()
            .unwrap()
            .ref_schedulers()
            .iter()
            .map(|(name, scheduler)| {
                (
                    SchedulerData::from_scheduler(scheduler.as_ref(), name.to_string(), &mut now),
                    match scheduler.get_next(&mut now) {
                        Next::At(dur, _) => Some(
                            (dur - get_naive_now())
                                .to_std()
                                .unwrap_or(Duration::new(0, 0)),
                        ),
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
            match percent_encoding::percent_decode_str(name).decode_utf8() {
                Ok(s) => {
                    {
                        controller
                            .lock()
                            .unwrap()
                            .send(Command::RemoveScheduler(s.to_string()));
                    }
                    // Can be removed since we check if internal schedulers disappeared.
                    // save.lock()
                    //     .unwrap()
                    //     .get_mut()
                    //     .mut_schedulers()
                    //     .retain(|scheduler| scheduler.name != s);
                }
                Err(_) => {
                    utility::write_error(buffer, 400, cache);
                }
            };
        });
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

/// Quite nasty code
pub mod save_state {
    use super::*;
    use chrono::Weekday;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct WeekSchedulerData {
        pub mon: Option<String>,
        pub tue: Option<String>,
        pub wed: Option<String>,
        pub thu: Option<String>,
        pub fri: Option<String>,
        pub sat: Option<String>,
        pub sun: Option<String>,
        pub transition: TransitionData,
    }
    impl WeekSchedulerData {
        pub fn get_mut(&mut self, day: Weekday) -> &mut Option<String> {
            match day {
                Weekday::Mon => &mut self.mon,
                Weekday::Tue => &mut self.tue,
                Weekday::Wed => &mut self.wed,
                Weekday::Thu => &mut self.thu,
                Weekday::Fri => &mut self.fri,
                Weekday::Sat => &mut self.sat,
                Weekday::Sun => &mut self.sun,
            }
        }
        pub fn from_scheduler(scheduler: &WeekScheduler) -> Self {
            macro_rules! fmt_time {
                ($e:expr) => {
                    $e.map(|time| time.format("%H:%M:%S").to_string())
                };
            };

            WeekSchedulerData {
                mon: fmt_time!(scheduler.mon),
                tue: fmt_time!(scheduler.tue),
                wed: fmt_time!(scheduler.wed),
                thu: fmt_time!(scheduler.thu),
                fri: fmt_time!(scheduler.fri),
                sat: fmt_time!(scheduler.sat),
                sun: fmt_time!(scheduler.sun),
                transition: TransitionData::from_transition(&scheduler.transition),
            }
        }
        pub fn to_scheduler(&self) -> Option<WeekScheduler> {
            macro_rules! fmt_time {
                ($e:expr) => {
                    match $e.as_ref() {
                        Some(time) => Some(
                            chrono::NaiveTime::parse_from_str(time.as_str(), "%H:%M:%S").ok()?,
                        ),
                        None => None,
                    }
                };
            };

            let mut scheduler = WeekScheduler::empty(self.transition.to_transition()?);

            scheduler.mon = fmt_time!(self.mon);
            scheduler.tue = fmt_time!(self.tue);
            scheduler.wed = fmt_time!(self.wed);
            scheduler.thu = fmt_time!(self.thu);
            scheduler.fri = fmt_time!(self.fri);
            scheduler.sat = fmt_time!(self.sat);
            scheduler.sun = fmt_time!(self.sun);
            Some(scheduler)
        }
    }
    pub struct DataWrapper(Data, bool);
    impl DataWrapper {
        pub fn new(data: Data) -> Self {
            Self(data, false)
        }
        pub fn get_ref(&self) -> &Data {
            &self.0
        }
        /// Returns mutable reference to inner [`Data`].
        /// Sets internal `save` bool true.
        pub fn get_mut(&mut self) -> &mut Data {
            self.1 = true;
            &mut self.0
        }
        /// Will not signal that the data has been changed. Use with caution.
        pub fn no_save_mut(&mut self) -> &mut Data {
            &mut self.0
        }
        pub fn save(&mut self) -> bool {
            let save = self.1;
            self.1 = false;
            save
        }
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Data {
        strength: Option<f64>,
        schedulers: Vec<AddSchedulerData>,
        week_scheduler: Option<WeekSchedulerData>,
        current_transition: Option<TransitionData>,
    }
    impl Data {
        pub fn read_from_file<P: AsRef<Path>>(
            path: P,
            week_scheduler: &WeekScheduler,
        ) -> io::Result<Self> {
            fn read(path: &Path) -> io::Result<Data> {
                let file = fs::File::open(path)?;
                ron::de::from_reader(file)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            }
            read(path.as_ref()).map(|mut data| {
                if data.week_scheduler.is_none() {
                    data.week_scheduler = Some(WeekSchedulerData::from_scheduler(week_scheduler));
                }
                data
            })
        }
        pub fn from_week_scheduler(scheduler: &WeekScheduler) -> Self {
            Self {
                strength: None,
                schedulers: Vec::new(),
                week_scheduler: Some(WeekSchedulerData::from_scheduler(scheduler)),
                current_transition: None,
            }
        }

        pub fn apply<T: VariableOut + Send>(&self, controller: &Controller<T>) {
            if let Some(s) = self.strength {
                controller.send(Command::Set(Strength::new_clamped(s)));
            }
            for scheduler in self
                .schedulers
                .iter()
                .filter_map(|s| s.clone().into_command())
            {
                controller.send(scheduler);
            }
            if let Some(transition) = self
                .current_transition
                .as_ref()
                .and_then(TransitionData::to_transition)
            {
                controller.send(Command::SetTransition(transition));
            }
        }

        pub fn ref_strength(&self) -> Option<f64> {
            self.strength
        }
        pub fn set_strength(&mut self, strength: Strength) -> Option<Strength> {
            self.strength
                .replace(strength.into_inner())
                .map(|f| Strength::new_clamped(f))
        }

        pub fn ref_schedulers(&self) -> &Vec<AddSchedulerData> {
            &self.schedulers
        }
        pub fn mut_schedulers(&mut self) -> &mut Vec<AddSchedulerData> {
            &mut self.schedulers
        }

        pub fn ref_week_scheduler(&self) -> &WeekSchedulerData {
            // ok, since it must be `Some`, it's just an option for parsing from file.
            self.week_scheduler.as_ref().unwrap()
        }
        pub fn mut_week_scheduler(&mut self) -> &mut WeekSchedulerData {
            // ok, since it must be `Some`, it's just an option for parsing from file.
            self.week_scheduler.as_mut().unwrap()
        }
        pub fn set_week_scheduler(&mut self, new: &WeekScheduler) -> Option<WeekSchedulerData> {
            self.week_scheduler
                .replace(WeekSchedulerData::from_scheduler(new))
        }
        pub fn eq_transition(&self, other: Option<&Transition>) -> bool {
            match self.current_transition.as_ref() {
                Some(transition) => match transition.to_transition() {
                    Some(transition) => match other {
                        Some(other) => &transition == other,
                        None => false,
                    },
                    None => false,
                },
                None => other.is_none(),
            }
        }
        pub fn set_transition(&mut self, new: Option<&Transition>) -> Option<TransitionData> {
            match new {
                None => self.current_transition.take(),
                Some(transition) => self
                    .current_transition
                    .replace(TransitionData::from_transition(transition)),
            }
        }
    }
}

#[derive(Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DayData {
    day: String,
    time: Option<String>,
}
impl DayData {
    pub fn parse(&self) -> Option<(Weekday, Option<NaiveTime>)> {
        let day: chrono::Weekday = self.day.parse().ok()?;
        let time = match self.time.as_ref() {
            Some(time) => Some(parse_time(&time)?),
            None => None,
        };
        Some((day, time))
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct TransitionData {
    from: f64,
    to: f64,
    time: f64,
    interpolation: String,
    extras: Vec<String>,
}
impl TransitionData {
    pub fn to_transition(&self) -> Option<Transition> {
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
pub struct StateData {
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
                    .ref_week_schedule()
                    .get(day)
                    .map(|time| time.to_string()),
            );
            day = day.succ();
        }

        Self {
            strength: Strength::clone(state.get_strength()).into_inner(),
            days,
            transition: TransitionData::from_transition(&state.ref_week_schedule().transition),
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
        fn get_next(&self, _: &mut scheduler::LazyNow) -> Next {
            Next::At(self.moment, self.common.get_command().into_inner())
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
        fn get_next(&self, now: &mut scheduler::LazyNow) -> Next {
            let now = now.now();
            if self.day == now.weekday() && now.time() < self.time {
                // Unwrap is OK, now will never be over self.time.
                Next::At(
                    chrono::Local::today().naive_utc().and_time(self.time),
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
                Next::At(
                    now.date().and_time(time) + chrono::Duration::days(offset as i64),
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
        fn get_next(&self, now: &mut scheduler::LazyNow) -> Next {
            let now = now.now();
            if now.time() < self.time {
                // Unwrap is OK, now will never be over self.time.
                Next::At(
                    chrono::Local::today().naive_utc().and_time(self.time),
                    self.common.get_command().into_inner(),
                )
            } else {
                // Unwrap is OK, it's one day ahead!
                Next::At(
                    chrono::Local::today().naive_utc().and_time(self.time)
                        + chrono::Duration::days(1),
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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AddSchedulerData {
    kind: String,
    time: String,
    name: String,
    description: String,
    extras: Vec<String>,
    transition: TransitionData,
}
impl AddSchedulerData {
    pub fn into_command(self) -> Option<Command> {
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
pub struct SchedulerData {
    name: String,
    description: String,
    kind: String,
    next_occurrence: String,
}
impl SchedulerData {
    pub fn from_scheduler(
        scheduler: &dyn Scheduler,
        name: String,
        now: &mut scheduler::LazyNow,
    ) -> Self {
        let dur = scheduler.get_next(now);

        let next_occurrence = match dur {
            Next::At(date_time, _) => {
                let dur = date_time - get_naive_now();
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
