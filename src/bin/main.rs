#[cfg(feature = "esp32")]
use embedded_svc::{storage::*, wifi::*};
#[cfg(feature = "esp32")]
use esp_idf_svc::{netif::*, nvs::*, sysloop::*, wifi::*};
use httpwm::*;
#[cfg(feature = "web")]
use kvarn::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

#[cfg(feature = "esp32")]
type NvsStorage = Arc<Mutex<esp_idf_svc::nvs_storage::EspNvsStorage>>;

const SAVE_PATH: &str = "state.ron";
static SECOND_FORMAT: &[time::format_description::FormatItem] =
    time::macros::format_description!("[hour]:[minute]:[second]");
static MINUTE_FORMAT: &[time::format_description::FormatItem] =
    time::macros::format_description!("[hour]:[minute]");
static DATE_FORMAT: &[time::format_description::FormatItem] =
    time::macros::format_description!("[year]-[month]-[day]");
static DATE_TIME_FORMAT: &[time::format_description::FormatItem] =
    time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

#[cfg(feature = "esp32")]
static INDEX_HTML: &str = include_str!("../../web/public/index.html");
#[cfg(feature = "esp32")]
static SCRIPT_JS: &str = include_str!("../../web/public/script.js");
#[cfg(feature = "esp32")]
static STYLE_CSS: &str = include_str!("../../web/public/style.css");
#[cfg(feature = "esp32")]
static WIFI_NAME: &'static str = env!("WIFI_NAME");
#[cfg(feature = "esp32")]
static WIFI_PASSWORD: &'static str = env!("WIFI_PASSWORD");

fn main() {
    #[cfg(feature = "esp32")]
    esp_idf_sys::link_patches();
    // Bind the log crate to the ESP Logging facilities
    #[cfg(feature = "esp32")]
    esp_idf_svc::log::EspLogger::initialize_default();

    #[cfg(any(feature = "rpi", feature = "test"))]
    env_logger::init();

    #[cfg(feature = "rpi")]
    let pwm = rppal::pwm::Pwm::with_period(
        rppal::pwm::Channel::Pwm0,
        Duration::from_millis(1),
        Duration::from_millis(0),
        rppal::pwm::Polarity::Normal,
        true,
    )
    .expect("failed to get PWM");

    #[cfg(feature = "esp32")]
    let mut pwm = {
        use esp_idf_hal::{
            ledc::config::TimerConfig, ledc::Channel, ledc::Resolution, ledc::Timer,
            peripherals::Peripherals, units::FromValueType,
        };
        let peripherals = Peripherals::take().unwrap();
        let config = TimerConfig::default()
            .frequency(25.kHz().into())
            .resolution(Resolution::Bits10);
        let timer = Timer::new(peripherals.ledc.timer0, &config).expect("esp32 timer failed");
        Channel::new(peripherals.ledc.channel0, timer, peripherals.pins.gpio18)
            .expect("failed to create a esp32 PWM channel")
    };

    #[cfg(feature = "esp32")]
    let netif_stack =
        Arc::new(EspNetifStack::new().expect("Failed to create network stack on esp32"));
    #[cfg(feature = "esp32")]
    let sys_loop_stack =
        Arc::new(EspSysLoopStack::new().expect("Failed to create sys loop stack on esp32"));
    #[cfg(feature = "esp32")]
    let default_nvs = Arc::new(EspDefaultNvs::new().expect("Failed to create nvs on esp32"));
    #[cfg(feature = "esp32")]
    let storage = Arc::new(Mutex::new(
        esp_idf_svc::nvs_storage::EspNvsStorage::new_default(default_nvs.clone(), "icelk", true)
            .expect("Failed to initialize persistent storage"),
    ));

    #[cfg(feature = "esp32")]
    let get_data = |path: &str| {
        let storage = storage.lock().unwrap();
        let mut buf = vec![0; storage.len(path).ok()?? as usize];
        storage.get_raw(path, &mut buf).ok()?;
        Some(buf)
    };
    #[cfg(feature = "esp32")]
    let known_networks = {
        let mut networks = if let Some(file) = get_data("networks.ron") {
            if let Ok(networks) = ron::de::from_bytes(&file) {
                networks
            } else {
                error!(
                    "Failed to parse networks file: {}",
                    String::from_utf8_lossy(&file)
                );
                HashMap::new()
            }
        } else {
            HashMap::new()
        };
        networks.insert(WIFI_NAME.into(), WIFI_PASSWORD.into());
        networks
    };
    // connect to wifi
    #[cfg(feature = "esp32")]
    let _wifi = {
        let mut blink_light = || {
            pwm.enable().unwrap();
            let initial = Instant::now();
            // function for intensity
            let f = |t: f64| (t * 10.).sin().abs() / (4. * t + 1.);
            let end = std::f64::consts::PI / 5.;
            loop {
                let t = initial.elapsed().as_secs_f64();
                if t >= end {
                    break;
                }
                let strength = f(t);
                pwm.set(Strength::new_clamped(strength));
                thread::sleep(Duration::from_millis(10));
            }
            pwm.set(Strength::new(0.));
            pwm.disable().unwrap();
        };
        loop {
            match wifi(
                netif_stack.clone(),
                sys_loop_stack.clone(),
                default_nvs.clone(),
                &known_networks,
            ) {
                Ok(w) => break w,
                Err(_) => {
                    // blink light to signal no network was found
                    blink_light();
                    thread::sleep(Duration::from_millis(150));
                    blink_light();

                    thread::sleep(Duration::from_secs(2));
                }
            };
        }
    };
    // sync time
    #[cfg(feature = "esp32")]
    let sntp = esp_idf_svc::sntp::EspSntp::new_default()
        .expect("Failed to set up time synchronization on esp32");
    #[cfg(feature = "esp32")]
    loop {
        use embedded_svc::sys_time::SystemTime;
        if sntp.get_sync_status() != esp_idf_svc::sntp::SyncStatus::Completed {
            thread::sleep(Duration::from_millis(500));
            continue;
        }
        info!(
            "Time synced: {:?}",
            std::time::SystemTime::UNIX_EPOCH + esp_idf_svc::systime::EspSystemTime.now()
        );
        break;
    }
    // try load set timezone
    #[cfg(feature = "esp32")]
    {
        if let Some(data) = get_data("timezone.txt") {
            if let Ok(s) = std::str::from_utf8(&data) {
                if httpwm::env_timezone::try_set_timezone(s).is_err() {
                    error!("Failed to parse saved timezone: {s:?}");
                    let mut storage = storage.lock().unwrap();
                    let _ = storage.remove("timezone.txt");
                }
            }
        }
    }

    #[cfg(feature = "test")]
    let pwm = PrintOut(test_output::spawn());

    let time = time::Time::from_hms(7, 00, 00).unwrap();
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
        #[cfg(not(feature = "esp32"))]
        let saved_state = save_state::Data::read_from_file(SAVE_PATH, &scheduler);

        #[cfg(feature = "esp32")]
        let saved_state = {
            let bytes = get_data(SAVE_PATH);
            bytes
                .map(|b| ron::de::from_bytes::<'_, save_state::Data>(&b).ok())
                .flatten()
                .map(|mut data| {
                    if data.week_scheduler.is_none() {
                        data.week_scheduler =
                            Some(save_state::WeekSchedulerData::from_scheduler(&scheduler));
                    }
                    data
                })
                .ok_or(())
        };

        match saved_state.ok().and_then(|state| {
            state
                .ref_week_scheduler()
                .to_scheduler()
                .map(|scheduler| (scheduler, state))
        }) {
            Some((scheduler, data)) => (data, scheduler),
            None => {
                error!("Failed to parse state file. Using defaults.");
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
        #[cfg(feature = "esp32")]
        let storage = Arc::clone(&storage);
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
                    info!("Saving state!");

                    let data = {
                        let config = ron::ser::PrettyConfig::default()
                            .extensions(ron::extensions::Extensions::IMPLICIT_SOME);
                        match ron::ser::to_string_pretty(saved.get_ref(), config) {
                            Err(err) => {
                                error!("Failed to serialize state {}", err);
                                continue;
                            }
                            Ok(s) => s,
                        }
                    };
                    drop(saved);

                    #[cfg(not(feature = "esp32"))]
                    {
                        let mut file = match std::fs::File::create(SAVE_PATH) {
                            Err(err) => {
                                error!("Failed to create file {}", err);
                                continue;
                            }
                            Ok(f) => f,
                        };
                        if let Err(err) = file.write_all(data.as_bytes()) {
                            error!("Failed to write data to file {}", err);
                        }
                    }
                    #[cfg(feature = "esp32")]
                    {
                        let mut storage = storage.lock().unwrap();
                        if storage.put_raw(SAVE_PATH, data.as_bytes()).is_err() {
                            error!("Failed to write to NVS.");
                        }
                    }
                }
            });
        });
    }

    #[cfg(feature = "web")]
    run(
        controller,
        saved_state,
        shared,
        #[cfg(feature = "esp32")]
        known_networks,
        #[cfg(feature = "esp32")]
        storage,
    );
}

#[cfg(feature = "web")]
fn get_query_value<'a, T>(req: &'a Request<T>, key: &'a str) -> Option<String> {
    let query = req.uri().query().map(parse::query);
    let pair = query.as_ref().and_then(|q| q.get(key));
    pair.map(|pair| pair.value().to_owned())
}

// #[cfg(all(feature = "web", not(feature = "esp32")))]
#[cfg(feature = "web")]
#[tokio::main(flavor = "current_thread")]
async fn run<T: VariableOut + Send>(
    controller: Arc<Mutex<Controller<T>>>,
    save_state: Arc<Mutex<save_state::DataWrapper>>,
    shared: Arc<Mutex<SharedState>>,
    #[cfg(feature = "esp32")] known_networks: HashMap<String, String>,
    #[cfg(feature = "esp32")] storage: NvsStorage,
) {
    create_server(
        controller,
        save_state,
        shared,
        #[cfg(feature = "esp32")]
        known_networks,
        #[cfg(feature = "esp32")]
        storage,
    )
    .execute()
    .await
    .wait()
    .await;
}
// #[cfg(all(feature = "web", feature = "esp32"))]
// #[tokio::main(flavor = "current_thread")]
// async fn run<T: VariableOut + Send>(
// controller: Arc<Mutex<Controller<T>>>,
// save_state: Arc<Mutex<save_state::DataWrapper>>,
// shared: Arc<Mutex<SharedState>>,
// ) {
// let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
// let registry = create_server(tx);
// let _server = registry
// .start(&esp_idf_svc::httpd::Configuration {
// http_port: 80,
// https_port: 0,
// max_uri_handlers: 15,
// })
// .unwrap();

// let host = create_host(controller, save_state, shared);

// while let Some((mut req, channel)) = rx.recv().await {
// let response = kvarn::handle_cache(
// &mut req,
// SocketAddr::V4(net::SocketAddrV4::new(net::Ipv4Addr::UNSPECIFIED, 0)),
// &host,
// )
// .await;
// channel.send(response.response).unwrap();
// }
// }

#[cfg(feature = "web")]
fn create_host<T: VariableOut + Send>(
    controller: Arc<Mutex<Controller<T>>>,
    save_state: Arc<Mutex<save_state::DataWrapper>>,
    shared: Arc<Mutex<SharedState>>,
    #[cfg(feature = "esp32")] known_networks: HashMap<String, String>,
    #[cfg(feature = "esp32")] storage: NvsStorage,
) -> kvarn::host::Host {
    let mut extensions = Extensions::new();

    extensions.with_csp(
        Csp::default()
            .add(
                "/*",
                CspRule::default()
                    .script_src(CspValueSet::default().unsafe_inline())
                    // allow connect (fetch) to anywhere
                    .connect_src(CspValueSet::default().scheme("http:")),
            )
            .arc(),
    );
    extensions.with_cors(
        Cors::empty()
            .add("/get-state", CorsAllowList::default().allow_all_origins())
            .add(
                "/set-strength",
                CorsAllowList::default().allow_all_origins(),
            )
            .add(
                "/set-effect",
                CorsAllowList::default()
                    .allow_all_origins()
                    .allow_all_methods(),
            )
            .arc(),
    );

    let state = { move || Arc::clone(&shared) };
    let ctl = move || controller.lock().unwrap().to_sender();

    let saved = move || Arc::clone(&save_state);

    fn r200() -> FatResponse {
        FatResponse::no_cache(Response::new(Bytes::new()))
    }
    async fn read_body(request: &mut FatRequest) -> io::Result<Bytes> {
        request.body_mut().read_to_bytes().await
    }

    let controller = ctl();
    let save = saved();
    extensions.add_prepare_single(
        "/clear-schedulers",
        prepare!(
            _req,
            _host,
            _path,
            _addr,
            move |save: Arc<Mutex<save_state::DataWrapper>>, controller: ControllerSender| {
                {
                    controller.send(Command::ClearAllSchedulers);
                }
                save.lock().unwrap().get_mut().mut_schedulers().clear();
                r200()
            }
        ),
    );

    let controller = ctl();
    let save = saved();
    extensions.add_prepare_single(
        "/set-strength",
        prepare!(
            request,
            host,
            _path,
            _addr,
            move |save: Arc<Mutex<save_state::DataWrapper>>, controller: ControllerSender| {
                match get_query_value(request, "strength").and_then(|value| value.parse().ok()) {
                    Some(f) => {
                        controller.send(Command::Set(Strength::new_clamped(f)));
                        save.lock()
                            .unwrap()
                            .get_mut()
                            .set_strength(Strength::new_clamped(f));
                    }
                    None => return default_error_response(
                        StatusCode::BAD_REQUEST,
                        host,
                        Some("must have query key `strength` with a floating point numeric value."),
                    )
                    .await,
                }
                r200()
            }
        ),
    );
    let controller = ctl();
    let save = saved();
    extensions.add_prepare_single(
        "/set-day-time",
        prepare!(
            request,
            host,
            _path,
            _addr,
            move |save: Arc<Mutex<save_state::DataWrapper>>, controller: ControllerSender| {
                let body = match read_body(request).await {
                    Ok(b) => b,
                    Err(_) => {
                        return default_error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            host,
                            Some("Failed to read request body"),
                        )
                        .await
                    }
                };

                let day_data = serde_json::from_slice(&body).ok();
                let command = day_data
                    .as_ref()
                    .and_then(|set_day: &datas::DayData| set_day.parse());

                match command {
                    Some((day, time)) => {
                        info!("Changed time of {:?} to {:?}", day, time);

                        {
                            let mut lock = save.lock().unwrap();
                            let week_scheduler = lock.get_mut().mut_week_scheduler();
                            *week_scheduler.get_mut(day) =
                                time.map(|time| time.format(&SECOND_FORMAT).unwrap());
                        }
                        {
                            controller.send(Command::ChangeDayTimer(day, time));
                        }
                    }
                    None => {
                        return default_error_response(
                            StatusCode::BAD_REQUEST,
                            host,
                            Some("Failed to serialize body"),
                        )
                        .await
                    }
                }
                r200()
            }
        ),
    );

    let controller = ctl();
    let save = saved();
    extensions.add_prepare_single(
        "/transition",
        prepare!(
            request,
            host,
            _path,
            _addr,
            move |save: Arc<Mutex<save_state::DataWrapper>>, controller: ControllerSender| {
                let body = match read_body(request).await {
                    Ok(b) => b,
                    Err(_) => {
                        return default_error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            host,
                            Some("Failed to read request body"),
                        )
                        .await
                    }
                };

                let action = get_query_value(request, "action");
                let transition = serde_json::from_slice(&body).ok().and_then(
                    |set_transition: datas::TransitionData| set_transition.to_transition(),
                );
                let transition = match transition {
                    Some(transition) => transition,
                    None => {
                        return default_error_response(
                            StatusCode::BAD_REQUEST,
                            host,
                            Some("Failed to serialize body"),
                        )
                        .await
                    }
                };

                match action.as_deref() {
                    Some("set") => {
                        save.lock()
                            .unwrap()
                            .get_mut()
                            .mut_week_scheduler()
                            .transition = datas::TransitionData::from_transition(&transition);
                        info!("Setting default transition.");
                        {
                            controller.send(Command::ChangeDayTimerTransition(transition));
                        }
                    }
                    Some("preview") => {
                        info!("Applying transition.");
                        {
                            controller.send(Command::SetTransition(transition));
                        }
                    }
                    _ => {
                        return default_error_response(
                            StatusCode::BAD_REQUEST,
                            host,
                            Some("Has to have a query key `action`"),
                        )
                        .await
                    }
                }

                r200()
            }
        ),
    );

    let local_state = state();
    extensions.add_prepare_single(
        "/get-state",
        prepare!(_request, _host, _path, _addr, move |local_state: Arc<
            Mutex<SharedState>,
        >| {
            let state = datas::StateData::from_shared_state(&local_state.lock().unwrap());
            let mut body = utils::WriteableBytes::with_capacity(1024);
            serde_json::to_writer(&mut body, &state).expect("failed to parse shared state");
            let body = body.into_inner().freeze();
            FatResponse::no_cache(Response::new(body))
        }),
    );

    let controller = ctl();
    let save = saved();
    extensions.add_prepare_single(
        "/add-scheduler",
        prepare!(
            request,
            host,
            _path,
            _addr,
            move |save: Arc<Mutex<save_state::DataWrapper>>, controller: ControllerSender| {
                let body = match read_body(request).await {
                    Ok(b) => b,
                    Err(_) => {
                        return default_error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            host,
                            Some("Failed to read request body"),
                        )
                        .await
                    }
                };

                let data = serde_json::from_slice(&body).ok();
                let command = data.and_then(|data: datas::AddSchedulerData| {
                    let data_clone = data.clone();
                    data.into_command(false).map(|cmd| (data_clone, cmd))
                });

                match command {
                    Some((data, cmd)) => {
                        {
                            controller.send(cmd);
                        }
                        save.lock().unwrap().get_mut().mut_schedulers().push(data);
                    }
                    None => {
                        return default_error_response(
                            StatusCode::BAD_REQUEST,
                            host,
                            Some("Failed to serialize body"),
                        )
                        .await
                    }
                }

                r200()
            }
        ),
    );

    let local_state = state();
    extensions.add_prepare_single(
        "/get-schedulers",
        prepare!(_request, _host, _path, _addr, move |local_state: Arc<
            Mutex<SharedState>,
        >| {
            let mut now = scheduler::LazyNow::new();

            let mut schedulers: Vec<(datas::SchedulerData, Option<Duration>)> = local_state
                .lock()
                .unwrap()
                .ref_schedulers()
                .iter()
                .map(|(name, scheduler)| {
                    (
                        datas::SchedulerData::from_scheduler(
                            scheduler.as_ref(),
                            name.to_string(),
                            &mut now,
                        ),
                        match scheduler.get_next(&mut now) {
                            Next::At(dur, _) => Some((dur - now.now()).unsigned_abs()),
                            Next::Unknown => None,
                        },
                    )
                })
                .collect();

            schedulers.sort_by(|(_, d1), (_, d2)| d1.cmp(d2));

            let schedulers: Vec<datas::SchedulerData> =
                schedulers.into_iter().map(|(data, _)| data).collect();

            let mut buffer = utils::WriteableBytes::with_capacity(1024);
            serde_json::to_writer(&mut buffer, &schedulers).expect("failed to write to Vec?");

            FatResponse::no_cache(Response::new(buffer.into_inner().freeze()))
        }),
    );

    let controller = ctl();
    extensions.add_prepare_single(
        "/remove-scheduler",
        prepare!(
            request,
            host,
            _path,
            _addr,
            move |controller: ControllerSender| {
                match get_query_value(request, "name") {
                    Some(s) => {
                        {
                            controller.send(Command::RemoveScheduler(s));
                        }
                        // We don't save since we check if internal schedulers disappeared.
                    }
                    None => {
                        return default_error_response(
                            StatusCode::BAD_REQUEST,
                            host,
                            Some("Has to have the query key `name`"),
                        )
                        .await
                    }
                }

                r200()
            }
        ),
    );
    let controller = ctl();
    extensions.add_prepare_single(
        "/set-effect",
        prepare!(
            request,
            host,
            _path,
            _addr,
            move |controller: ControllerSender| {
                let body = match read_body(request).await {
                    Ok(b) => b,
                    Err(_) => {
                        return default_error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            host,
                            Some("Failed to read request body"),
                        )
                        .await
                    }
                };

                let data: Option<datas::EffectData> = serde_json::from_slice(&body).ok();
                let command = data.and_then(datas::EffectData::into_command);

                match command {
                    Some(cmd) => {
                        controller.send(cmd);
                    }
                    None => {
                        return default_error_response(
                            StatusCode::BAD_REQUEST,
                            host,
                            Some("Failed to serialize body"),
                        )
                        .await
                    }
                }

                r200()
            }
        ),
    );
    #[cfg(feature = "esp32")]
    {
        let controller = ctl();
        let nvs = storage.clone();
        extensions.add_prepare_single(
            "/set-timezone",
            prepare!(
                request,
                host,
                _path,
                _addr,
                move |nvs: NvsStorage, controller: ControllerSender| {
                    match get_query_value(request, "timezone") {
                        Some(timezone) => {
                            if httpwm::env_timezone::try_set_timezone(&timezone).is_err() {
                                default_error_response(
                                StatusCode::BAD_REQUEST,
                                host,
                                Some("timezone needs to have the format '[+-]hh:mm' (e.g. +01:00)"),
                            )
                            .await
                            } else {
                                controller.send(Command::UpdateWake);
                                let mut lock = nvs.lock().unwrap();
                                if lock
                                    .put_raw("timezone.txt", timezone.trim().as_bytes())
                                    .is_err()
                                {
                                    error!("Failed to write timezone");
                                }
                                r200()
                            }
                        }
                        None => {
                            default_error_response(
                                StatusCode::BAD_REQUEST,
                                host,
                                Some("must have query key `timezone` with a timezone offset"),
                            )
                            .await
                        }
                    }
                }
            ),
        );
    }
    // wifi
    #[cfg(feature = "esp32")]
    {
        let nvs = storage.clone();
        type Networks = Arc<Mutex<HashMap<String, String>>>;
        let networks = Arc::new(Mutex::new(known_networks));
        let get_networks = Arc::clone(&networks);
        extensions.add_prepare_single(
            "/get-wifi",
            prepare!(_req, _host, _path, _addr, move |get_networks: Networks| {
                let lock = get_networks.lock().unwrap();
                let data = serde_json::to_string(&*lock).unwrap();
                drop(lock);
                FatResponse::no_cache(Response::new(Bytes::from(data.into_bytes())))
            }),
        );
        extensions.add_prepare_single(
            "/set-wifi",
            prepare!(req, host, _path, _addr, move |networks: Networks, nvs: NvsStorage| {
                let Ok(data) = read_body(req).await else {
                    return default_error_response(StatusCode::BAD_REQUEST, host, None).await
                };
                let Ok(parsed_networks) = serde_json::from_slice::<'_, HashMap<String, String>>(&data) else {
                    return default_error_response(StatusCode::BAD_REQUEST, host, None).await
                };
                let ser = ron::to_string(&parsed_networks).unwrap();
                {
                    let mut lock = networks.lock().unwrap();
                    *lock = parsed_networks;
                }
                {
                    let mut lock = nvs.lock().unwrap();
                    if lock.put_raw("networks.ron", ser.as_bytes()).is_err() {
                        error!("Failed to write networks.ron: {ser}");
                    }
                }

                r200()
            }),
        );
    }
    #[cfg(feature = "esp32")]
    // serve files
    {
        let mut add_path = |path: &str, data: &'static str| {
            extensions.add_prepare_single(
                path,
                prepare!(_, _, _, _, move |data: &'static str| {
                    FatResponse::cache(Response::new(Bytes::from_static(data.as_bytes())))
                }),
            )
        };
        add_path("/index.html", INDEX_HTML);
        add_path("/script.js", SCRIPT_JS);
        add_path("/style.css", STYLE_CSS);
    }

    let mut localhost = Host::unsecure(
        "localhost",
        PathBuf::from("web"),
        extensions,
        host::Options::new(),
    );
    localhost.disable_server_cache().disable_client_cache();
    localhost.limiter.disable();
    #[cfg(feature = "esp32")]
    localhost.options.disable_fs();
    localhost
}
// #[cfg(all(feature = "web", not(feature = "esp32")))]
#[cfg(feature = "web")]
fn create_server<T: VariableOut + Send>(
    controller: Arc<Mutex<Controller<T>>>,
    save_state: Arc<Mutex<save_state::DataWrapper>>,
    shared: Arc<Mutex<SharedState>>,
    #[cfg(feature = "esp32")] known_networks: HashMap<String, String>,
    #[cfg(feature = "esp32")] storage: NvsStorage,
) -> kvarn::RunConfig {
    #[cfg(feature = "esp32")]
    let default_port = 80;
    #[cfg(not(feature = "esp32"))]
    let default_port = 8080;
    let port = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(default_port);

    let localhost = create_host(
        controller,
        save_state,
        shared,
        #[cfg(feature = "esp32")]
        known_networks,
        #[cfg(feature = "esp32")]
        storage,
    );
    let hosts = HostCollection::builder().default(localhost).build();
    RunConfig::new().bind(PortDescriptor::new(port, hosts).ipv4_only())
}
// #[cfg(feature = "esp32")]
// fn create_server(
// channel: tokio::sync::mpsc::UnboundedSender<(
// FatRequest,
// std::sync::mpsc::SyncSender<Response<Bytes>>,
// )>,
// ) -> esp_idf_svc::httpd::ServerRegistry {
// fn convert_request(req: &mut embedded_svc::httpd::Request, handler_path: &str) -> FatRequest {
// let qs = req.query_string();
// let body = kvarn::application::Body::Bytes(Bytes::from(req.as_bytes().unwrap()).into());
// let mut req = FatRequest::new(body);
// let mut path =
// BytesMut::with_capacity(handler_path.len() + qs.as_ref().map_or(0, |qs| 1 + qs.len()));
// path.extend_from_slice(handler_path.as_bytes());
// if let Some(qs) = qs {
// path.extend_from_slice(b"?");
// path.extend_from_slice(qs.as_bytes());
// }
// *req.uri_mut() = Uri::from_maybe_shared(path).unwrap();
// req
// }
// fn convert_response(mut res: Response<Bytes>) -> embedded_svc::httpd::Response {
// embedded_svc::httpd::Response {
// status: res.status().as_u16(),
// status_message: None,
// headers: res
// .headers_mut()
// .into_iter()
// .map(|(k, v)| {
// (
// k.as_str().to_owned(),
// String::from_utf8_lossy(v.as_bytes()).into_owned(),
// )
// })
// .collect(),
// body: embedded_svc::httpd::Body::Bytes(res.into_body().into()),
// new_session_state: None,
// }
// }

// use embedded_svc::httpd::{registry::Registry, Method};

// let mut registry = esp_idf_svc::httpd::ServerRegistry::new();
// macro_rules! add_to_registry {
// ($path: expr, $method: expr) => {
// let channel = channel.clone();
// registry = registry
// .handler(embedded_svc::httpd::Handler::new(
// $path,
// $method,
// move |mut req| {
// let request = convert_request(&mut req, $path);
// let (tx, rx) = std::sync::mpsc::sync_channel(0);
// channel.send((request, tx)).unwrap();
// let response = rx.recv().unwrap();
// Ok(convert_response(response))
// },
// ))
// .unwrap();
// };
// }
// macro_rules! add_file_to_registry {
// ($path: expr, $file: expr, $ct: expr) => {
// registry = registry
// .handler(embedded_svc::httpd::Handler::new(
// $path,
// Method::Get,
// |_req| {
// let mut headers = std::collections::BTreeMap::new();
// headers.insert("content-type".into(), $ct.into());
// let response = embedded_svc::httpd::Response {
// status: 200,
// status_message: None,
// headers,
// body: embedded_svc::httpd::Body::Bytes($file.as_bytes().to_vec()),
// new_session_state: None,
// };
// Ok(response)
// },
// ))
// .unwrap();
// };
// }
// add_to_registry!("/clear-schedulers", Method::Get);
// add_to_registry!("/set-strength", Method::Get);
// add_to_registry!("/set-day-time", Method::Put);
// add_to_registry!("/transition", Method::Put);
// add_to_registry!("/get-state", Method::Get);
// add_to_registry!("/add-scheduler", Method::Put);
// add_to_registry!("/get-schedulers", Method::Get);
// add_to_registry!("/remove-schedulers", Method::Get);
// add_to_registry!("/set-effect", Method::Put);
// add_file_to_registry!("/", INDEX_HTML, "text/html");
// add_file_to_registry!("/index.html", INDEX_HTML, "text/html");
// add_file_to_registry!("/script.js", SCRIPT_JS, "application/javascript");
// add_file_to_registry!("/style.css", STYLE_CSS, "text/css");
// registry
// }

pub fn parse_time(string: &str) -> Option<time::Time> {
    time::Time::parse(string, &SECOND_FORMAT)
        .or_else(|_| time::Time::parse(string, &MINUTE_FORMAT))
        .ok()
}

#[cfg(feature = "esp32")]
fn wifi(
    netif_stack: Arc<EspNetifStack>,
    sys_loop_stack: Arc<EspSysLoopStack>,
    default_nvs: Arc<EspDefaultNvs>,
    networks: &HashMap<String, String>,
) -> Result<Box<EspWifi>, ()> {
    let mut wifi =
        Box::new(EspWifi::new(netif_stack, sys_loop_stack, default_nvs).map_err(|_| ())?);

    info!("Wifi created, about to scan");

    let ap_infos = wifi.scan().map_err(|_| ())?;

    let ours = ap_infos
        .into_iter()
        .find_map(|a| networks.get(a.ssid.as_str()).map(|passwd| (a, passwd)));

    let (ap, password) = if let Some((ours, passwd)) = ours {
        info!(
            "Found configured access point {} on channel {}",
            ours.ssid, ours.channel
        );
        (ours, passwd)
    } else {
        error!("Didn't find the selected WIFI!");
        return Err(());
    };

    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: ap.ssid,
        password: password.as_str().into(),
        channel: Some(ap.channel),
        auth_method: ap.auth_method,
        ..Default::default()
    }))
    .map_err(|_| ())?;

    info!("Wifi configuration set, about to get status");

    wifi.wait_status_with_timeout(Duration::from_secs(20), |status| !status.is_transitional())
        .map_err(|e| error!("Unexpected Wifi status: {:?}", e))?;

    let status = wifi.get_status();

    if let Status(
        ClientStatus::Started(ClientConnectionStatus::Connected(ClientIpStatus::Done(ip_settings))),
        _,
    ) = status
    {
        info!("Wifi connected at {}", ip_settings.ip);
    } else {
        error!("Unexpected Wifi status: {:?}", status);
        return Err(());
    }

    Ok(wifi)
}

#[cfg(feature = "test")]
mod test_output {
    use std::fmt::Write as WriteFmt;
    use std::io::Write;
    use std::sync::mpsc;

    fn size() -> Option<(u16, u16)> {
        use libc::winsize;
        use std::os::unix::io::AsRawFd;

        fn wrap_with_result(result: i32) -> std::io::Result<()> {
            if result == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        }

        // http://rosettacode.org/wiki/Terminal_control/Dimensions#Library:_BSD_libc
        let mut size = winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        let file = std::fs::File::open("/dev/tty");
        let fd = if let Ok(file) = &file {
            file.as_raw_fd()
        } else {
            // Fallback to libc::STDOUT_FILENO if /dev/tty is missing
            libc::STDOUT_FILENO
        };

        #[allow(clippy::useless_conversion)]
        if wrap_with_result(unsafe { libc::ioctl(fd, libc::TIOCGWINSZ.into(), &mut size) }).is_ok()
            && size.ws_col != 0
            && size.ws_row != 0
        {
            Some((size.ws_col, size.ws_row))
        } else {
            None
        }
    }
    pub fn spawn() -> mpsc::SyncSender<f64> {
        let (tx, rx) = mpsc::sync_channel(16);
        std::thread::spawn(move || {
            let sleep = std::time::Duration::from_millis(200);
            let start = std::time::Instant::now();
            let mut deadline = start + sleep;
            let mut s = 0.;
            let rows = 20;
            let mut strengths = [0.; 100];
            let mut out = String::new();
            loop {
                match rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                {
                    Ok(strength) => {
                        s = strength;
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let mut o = std::io::stdout().lock();
                        // update info
                        let cols = if let Some((cols, _)) = size() {
                            cols
                        } else {
                            break;
                        };
                        strengths.copy_within(1.., 0);
                        *strengths.last_mut().unwrap() = s;

                        // save cursor pos
                        // write!(out, "\x1b[s").unwrap();
                        for _ in 0..cols {
                            out.push(' ');
                        }
                        out.push('\n');
                        for row in (0..rows).rev() {
                            let threshold = (row + 1) as f64 / rows as f64;
                            for col in 0..cols {
                                let idx = (col * strengths.len() as u16) / cols;
                                if strengths[idx as usize] >= threshold {
                                    // full block: █
                                    out.push('\u{2588}')
                                } else {
                                    out.push(' ');
                                }
                            }
                            out.push('\n');
                        }
                        // move up `rows` lines.
                        write!(out, "\x1b[{}F", rows + 1).unwrap();

                        // load cursor pos
                        // write!(out, "\x1b[u").unwrap();

                        o.write_all(out.as_bytes()).unwrap();
                        o.flush().unwrap();
                        out.clear();
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                deadline += sleep;
            }
        });
        tx
    }
}
/// Quite nasty code
pub mod save_state {
    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct WeekSchedulerData {
        pub mon: Option<String>,
        pub tue: Option<String>,
        pub wed: Option<String>,
        pub thu: Option<String>,
        pub fri: Option<String>,
        pub sat: Option<String>,
        pub sun: Option<String>,
        pub transition: datas::TransitionData,
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
                    $e.map(|time| time.format(&SECOND_FORMAT).unwrap())
                };
            }

            WeekSchedulerData {
                mon: fmt_time!(scheduler.mon),
                tue: fmt_time!(scheduler.tue),
                wed: fmt_time!(scheduler.wed),
                thu: fmt_time!(scheduler.thu),
                fri: fmt_time!(scheduler.fri),
                sat: fmt_time!(scheduler.sat),
                sun: fmt_time!(scheduler.sun),
                transition: datas::TransitionData::from_transition(&scheduler.transition),
            }
        }
        pub fn to_scheduler(&self) -> Option<WeekScheduler> {
            macro_rules! fmt_time {
                ($e:expr) => {
                    match $e.as_ref() {
                        Some(time) => Some(time::Time::parse(time.as_str(), SECOND_FORMAT).ok()?),
                        None => None,
                    }
                };
            }

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
        pub strength: Option<f64>,
        pub schedulers: Vec<datas::AddSchedulerData>,
        pub week_scheduler: Option<WeekSchedulerData>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub current_transition: Option<datas::TransitionData>,
    }
    impl Data {
        pub fn read_from_file<P: AsRef<Path>>(
            path: P,
            week_scheduler: &WeekScheduler,
        ) -> io::Result<Self> {
            fn read(path: &Path) -> io::Result<Data> {
                let file = std::fs::File::open(path)?;
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
                .filter_map(|s| s.clone().into_command(true))
            {
                controller.send(scheduler);
            }
            if let Some(transition) = self
                .current_transition
                .as_ref()
                .and_then(datas::TransitionData::to_transition)
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
                .map(Strength::new_clamped)
        }

        pub fn ref_schedulers(&self) -> &Vec<datas::AddSchedulerData> {
            &self.schedulers
        }
        pub fn mut_schedulers(&mut self) -> &mut Vec<datas::AddSchedulerData> {
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
        pub fn set_transition(
            &mut self,
            new: Option<&Transition>,
        ) -> Option<datas::TransitionData> {
            match new {
                None => self.current_transition.take(),
                Some(transition) => self
                    .current_transition
                    .replace(datas::TransitionData::from_transition(transition)),
            }
        }
    }
}

pub mod datas {
    use httpwm::primitive_to_tz;

    use super::*;
    #[derive(Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct DayData {
        day: String,
        time: Option<String>,
    }
    impl DayData {
        pub fn parse(&self) -> Option<(Weekday, Option<time::Time>)> {
            let day: Weekday = self.day.parse().ok()?;
            let time = match self.time.as_ref() {
                Some(time) => Some(parse_time(time)?),
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

            let interpolation =
                TransitionInterpolation::from_str(&self.interpolation, &self.extras)?;
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
            let mut day = Weekday::Mon;
            for _ in 0..7 {
                days.insert(
                    weekday_to_lowercase_str(&day).to_string(),
                    state
                        .ref_week_schedule()
                        .get(day)
                        .map(|time| time.format(&SECOND_FORMAT).unwrap()),
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
    #[derive(Debug, Deserialize, Serialize, Clone)]
    pub struct AddSchedulerData {
        pub kind: String,
        pub time: String,
        pub name: String,
        pub description: String,
        pub extras: Vec<String>,
        pub transition: TransitionData,
    }
    impl AddSchedulerData {
        pub fn into_command(self, allow_past: bool) -> Option<Command> {
            let transition = self.transition.to_transition()?;
            let time = parse_time(&self.time)?;
            // Unwrap is ok, since we know `SetTransition` is clonable
            let run_command = ClonableCommand::new(Command::SetTransition(transition)).unwrap();
            let common = extra_schedulers::Common::new(self.description, run_command);

            let scheduler: Box<dyn Scheduler> = match self.kind.as_str() {
                "at" if self.extras.len() == 1 => {
                    let date_time = time::Date::parse(self.extras[0].as_str(), &DATE_FORMAT)
                        .ok()?
                        .with_time(time);
                    if has_occurred(primitive_to_tz(date_time)) && !allow_past {
                        return None;
                    }
                    Box::new(extra_schedulers::At::new(
                        common,
                        primitive_to_tz(date_time),
                    ))
                }
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
                    let dur = date_time - now.now();
                    if dur.whole_days() > 0 {
                        (now.now() + dur).format(&DATE_TIME_FORMAT).unwrap()
                    } else if dur.whole_hours() > 0 {
                        format!("In {} hours", dur.whole_hours())
                    } else if dur.whole_minutes() > 0 {
                        format!("In {} minutes", dur.whole_minutes())
                    } else {
                        format!("In {} seconds", dur.whole_seconds())
                    }
                }
                Next::Unknown => "unknown".to_string(),
            };

            Self {
                name,
                description: scheduler.description().to_string(),
                kind: scheduler.kind().to_string(),
                next_occurrence,
            }
        }
    }

    #[derive(Debug, Deserialize, Serialize, Clone)]
    pub struct EffectData {
        pub kind: String,
        pub nums: Vec<f64>,
    }
    impl EffectData {
        pub fn into_command(self) -> Option<Command> {
            match self.kind.as_str() {
                "radar" => {
                    let offset = *self.nums.first()?;
                    let speed = *self.nums.get(1)?;
                    Some(Command::SetEffect(Effect::Radar { offset, speed }))
                }
                _ => None,
            }
        }
    }
}
pub mod extra_schedulers {
    use httpwm::scheduler::Keep;

    use super::*;
    use httpwm::primitive_to_tz;
    use time::OffsetDateTime;

    pub(crate) fn get_next_day<F: Fn(Weekday) -> Option<time::Time>>(
        from: Weekday,
        get: F,
    ) -> Option<(time::Time, u8)> {
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
        moment: OffsetDateTime,
    }
    impl At {
        pub fn new(common: Common, moment: OffsetDateTime) -> Self {
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
        time: time::Time,
        day: Weekday,
    }
    impl EveryWeek {
        pub fn new(common: Common, time: time::Time, day: Weekday) -> Self {
            Self { common, time, day }
        }
    }
    impl Scheduler for EveryWeek {
        fn get_next(&self, now: &mut scheduler::LazyNow) -> Next {
            let now = now.now();
            if self.day == Weekday::from(now.weekday()) && now.time() < self.time {
                // Unwrap is OK, now will never be over self.time.
                Next::At(
                    now.replace_time(self.time),
                    self.common.get_command().into_inner(),
                )
            } else {
                // Unwrap is ok, we must have one day containing a date.
                let (time, offset): (time::Time, _) = get_next_day(now.weekday().into(), |day| {
                    if day == self.day {
                        Some(self.time)
                    } else {
                        None
                    }
                })
                .unwrap();
                Next::At(
                    primitive_to_tz(
                        now.date().with_time(time) + time::Duration::days(offset as i64),
                    ),
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
        time: time::Time,
    }
    impl EveryDay {
        pub fn new(common: Common, time: time::Time) -> Self {
            Self { common, time }
        }
    }
    impl Scheduler for EveryDay {
        fn get_next(&self, now: &mut scheduler::LazyNow) -> Next {
            let now = now.now();
            if now.time() < self.time {
                // Unwrap is OK, now will never be over self.time.
                Next::At(
                    now.replace_time(self.time),
                    self.common.get_command().into_inner(),
                )
            } else {
                // Unwrap is OK, it's one day ahead!
                Next::At(
                    now.replace_time(self.time) + time::Duration::days(1),
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
