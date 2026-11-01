use bandwidth_limiter::{
    i18n::Message,
    model::{Config, Usage},
    platform::{self, Connection},
    policy::Engine,
    proxy::{Server, Status},
    storage,
    updater::{self, State},
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

pub enum Command {
    Save(Config),
    Call(bool),
    Start(u16),
    Stop,
    Refresh,
    Monitor(bool),
    Shutdown,
}
pub enum Event {
    Message(Message),
    Saved(String),
    Closed,
}
#[derive(Clone, Default, PartialEq)]
pub struct View {
    pub status: Status,
    pub usage: Usage,
    pub rates: BTreeMap<String, [f64; 2]>,
    pub connections: Vec<Connection>,
    pub connection_note: Message,
}
pub enum UpdateCommand {
    Check,
    Download(Box<updater::Release>),
    Install,
}

pub struct Runtime {
    pub system_tcp_commands: SyncSender<bandwidth_limiter::traffic::Command>,
    pub system_tcp_state: Arc<Mutex<bandwidth_limiter::traffic::State>>,
    pub commands: SyncSender<Command>,
    pub events: Receiver<Event>,
    pub view: Arc<Mutex<Arc<View>>>,
    pub update_commands: SyncSender<UpdateCommand>,
    pub update_state: Arc<Mutex<State>>,
    pub update_exit: Arc<std::sync::atomic::AtomicBool>,
}

impl Runtime {
    /// No workers, I/O, device access or update checks in the headless fixture path.
    pub fn preview() -> Self {
        let (system_tcp_commands, _) = mpsc::sync_channel(1);
        let (commands, _) = mpsc::sync_channel(1);
        let (_, events) = mpsc::sync_channel(1);
        let (update_commands, _) = mpsc::sync_channel(1);
        Self {
            system_tcp_commands,
            system_tcp_state: Arc::new(Mutex::new(Default::default())),
            commands,
            events,
            view: Arc::new(Mutex::new(Arc::new(View::default()))),
            update_commands,
            update_state: Arc::new(Mutex::new(State::Idle)),
            update_exit: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
    pub fn new(config: Config, usage: Usage, directory: PathBuf, ctx: egui::Context) -> Self {
        let (system_tcp_commands, requests) = mpsc::sync_channel(2);
        let system_tcp_state = Arc::new(Mutex::new(bandwidth_limiter::traffic::State::default()));
        let system_tcp_shared = system_tcp_state.clone();
        let system_tcp_ctx = ctx.clone();
        thread::Builder::new()
            .name("packet-service-client".into())
            .spawn(move || {
                while let Ok(command) = requests.recv() {
                    system_tcp_shared.lock().unwrap().busy = true;
                    system_tcp_ctx.request_repaint();
                    let result = bandwidth_limiter::traffic::action(command)
                        .and_then(bandwidth_limiter_packet_control::transport::exchange);
                    let mut state = system_tcp_shared.lock().unwrap();
                    state.busy = false;
                    match result {
                        Ok(response) => {
                            state.error = response.error.clone();
                            state.response = Some(response);
                        }
                        Err(error) => {
                            state.error = Some(error);
                            state.response = None;
                        }
                    }
                    drop(state);
                    system_tcp_ctx.request_repaint();
                }
            })
            .expect("Cannot create WinDivert client worker");
        let (commands, rx) = mpsc::sync_channel(8);
        let (tx, events) = mpsc::sync_channel(8);
        let engine = Arc::new(Mutex::new(Engine::new(config, usage)));
        let status = Arc::new(Mutex::new(Status::default()));
        let view = Arc::new(Mutex::new(Arc::new(View {
            usage: engine.lock().unwrap().usage.clone(),
            connection_note:
                "Not sampled. Refresh reads Windows endpoints; direct-traffic bytes are unknown."
                    .into(),
            ..View::default()
        })));
        let worker_view = view.clone();
        let worker_ctx = ctx.clone();
        let worker_directory = directory.clone();
        thread::Builder::new().name("policy-control".into()).spawn(move || {
            let mut proxy: Option<Server> = None;
            let mut monitor = false;
            let mut last_save = Instant::now();
            let mut previous_time = Instant::now();
            let mut previous = BTreeMap::<String, [u64; 2]>::new();
            let mut last_usage = String::new();
            let mut connections = vec![]; let mut connection_note = "Not sampled. Refresh reads Windows TCP/UDP endpoints; byte counters are unavailable for direct traffic.".into();
            loop {
                let active = proxy.is_some() || monitor;
                let command = if active { rx.recv_timeout(Duration::from_secs(1)) }
                    else { rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected) };
                let mut refresh = false;
                let mut closing = false;
                let timed_out = matches!(&command, Err(mpsc::RecvTimeoutError::Timeout));
                match command {
                    Ok(Command::Save(config)) => {
                        let result = config.validate().and_then(|()| storage::write_json(&worker_directory.join("settings.json"), &config));
                        match result {
                            Ok(()) => {
                                let saved = serde_json::to_string(&config).unwrap_or_default();
                                engine.lock().unwrap().configure(config); let _ = tx.send(Event::Saved(saved));
                            }
                            Err(e) => { let _ = tx.send(Event::Message(Message::new("Settings were not applied: {e}", vec![e]))); }
                        }
                    }
                    Ok(Command::Call(enabled)) => { engine.lock().unwrap().set_call_mode(enabled); }
                    Ok(Command::Start(port)) if proxy.is_none() => {
                        match Server::start(port, engine.clone(), status.clone()) {
                            Ok(server) => { proxy = Some(server); let _ = tx.send(Event::Message(Message::new("SOCKS5 listening on 127.0.0.1:{port}; only enabled executable rules are accepted", vec![port.to_string()]))); }
                            Err(e) => { let _ = tx.send(Event::Message(e.into())); }
                        }
                    }
                    Ok(Command::Stop) => { proxy.take(); }
                    Ok(Command::Monitor(enabled)) => { monitor = enabled; refresh = enabled; }
                    Ok(Command::Refresh) => { refresh = true; }
                    Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => { proxy.take(); closing = true; }
                    _ => {}
                }
                if refresh || (monitor && previous_time.elapsed() >= Duration::from_secs(1)) {
                    match platform::snapshot() {
                        Ok(snapshot) => { connections = snapshot; connection_note = Message::new("{} endpoints • {} • read-only; direct-traffic bytes are unknown", vec![connections.len().to_string(), chrono::Local::now().format("%H:%M:%S").to_string()]); }
                        Err(e) => { connection_note = e.into(); }
                    }
                }
                let usage = engine.lock().unwrap().usage.clone();
                if closing || proxy.is_none() || last_save.elapsed() >= Duration::from_secs(5) {
                    let serialized = serde_json::to_string(&usage).unwrap_or_default();
                    if serialized != last_usage {
                        match storage::write_json(&worker_directory.join("usage.json"), &usage) {
                            Ok(()) => { last_usage = serialized; }
                            Err(e) => { let _ = tx.send(Event::Message(Message::new("Usage checkpoint failed; restart may lose quota history: {e}", vec![e]))); }
                        }
                    }
                    last_save = Instant::now();
                }
                let current = status.lock().unwrap().clone();
                let elapsed = previous_time.elapsed().as_secs_f64().max(0.001);
                let rates = current.forwarded.iter().map(|(key, values)| {
                    let old = previous.get(key).copied().unwrap_or([0, 0]);
                    (key.clone(), [(values[0].saturating_sub(old[0])) as f64 / elapsed,
                        (values[1].saturating_sub(old[1])) as f64 / elapsed])
                }).collect();
                previous = current.forwarded.clone(); previous_time = Instant::now();
                let next = View { status: current, usage, rates, connections: connections.clone(), connection_note: connection_note.clone() };
                let changed = { let mut shared = worker_view.lock().unwrap();
                    let changed = shared.as_ref() != &next; if changed { *shared = Arc::new(next); } changed };
                if changed || !timed_out { worker_ctx.request_repaint(); }
                if closing { let _ = tx.send(Event::Closed); worker_ctx.request_repaint(); break; }
            }
        }).expect("Cannot create policy worker");

        let (update_commands, updates) = mpsc::sync_channel(1);
        let update_state = Arc::new(Mutex::new(State::Idle));
        let shared_state = update_state.clone();
        let update_exit = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let exit = update_exit.clone();
        thread::Builder::new()
            .name("release-updates".into())
            .spawn(move || {
                let mut prepared = None;
                while let Ok(command) = updates.recv() {
                    match command {
                        UpdateCommand::Check => {
                            *shared_state.lock().unwrap() = State::Checking;
                            ctx.request_repaint();
                            let result = match updater::check() {
                                Ok(Some(release)) => State::Available(Box::new(release)),
                                Ok(None) => State::Current,
                                Err(e) => State::Error(e),
                            };
                            *shared_state.lock().unwrap() = result;
                        }
                        UpdateCommand::Download(release) => {
                            *shared_state.lock().unwrap() = State::Downloading {
                                received: 0,
                                total: 0,
                            };
                            ctx.request_repaint();
                            let result = match updater::download(&release, |received, total| {
                                *shared_state.lock().unwrap() =
                                    State::Downloading { received, total };
                                ctx.request_repaint()
                            }) {
                                Ok(value) => {
                                    prepared = Some(value);
                                    State::Ready
                                }
                                Err(e) => State::Error(e),
                            };
                            *shared_state.lock().unwrap() = result;
                        }
                        UpdateCommand::Install => {
                            let result = prepared
                                .as_mut()
                                .ok_or_else(|| {
                                    "No verified update is ready. Download it again.".to_string()
                                })
                                .and_then(updater::install);
                            match result {
                                Ok(()) => {
                                    exit.store(true, std::sync::atomic::Ordering::Release);
                                }
                                Err(e) => {
                                    *shared_state.lock().unwrap() = State::Error(e);
                                }
                            }
                        }
                    }
                    ctx.request_repaint();
                }
            })
            .expect("Cannot create update worker");
        Self {
            system_tcp_commands,
            system_tcp_state,
            commands,
            events,
            view,
            update_commands,
            update_state,
            update_exit,
        }
    }
}
