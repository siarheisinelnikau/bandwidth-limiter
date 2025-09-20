use crate::runtime::{Command, Event, Runtime, UpdateCommand, View};
use bandwidth_limiter::{
    model::{Config, Rule, Schedule, Usage, VERSION, app_key},
    updater::State,
};
use eframe::egui::{self, Color32, RichText};
use std::{fs::File, path::PathBuf, sync::atomic::Ordering, time::Duration};

const ACCENT: Color32 = Color32::from_rgb(77, 204, 177);

pub struct App {
    config: Config,
    runtime: Runtime,
    view: View,
    page: usize,
    selected: usize,
    message: String,
    dirty: bool,
    call_mode: bool,
    monitor: bool,
    closing: bool,
    can_close: bool,
    filter: String,
    directory: PathBuf,
    _guard: File,
}

impl App {
    pub fn new(
        config: Config,
        usage: Usage,
        directory: PathBuf,
        guard: File,
        ctx: egui::Context,
    ) -> Self {
        let runtime = Runtime::new(config.clone(), usage, directory.clone(), ctx);
        if config.startup_update_check {
            let _ = runtime.update_commands.try_send(UpdateCommand::Check);
        }
        Self { config, runtime, view: View::default(), page: 0, selected: 0,
            message: "Add executable rules, save them, then start the proxy. System-wide shaping is unavailable in this build.".into(),
            dirty: false, call_mode: false, monitor: false, closing: false, can_close: false, filter: String::new(), directory, _guard: guard }
    }

    fn command(&mut self, command: Command) -> bool {
        if let Err(error) = self.runtime.commands.try_send(command) {
            self.message = format!("Background worker is busy: {error}. Retry shortly.");
            false
        } else {
            true
        }
    }
    fn update_command(&mut self, command: UpdateCommand) {
        if let Err(error) = self.runtime.update_commands.try_send(command) {
            self.message = format!("Update worker is busy: {error}");
        }
    }

    fn save(&mut self) {
        match self.config.validate() {
            Ok(()) => {
                self.command(Command::Save(self.config.clone()));
            }
            Err(e) => {
                self.message = e;
            }
        }
    }

    fn traffic(&mut self, ui: &mut egui::Ui) {
        ui.heading("Traffic overview");
        ui.label("TCP payload forwarding for applications explicitly configured to use this SOCKS5 proxy.");
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if self.view.status.running {
                ui.colored_label(ACCENT, "PROXY RUNNING");
                if ui.button("Stop proxy").clicked() {
                    self.command(Command::Stop);
                }
            } else if ui.button("Start loopback proxy").clicked() {
                if self.dirty {
                    self.message = "Save changes before starting the proxy.".into();
                } else {
                    self.command(Command::Start(self.config.proxy_port));
                }
            }
            ui.label(format!(
                "127.0.0.1:{}  •  {} active / 32 max",
                self.config.proxy_port, self.view.status.active
            ));
        });
        ui.add_space(10.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut enabled = self.call_mode;
                if ui.checkbox(&mut enabled, RichText::new("Call mode").strong()).changed()
                    && self.command(Command::Call(enabled)) { self.call_mode = enabled; }
                ui.label(format!("Background apps: ↓ {} KiB/s  ↑ {} KiB/s", self.config.call_download_kib, self.config.call_upload_kib));
            });
            ui.small("Applies a shared cap to each enabled rule marked Background during calls. Direct traffic and UDP are unaffected.");
        });
        ui.add_space(12.0);
        ui.strong("Measured proxy activity");
        egui::Grid::new("proxy-activity")
            .num_columns(4)
            .striped(true)
            .min_col_width(115.0)
            .show(ui, |ui| {
                ui.strong("Application");
                ui.strong("Download");
                ui.strong("Upload");
                ui.strong("Daily usage");
                ui.end_row();
                for rule in &self.config.rules {
                    let key = app_key(&rule.path);
                    let rate = self.view.rates.get(&key).copied().unwrap_or([0.0; 2]);
                    let usage = self.view.usage.apps.get(&key).copied().unwrap_or([0; 2]);
                    ui.label(name(&rule.path)).on_hover_text(&rule.path);
                    ui.label(format!("{:.1} KiB/s", rate[0] / 1024.0));
                    ui.label(format!("{:.1} KiB/s", rate[1] / 1024.0));
                    ui.label(format!(
                        "{:.2} MiB",
                        (usage[0] + usage[1]) as f64 / 1048576.0
                    ));
                    ui.end_row();
                }
            });
        if self.config.rules.is_empty() {
            ui.label("No application rules yet. Add them in Rules & schedules.");
        }
        ui.small(format!("Usage date: {}. Daily usage includes in-flight quota reservations; rates count successfully forwarded payloads.", self.view.usage.day));
        if !self.view.status.last_error.is_empty() {
            ui.colored_label(
                Color32::from_rgb(240, 188, 114),
                format!("Last proxy event: {}", self.view.status.last_error),
            );
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("Windows connection inventory");
            if ui.button("Refresh").clicked() {
                self.command(Command::Refresh);
            }
            let mut enabled = self.monitor;
            if ui.checkbox(&mut enabled, "Refresh every second").changed()
                && self.command(Command::Monitor(enabled))
            {
                self.monitor = enabled;
            }
        });
        ui.small(&self.view.connection_note);
        ui.horizontal(|ui| {
            ui.label("Filter");
            ui.text_edit_singleline(&mut self.filter);
        });
        let filter = self.filter.to_lowercase();
        let mut add = None;
        egui::ScrollArea::vertical()
            .id_salt("connections")
            .max_height(260.0)
            .show(ui, |ui| {
                let entries: Vec<_> = self
                    .view
                    .connections
                    .iter()
                    .filter(|r| {
                        filter.is_empty()
                            || r.path
                                .as_deref()
                                .unwrap_or("Unknown process")
                                .to_lowercase()
                                .contains(&filter)
                            || r.pid.to_string().contains(&filter)
                    })
                    .collect();
                egui::Grid::new("connection-table")
                    .striped(true)
                    .num_columns(5)
                    .show(ui, |ui| {
                        ui.strong("Process / PID");
                        ui.strong("Protocol");
                        ui.strong("Local → Remote");
                        ui.strong("State");
                        ui.label("");
                        ui.end_row();
                        for r in entries.iter().take(500) {
                            ui.label(format!(
                                "{} ({})",
                                r.path.as_deref().map(name).unwrap_or("Unknown / protected"),
                                r.pid
                            ))
                            .on_hover_text(
                                r.path
                                    .as_deref()
                                    .unwrap_or("Executable identity unavailable"),
                            );
                            ui.label(r.protocol);
                            ui.label(format!(
                                "{} → {}",
                                r.local,
                                r.remote
                                    .map(|a| a.to_string())
                                    .unwrap_or_else(|| "—".into())
                            ));
                            ui.label(&r.state);
                            if let Some(path) = &r.path {
                                if ui.small_button("Add rule").clicked() {
                                    add = Some(path.clone());
                                }
                            } else {
                                ui.label("—");
                            }
                            ui.end_row();
                        }
                    });
                if entries.len() > 500 {
                    ui.label("Showing the first 500 matching endpoints. Refine the filter.");
                }
            });
        if let Some(path) = add {
            if let Some(index) = self
                .config
                .rules
                .iter()
                .position(|r| app_key(&r.path) == app_key(&path))
            {
                self.selected = index;
            } else if self.config.rules.len() < 256 {
                self.config.rules.push(Rule {
                    path,
                    ..Rule::default()
                });
                self.selected = self.config.rules.len() - 1;
                self.dirty = true;
            }
            self.page = 1;
        }
    }

    fn rules(&mut self, ui: &mut egui::Ui) {
        ui.heading("Rules & schedules");
        ui.label("Full executable paths identify applications. Caps are aggregate across their proxied TCP connections.");
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.config.rules.len() < 256,
                    egui::Button::new("Add application"),
                )
                .clicked()
            {
                self.config.rules.push(Rule::default());
                self.selected = self.config.rules.len() - 1;
                self.dirty = true;
            }
            if ui.button("Save changes").clicked() {
                self.save();
            }
            if self.dirty {
                ui.colored_label(Color32::YELLOW, "Unsaved changes");
            }
        });
        let before = serde_json::to_string(&self.config).unwrap_or_default();
        ui.add_space(10.0);
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                for (index, r) in self.config.rules.iter().enumerate() {
                    ui.selectable_value(
                        &mut self.selected,
                        index,
                        if r.path.is_empty() {
                            "New application"
                        } else {
                            name(&r.path)
                        },
                    );
                }
            });
        });
        let mut remove = false;
        if let Some(rule) = self.config.rules.get_mut(self.selected) {
            ui.add_space(10.0);
            ui.checkbox(
                &mut rule.enabled,
                "Rule enabled (disabled rules cannot use the proxy)",
            );
            ui.label("Executable path");
            ui.add(
                egui::TextEdit::singleline(&mut rule.path)
                    .desired_width(f32::INFINITY)
                    .hint_text(r"C:\Program Files\Example\Example.exe"),
            );
            ui.checkbox(&mut rule.call_background, "Background during calls");
            egui::Grid::new("rule-fields")
                .spacing([25.0, 12.0])
                .show(ui, |ui| {
                    ui.label("Download cap");
                    cap(ui, &mut rule.download_kib);
                    ui.end_row();
                    ui.label("Upload cap");
                    cap(ui, &mut rule.upload_kib);
                    ui.end_row();
                    ui.label("Daily quota (download + upload)");
                    ui.add(
                        egui::DragValue::new(&mut rule.daily_quota_mib)
                            .range(0..=u32::MAX)
                            .suffix(" MiB"),
                    );
                    ui.end_row();
                });
            ui.small("0 means unlimited. Reaching the daily quota closes the application's proxy connections.");
            let mut scheduled = rule.schedule.is_some();
            if ui
                .checkbox(&mut scheduled, "Apply base caps on a weekly schedule")
                .changed()
            {
                rule.schedule = scheduled.then_some(Schedule {
                    days: 31,
                    start_minute: 9 * 60,
                    end_minute: 18 * 60,
                });
            }
            if let Some(schedule) = &mut rule.schedule {
                ui.horizontal(|ui| {
                    for (day, label) in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
                        .iter()
                        .enumerate()
                    {
                        let mut enabled = schedule.days & (1 << day) != 0;
                        if ui.checkbox(&mut enabled, *label).changed() {
                            if enabled {
                                schedule.days |= 1 << day;
                            } else {
                                schedule.days &= !(1 << day);
                            }
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("From");
                    time(ui, &mut schedule.start_minute);
                    ui.label("to");
                    time(ui, &mut schedule.end_minute);
                    ui.label("local time");
                });
                ui.small("End is exclusive. An overnight window belongs to its starting weekday; equal times mean all day. Outside the schedule, base caps are unlimited. Call mode and quota remain active.");
            }
            ui.add_space(12.0);
            if ui.button("Remove rule").clicked() {
                remove = true;
            }
        } else {
            ui.label("Add an application or use Add rule in the connection inventory.");
        }
        if remove {
            self.config.rules.remove(self.selected);
            self.selected = self.selected.saturating_sub(1);
        }
        if serde_json::to_string(&self.config).unwrap_or_default() != before {
            self.dirty = true;
        }
    }

    fn settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Settings & updates");
        ui.label(format!("BandwidthLimiter {VERSION} • Windows x64 • MIT"));
        ui.add_space(10.0);
        let before = serde_json::to_string(&self.config).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label("Loopback SOCKS5 port");
            ui.add_enabled(
                !self.view.status.running,
                egui::DragValue::new(&mut self.config.proxy_port).range(1..=65535),
            );
        });
        ui.small(
            "Restart the proxy after saving a new port. No system proxy settings are changed.",
        );
        ui.horizontal(|ui| {
            ui.label("Call mode download cap");
            cap(ui, &mut self.config.call_download_kib);
        });
        ui.horizontal(|ui| {
            ui.label("Call mode upload cap");
            cap(ui, &mut self.config.call_upload_kib);
        });
        ui.checkbox(
            &mut self.config.startup_update_check,
            "Check for updates on startup",
        );
        if serde_json::to_string(&self.config).unwrap_or_default() != before {
            self.dirty = true;
        }
        if ui.button("Save settings").clicked() {
            self.save();
        }
        ui.label(format!("Data directory: {}", self.directory.display()));
        ui.separator();
        ui.strong("Updates");
        let state = self
            .runtime
            .update_state
            .try_lock()
            .map(|s| s.clone())
            .unwrap_or(State::Checking);
        let busy = matches!(state, State::Checking | State::Downloading { .. });
        if ui
            .add_enabled(!busy, egui::Button::new("Check for updates"))
            .clicked()
        {
            self.update_command(UpdateCommand::Check);
        }
        match state {
            State::Idle => {
                ui.label("No update check has run.");
            }
            State::Checking => {
                ui.spinner();
                ui.label("Checking the canonical GitHub repository…");
            }
            State::Current => {
                ui.label("This installation is up to date on the stable channel.");
            }
            State::Available(release) => {
                ui.colored_label(ACCENT, format!("Version {} is available", release.version));
                if ui.button("Download and install").clicked() {
                    self.update_command(UpdateCommand::Download(release));
                }
            }
            State::Downloading { received, total } => {
                ui.add(
                    egui::ProgressBar::new(received as f32 / total.max(1) as f32).show_percentage(),
                );
                ui.label(format!(
                    "Downloading and verifying: {received} / {total} bytes"
                ));
                ctx.request_repaint_after(Duration::from_millis(250));
            }
            State::Ready { staged, hash } => {
                ui.label("Download verified. Settings and usage remain in the data directory.");
                if ui.button("Restart to finish update").clicked() {
                    self.update_command(UpdateCommand::Install(staged, hash));
                }
            }
            State::Error(message) => {
                ui.colored_label(Color32::from_rgb(240, 188, 114), message);
            }
        }
        ui.hyperlink_to(
            "Canonical repository (publication pending)",
            "https://github.com/siarheisinelnikau/bandwidth-limiter",
        );
        ui.separator();
        ui.strong("Capabilities");
        ui.label("Available: cooperative SOCKS5 TCP upload / delivery pacing; per-application rules, quotas and call mode; read-only IPv4/IPv6 TCP/UDP inventory.");
        ui.label("Unavailable: transparent system-wide shaping, UDP forwarding, direct-traffic byte rates, driver/service backend, tray operation.");
        ui.label("A download cap paces payload delivery and TCP backpressure; kernel receive buffers can still receive bursts. This is not a cap at the network adapter.");
        ui.small("No driver, service, firewall filter, QoS rule or certificate is installed. GUI, hardware behavior and updater replacement require later manual validation.");
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        while let Ok(event) = self.runtime.events.try_recv() {
            match event {
                Event::Message(message) => self.message = message,
                Event::Saved(saved) => {
                    self.dirty = serde_json::to_string(&self.config).unwrap_or_default() != saved;
                    self.message =
                        "Settings saved and active for existing and new proxy flows.".into();
                }
                Event::Closed => {
                    self.can_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if let Ok(view) = self.runtime.view.try_lock() {
            self.view = view.clone();
        }
        if self.runtime.update_exit.load(Ordering::Acquire)
            && !self.closing
            && self.command(Command::Shutdown)
        {
            self.closing = true;
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.can_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if !self.closing && self.command(Command::Shutdown) {
                self.closing = true;
            }
        }
        egui::SidePanel::left("navigation").exact_width(190.0).show(ctx, |ui| {
            ui.add_space(15.0); ui.heading("Bandwidth\nLimiter"); ui.colored_label(ACCENT, "TCP POLICY WORKBENCH"); ui.add_space(25.0);
            for (index, label) in ["Traffic", "Rules & schedules", "Settings & updates"].iter().enumerate() { ui.selectable_value(&mut self.page, index, *label); }
            ui.add_space(30.0); ui.small("System-wide enforcement"); ui.colored_label(Color32::from_rgb(240, 188, 114), "UNAVAILABLE");
            ui.small("Use an application's SOCKS5 setting to route traffic through the explicit proxy.");
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(if self.closing {
                "Stopping connections and saving usage…"
            } else {
                &self.message
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_enabled_ui(!self.closing, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("page")
                    .show(ui, |ui| match self.page {
                        0 => self.traffic(ui),
                        1 => self.rules(ui),
                        _ => self.settings(ui, ctx),
                    });
            });
        });
        if matches!(
            *self.runtime.update_state.lock().unwrap(),
            State::Available(_)
        ) && self.page != 2
        {
            egui::TopBottomPanel::top("update-notice").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(ACCENT, "An update is available.");
                    if ui.button("Review update").clicked() {
                        self.page = 2;
                    }
                });
            });
        }
    }
}

fn name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}
fn cap(ui: &mut egui::Ui, value: &mut u32) {
    ui.add(
        egui::DragValue::new(value)
            .range(0..=1_000_000)
            .suffix(" KiB/s"),
    );
}
fn time(ui: &mut egui::Ui, value: &mut u16) {
    ui.add(
        egui::DragValue::new(value)
            .range(0..=1439)
            .custom_formatter(|v, _| format!("{:02}:{:02}", v as u16 / 60, v as u16 % 60))
            .custom_parser(|text| {
                let (h, m) = text.split_once(':')?;
                let h = h.parse::<u16>().ok()?;
                let m = m.parse::<u16>().ok()?;
                (h < 24 && m < 60).then_some(f64::from(h * 60 + m))
            }),
    );
}
