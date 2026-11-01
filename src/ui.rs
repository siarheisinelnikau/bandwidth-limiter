use crate::{
    design,
    runtime::{Command, Event, Runtime, UpdateCommand, View},
};
use bandwidth_limiter::{
    i18n::{Language, Message, text, tr},
    model::{Config, Rule, Schedule, Theme, Usage, VERSION, app_key},
    updater::State,
};
use egui::{self, RichText};
use std::{
    fs::File,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Applications,
    Connections,
    Settings,
    SystemTcp,
}
pub struct App {
    config: Config,
    runtime: Runtime,
    view: Arc<View>,
    pub page: Page,
    selected: usize,
    message: Message,
    dirty: bool,
    pub call_mode: bool,
    monitor: bool,
    pub closing: bool,
    pub can_close: bool,
    filter: String,
    directory: PathBuf,
    preview: bool,
    pub tray_available: bool,
    _guard: Option<File>,
}
impl App {
    pub fn new(
        config: Config,
        usage: Usage,
        directory: PathBuf,
        guard: File,
        ctx: egui::Context,
    ) -> Self {
        design::apply(&ctx, config.theme);
        let runtime = Runtime::new(config.clone(), usage, directory.clone(), ctx);
        if config.startup_update_check {
            let _ = runtime.update_commands.try_send(UpdateCommand::Check);
        }
        Self::from_runtime(config, runtime, directory, Some(guard), false)
    }
    fn from_runtime(
        config: Config,
        runtime: Runtime,
        directory: PathBuf,
        guard: Option<File>,
        preview: bool,
    ) -> Self {
        Self {
            config,
            runtime,
            view: Arc::new(View::default()),
            page: Page::Applications,
            selected: 0,
            message: Message::default(),
            dirty: false,
            call_mode: false,
            monitor: false,
            closing: false,
            can_close: false,
            filter: String::new(),
            directory,
            preview,
            tray_available: false,
            _guard: guard,
        }
    }
    pub fn fixture(config: Config, view: View, page: Page, error: bool) -> Self {
        let mut app = Self::from_runtime(
            config,
            Runtime::preview(),
            PathBuf::from("Isolated preview data"),
            None,
            true,
        );
        app.view = Arc::new(view);
        app.page = page;
        if error {
            app.message = "Settings could not be saved: the data directory is read-only. Your edits are still here.".into();
            app.dirty = true;
        }
        app
    }
    fn command(&mut self, command: Command) -> bool {
        if self.preview {
            return false;
        }
        if let Err(e) = self.runtime.commands.try_send(command) {
            self.message = Message::new(
                "The background task is busy. Try again shortly. ({e})",
                vec![e.to_string()],
            );
            false
        } else {
            true
        }
    }
    fn update_command(&mut self, command: UpdateCommand) {
        if !self.preview && self.runtime.update_commands.try_send(command).is_err() {
            self.message = "An update task is already running. Wait for it to finish.".into();
        }
    }
    pub fn poll(&mut self) {
        if self.preview {
            return;
        }
        while let Ok(event) = self.runtime.events.try_recv() {
            match event {
                Event::Message(message) => self.message = message,
                Event::Saved(saved) => {
                    self.dirty = serde_json::to_string(&self.config).unwrap_or_default() != saved;
                    self.message =
                        "Saved. The new limits are active for proxied connections.".into();
                }
                Event::Closed => self.can_close = true,
            }
        }
        if let Ok(view) = self.runtime.view.try_lock() {
            self.view = view.clone();
        }
        if self.runtime.update_exit.load(Ordering::Acquire) {
            self.request_quit();
        }
    }
    pub fn request_quit(&mut self) {
        if !self.closing && self.command(Command::Shutdown) {
            self.closing = true;
        }
    }
    pub fn language(&self) -> Language {
        self.config.language
    }
    pub fn running(&self) -> bool {
        self.view.status.running
    }
    pub fn toggle_call(&mut self) {
        if self.command(Command::Call(!self.call_mode)) {
            self.call_mode = !self.call_mode;
        }
    }
    pub fn toggle_proxy(&mut self) {
        if self.running() {
            self.command(Command::Stop);
        } else if self.dirty {
            self.message = "Save your changes before starting the limiter.".into();
        } else if !self.config.rules.iter().any(|r| r.enabled) {
            self.message = "Add and save an application first.".into();
        } else {
            self.command(Command::Start(self.config.proxy_port));
        }
    }
    fn save(&mut self) {
        match self.config.validate() {
            Ok(()) => {
                self.command(Command::Save(self.config.clone()));
            }
            Err(e) => self.message = e.into(),
        }
    }
    fn add(&mut self, path: String) {
        if let Some(i) = self
            .config
            .rules
            .iter()
            .position(|r| app_key(&r.path) == app_key(&path))
        {
            self.selected = i;
        } else if self.config.rules.len() < 256 {
            self.config.rules.push(Rule {
                path,
                ..Rule::default()
            });
            self.selected = self.config.rules.len() - 1;
            self.dirty = true;
        }
        self.page = Page::Applications;
    }
    pub fn draw(&mut self, ctx: &egui::Context) {
        let palette = design::palette(self.config.theme);
        let language = self.config.language;
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(palette.surface)
                    .inner_margin(egui::Margin::symmetric(22, 16)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("BandwidthLimiter");
                    if self.preview {
                        ui.colored_label(palette.warning, tr(language, "PREVIEW DATA"));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(tr(language, "Quit")).clicked() {
                            self.request_quit();
                        }
                        ui.colored_label(
                            if self.running() {
                                palette.accent
                            } else {
                                palette.muted
                            },
                            if self.running() {
                                tr(language, "Proxy running")
                            } else {
                                tr(language, "Proxy paused")
                            },
                        );
                    });
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    for (page, label) in [
                        (Page::Applications, tr(language, "Applications")),
                        (Page::Connections, tr(language, "Connections")),
                        (Page::SystemTcp, tr(language, "System TCP (WinDivert)")),
                        (Page::Settings, tr(language, "Settings")),
                    ] {
                        ui.selectable_value(&mut self.page, page, label);
                    }
                });
            });
        egui::TopBottomPanel::bottom("footer")
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(22, 10)))
            .show(ctx, |ui| {
                if self.closing {
                    ui.label(tr(language, "Stopping connections and saving usage…"));
                } else {
                    design::note(
                        ui,
                        if self.tray_available {
                            tr(language, "Closing hides this window. Quit stops the proxy; the WinDivert service is independent.")
                        } else {
                            tr(language, "Proxy controls affect SOCKS5 traffic. System TCP has separate WinDivert controls.")
                        },
                    );
                }
                if !self.message.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(self.message.render(language));
                        if ui.small_button(tr(language, "Dismiss")).clicked() {
                            self.message.clear();
                        }
                    });
                }
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(palette.background).inner_margin(22))
            .show(ctx, |ui| {
                if self.dirty {
                    ui.horizontal(|ui| {
                        ui.colored_label(palette.warning, tr(language, "Unsaved changes"));
                        if ui.button(tr(language, "Save changes")).clicked() {
                            self.save();
                        }
                    });
                    ui.add_space(8.0);
                }
                let update_available = self
                    .runtime
                    .update_state
                    .try_lock()
                    .is_ok_and(|s| matches!(*s, State::Available(_)));
                if update_available
                    && self.page != Page::Settings
                    && ui
                        .button(tr(language, "An update is available · Review in Settings"))
                        .clicked()
                {
                    self.page = Page::Settings;
                }
                ui.add_enabled_ui(!self.closing, |ui| match self.page {
                    Page::Applications => self.applications(ui),
                    Page::Connections => self.connections(ui),
                    Page::Settings => self.settings(ui),
                    Page::SystemTcp => self.system_tcp(ui),
                });
            });
    }
    fn applications(&mut self, ui: &mut egui::Ui) {
        let palette = design::palette(self.config.theme);
        let language = self.config.language;
        ui.horizontal(|ui| {
            design::section(ui, tr(language, "Application limits"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        self.running()
                            || (self.config.rules.iter().any(|r| r.enabled) && !self.dirty),
                        egui::Button::new(
                            RichText::new(if self.running() {
                                tr(language, "Pause proxy")
                            } else {
                                tr(language, "Start proxy")
                            })
                            .color(if self.running() {
                                palette.text
                            } else {
                                palette.on_accent
                            }),
                        )
                        .fill(if self.running() {
                            palette.surface
                        } else {
                            palette.accent
                        }),
                    )
                    .clicked()
                {
                    self.toggle_proxy();
                }
            });
        });
        design::note(
            ui,
            tr(
                language,
                "Choose an application, set its limits, then connect it to the local proxy.",
            ),
        );
        ui.add_space(10.0);
        egui::Frame::new().fill(palette.surface).inner_margin(14).corner_radius(8).show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut call = self.call_mode; if ui.checkbox(&mut call, RichText::new(tr(language, "Call mode")).strong()).changed() { self.toggle_call(); }
                design::note(ui, text(language, "Background apps: {} KiB/s down · {} KiB/s up", &[(self.config.call_download_kib).to_string(), (self.config.call_upload_kib).to_string()]));
            });
            design::note(ui, tr(language, "Lower limits for apps marked “Background during calls”. Stricter app limits stay in place."));
        });
        ui.add_space(10.0);
        ui.collapsing(tr(language, "Connection setup · SOCKS5 only"), |ui| {
            ui.label(text(language, "In the app's proxy settings, choose SOCKS5 at 127.0.0.1:{} with no authentication.", &[(self.config.proxy_port).to_string()]));
            ui.label(tr(language, "Proxy controls leave direct connections and UDP unaffected. Use System TCP (WinDivert) for the separate experimental service."));
        });
        ui.add_space(12.0);
        if self.config.rules.is_empty() {
            ui.add_space(35.0);
            design::section(ui, tr(language, "Add your first application"));
            ui.label(tr(
                language,
                "Add a downloader, game launcher or sync application by its full executable path.",
            ));
            ui.add_space(10.0);
            if ui.button(tr(language, "Add application")).clicked() {
                self.add(String::new());
            }
            if ui
                .button(tr(language, "Choose from current connections"))
                .clicked()
            {
                self.page = Page::Connections;
                self.command(Command::Refresh);
            }
            return;
        }
        let width = ui.available_width();
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2((width * 0.38).max(270.0), ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.horizontal(|ui| {
                        design::section(ui, tr(language, "Applications"));
                        if ui.small_button(tr(language, "+ Add")).clicked() {
                            self.add(String::new());
                        }
                    });
                    egui::ScrollArea::vertical()
                        .id_salt("app-list")
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                        )
                        .show(ui, |ui| {
                            for (index, rule) in self.config.rules.iter().enumerate() {
                                let rates = self
                                    .view
                                    .rates
                                    .get(&app_key(&rule.path))
                                    .copied()
                                    .unwrap_or([0.0; 2]);
                                let title = if rule.path.is_empty() {
                                    tr(language, "New application")
                                } else {
                                    name(&rule.path)
                                };
                                let subtitle = if rule.enabled {
                                    text(
                                        language,
                                        "Down {:.1} · Up {:.1} KiB/s",
                                        &[
                                            format!("{:.1}", rates[0] / 1024.0),
                                            format!("{:.1}", rates[1] / 1024.0),
                                        ],
                                    )
                                } else {
                                    tr(language, "Rule disabled").into()
                                };
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 60.0],
                                        egui::Button::new(format!("{title}\n{subtitle}"))
                                            .selected(self.selected == index),
                                    )
                                    .clicked()
                                {
                                    self.selected = index;
                                }
                            }
                        });
                },
            );
            ui.separator();
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("editor")
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                        )
                        .show(ui, |ui| self.editor(ui));
                },
            );
        });
    }
    fn editor(&mut self, ui: &mut egui::Ui) {
        let palette = design::palette(self.config.theme);
        let language = self.config.language;
        let Some(rule) = self.config.rules.get_mut(self.selected) else {
            return;
        };
        let mut changed = false;
        design::section(
            ui,
            if rule.path.is_empty() {
                tr(language, "New application")
            } else {
                name(&rule.path)
            },
        );
        ui.label(tr(language, "Executable path"));
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut rule.path)
                    .desired_width(f32::INFINITY)
                    .hint_text(r"C:\Program Files\App\App.exe"),
            )
            .changed();
        changed |= ui
            .checkbox(&mut rule.enabled, tr(language, "Rule enabled"))
            .changed();
        ui.add_space(6.0);
        egui::Grid::new("limits")
            .num_columns(2)
            .spacing([18.0, 12.0])
            .show(ui, |ui| {
                ui.label(tr(language, "Download limit"));
                changed |= cap(ui, &mut rule.download_kib);
                ui.end_row();
                ui.label(tr(language, "Upload limit"));
                changed |= cap(ui, &mut rule.upload_kib);
                ui.end_row();
            });
        design::note(ui, tr(language, "0 KiB/s means unlimited."));
        changed |= ui
            .checkbox(
                &mut rule.call_background,
                tr(language, "Background during calls"),
            )
            .changed();
        let summary = text(
            language,
            "Quota & schedule · {}",
            &[(if rule.daily_quota_mib == 0 {
                tr(language, "no quota").into()
            } else {
                text(
                    language,
                    "{} MiB daily",
                    &[(rule.daily_quota_mib).to_string()],
                )
            })
            .to_string()],
        );
        ui.collapsing(summary, |ui| {
            ui.horizontal(|ui| {
                ui.label(tr(language, "Daily quota"));
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut rule.daily_quota_mib)
                            .range(0..=u32::MAX)
                            .suffix(" MiB"),
                    )
                    .changed();
            });
        design::note(
            ui,
            tr(language, "0 = unlimited. The daily quota includes downloads and uploads."),
        );
        let mut scheduled = rule.schedule.is_some();
        if ui
            .checkbox(&mut scheduled, tr(language, "Use a weekly schedule"))
            .changed()
        {
            rule.schedule = scheduled.then_some(Schedule {
                days: 31,
                start_minute: 540,
                end_minute: 1080,
            });
            changed = true;
        }
        if let Some(schedule) = &mut rule.schedule {
            ui.horizontal_wrapped(|ui| {
                for (day, label) in [tr(language, "Mon"), tr(language, "Tue"), tr(language, "Wed"), tr(language, "Thu"), tr(language, "Fri"), tr(language, "Sat"), tr(language, "Sun")]
                    .iter()
                    .enumerate()
                {
                    let mut on = schedule.days & (1 << day) != 0;
                    if ui.checkbox(&mut on, *label).changed() {
                        if on {
                            schedule.days |= 1 << day;
                        } else {
                            schedule.days &= !(1 << day);
                        }
                        changed = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(tr(language, "From"));
                changed |= time(ui, &mut schedule.start_minute);
                ui.label(tr(language, "to"));
                changed |= time(ui, &mut schedule.end_minute);
                ui.label(tr(language, "local time"));
            });
            design::note(
                ui,
                tr(language, "Outside this window, base limits are unlimited. Call mode and quota still apply."),
            );
            ui.collapsing(tr(language, "Overnight and all-day schedules"), |ui| { ui.label(tr(language, "Select the starting day for an overnight window. Equal start and end times mean all day. End times are exclusive.")); });
        }
        });
        ui.add_space(6.0);
        let usage = self
            .view
            .usage
            .apps
            .get(&app_key(&rule.path))
            .copied()
            .unwrap_or([0; 2]);
        design::note(
            ui,
            text(
                language,
                "Daily usage: {:.2} MiB · {}",
                &[
                    format!(
                        "{:.2}",
                        usage[0].saturating_add(usage[1]) as f64 / 1048576.0
                    ),
                    (self.view.usage.day).to_string(),
                ],
            ),
        );
        self.dirty |= changed;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new(tr(language, "Remove rule")).color(palette.error))
                .clicked()
            {
                self.config.rules.remove(self.selected);
                self.selected = self.selected.saturating_sub(1);
                self.dirty = true;
            }
        });
    }
    fn connections(&mut self, ui: &mut egui::Ui) {
        let language = self.config.language;
        design::section(
            ui,
            tr(language, "Choose an application from its connections"),
        );
        design::note(
            ui,
            tr(
                language,
                "Read-only Windows endpoints. Direct-traffic download and upload rates are unknown.",
            ),
        );
        ui.horizontal(|ui| {
            if ui.button(tr(language, "Refresh connections")).clicked() {
                self.command(Command::Refresh);
            }
            let mut monitor = self.monitor;
            if ui
                .checkbox(&mut monitor, tr(language, "Refresh every second"))
                .changed()
                && self.command(Command::Monitor(monitor))
            {
                self.monitor = monitor;
            }
        });
        ui.horizontal(|ui| {
            ui.label(tr(language, "Find an app or PID"));
            ui.text_edit_singleline(&mut self.filter);
        });
        design::note(ui, self.view.connection_note.render(language));
        let filter = self.filter.to_lowercase();
        let mut add = None;
        egui::ScrollArea::both()
            .id_salt("connection-table")
            .show(ui, |ui| {
                egui::Grid::new("endpoints")
                    .striped(true)
                    .spacing([18.0, 10.0])
                    .show(ui, |ui| {
                        for label in [
                            tr(language, "Application"),
                            tr(language, "Protocol"),
                            tr(language, "State"),
                            "",
                        ] {
                            ui.strong(label);
                        }
                        ui.end_row();
                        for r in self
                            .view
                            .connections
                            .iter()
                            .filter(|r| {
                                filter.is_empty()
                                    || r.pid.to_string().contains(&filter)
                                    || r.path
                                        .as_deref()
                                        .unwrap_or("unknown")
                                        .to_lowercase()
                                        .contains(&filter)
                            })
                            .take(500)
                        {
                            ui.label(format!(
                                "{} · {}",
                                r.path
                                    .as_deref()
                                    .map(name)
                                    .unwrap_or(tr(language, "Unknown / protected")),
                                r.pid
                            ))
                            .on_hover_text(text(
                                language,
                                "{}\nLocal: {}\nRemote: {}",
                                &[
                                    (r.path
                                        .as_deref()
                                        .unwrap_or(tr(language, "Executable unavailable")))
                                    .to_string(),
                                    (r.local).to_string(),
                                    (r.remote.map_or("—".into(), |a| a.to_string())).to_string(),
                                ],
                            ));
                            ui.label(r.protocol);
                            ui.label(tr(language, &r.state));
                            if let Some(path) = &r.path {
                                if ui.small_button(tr(language, "Add rule")).clicked() {
                                    add = Some(path.clone());
                                }
                            } else {
                                ui.label("—");
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(path) = add {
            self.add(path);
        }
    }
    fn settings(&mut self, ui: &mut egui::Ui) {
        let palette = design::palette(self.config.theme);
        let language = self.config.language;
        egui::ScrollArea::vertical().id_salt("settings").show(ui, |ui| {
            design::section(ui, tr(language, "Appearance"));
            let previous_language = self.config.language;
            let previous_theme = self.config.theme;
            ui.horizontal_wrapped(|ui| {
                ui.label(tr(language, "Language"));
                egui::ComboBox::from_id_salt("language-choice").selected_text(self.config.language.name()).show_ui(ui, |ui| {
                    for choice in Language::ALL { ui.selectable_value(&mut self.config.language, choice, choice.name()); }
                });
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(tr(language, "Theme"));
                ui.selectable_value(&mut self.config.theme, Theme::Light, tr(language, "Light"));
                ui.selectable_value(&mut self.config.theme, Theme::Dark, tr(language, "Dark"));
            });
            design::note(ui, tr(language, "Language and theme apply immediately. Save changes to keep them for the next launch."));
            if previous_language != self.config.language || previous_theme != self.config.theme {
                self.dirty = true;
                design::apply(ui.ctx(), self.config.theme);
                ui.ctx().request_repaint();
            }
            ui.add_space(12.0); ui.separator();

            design::section(ui, tr(language, "Call mode")); ui.label(tr(language, "Limits for each application marked as background during calls."));
            egui::Grid::new("call-limits").num_columns(2).show(ui, |ui| { ui.label(tr(language, "Download")); self.dirty |= cap(ui, &mut self.config.call_download_kib); ui.end_row(); ui.label(tr(language, "Upload")); self.dirty |= cap(ui, &mut self.config.call_upload_kib); ui.end_row(); });
            ui.add_space(12.0); ui.separator(); ui.add_space(6.0); design::section(ui, tr(language, "Updates")); design::note(ui, text(language, "Installed version {VERSION} · Windows x64", &[(VERSION).to_string()]));
            self.dirty |= ui.checkbox(&mut self.config.startup_update_check, tr(language, "Check for updates on startup")).changed();
            let state = self.runtime.update_state.try_lock().map(|s| s.clone()).unwrap_or(State::Checking);
            if ui.add_enabled(!matches!(state, State::Checking | State::Downloading { .. }), egui::Button::new(tr(language, "Check for updates"))).clicked() { self.update_command(UpdateCommand::Check); }
            match state {
                State::Idle => design::note(ui, tr(language, "No check has run yet.")), State::Checking => { ui.label(tr(language, "Checking for updates…")); }, State::Current => { ui.label(tr(language, "You're up to date.")); },
                State::Available(release) => { ui.colored_label(palette.accent, text(language, "Version {} is available", &[(release.version).to_string()])); if ui.button(tr(language, "Download and install")).clicked() { self.update_command(UpdateCommand::Download(release)); } },
                State::Downloading { received, total } => { ui.add(egui::ProgressBar::new(received as f32 / total.max(1) as f32).show_percentage()); design::note(ui, tr(language, "Downloading and verifying. Your installed copy stays in place until restart.")); },
                State::Ready => { ui.label(tr(language, "Verified and ready. Settings and usage will be preserved.")); if self.dirty { ui.colored_label(palette.warning, tr(language, "Save your changes before restarting for the update.")); }
                    if ui.add_enabled(!self.dirty, egui::Button::new(tr(language, "Restart to finish update"))).clicked() { self.update_command(UpdateCommand::Install); } },
                State::Error(error) => { ui.colored_label(palette.warning, text(language, "Update failed: {}", &[tr(language, &error).to_string()])); },
            }
            ui.add_space(12.0); ui.separator();
            ui.collapsing(tr(language, "Connection and storage"), |ui| {
                ui.horizontal(|ui| { ui.label(tr(language, "SOCKS5 port")); let enabled = !self.running(); self.dirty |= ui.add_enabled(enabled, egui::DragValue::new(&mut self.config.proxy_port).range(1..=65535)).changed(); });
                ui.label(tr(language, "Stop the limiter before changing the port, then save and restart it.")); design::note(ui, text(language, "Settings and usage: {}", &[(self.directory.display()).to_string()]));
            });
            if ui.button(tr(language, "System TCP (WinDivert) · service controls")).clicked() { self.page = Page::SystemTcp; }
            ui.collapsing(tr(language, "About and limitations"), |ui| { ui.label(tr(language, "CPU-rendered interface · MIT license")); ui.label(tr(language, "Proxy rates measure forwarded TCP payloads. Incoming network buffers may still receive bursts. Quotas are checkpointed, so forced exit can lose recent usage.")); ui.hyperlink_to(tr(language, "Project repository"), "https://github.com/siarheisinelnikau/bandwidth-limiter"); });
        });
    }
}
impl App {
    fn system_tcp_command(&mut self, command: bandwidth_limiter::traffic::Command) {
        if self.preview {
            return;
        }
        let Ok(mut state) = self.runtime.system_tcp_state.try_lock() else {
            return;
        };
        if state.busy {
            return;
        }
        if self.runtime.system_tcp_commands.try_send(command).is_err() {
            self.message = "A WinDivert service request is already pending.".into();
        } else {
            state.busy = true;
        }
    }
    pub fn system_tcp_fixture(&mut self, connected: bool) {
        let language = self.config.language;
        let mut state = self.runtime.system_tcp_state.lock().unwrap();
        if connected {
            state.response = Some(bandwidth_limiter_packet_control::Response {
                version: bandwidth_limiter_packet_control::VERSION,
                backend: bandwidth_limiter_packet_control::BACKEND.into(),
                can_configure: true,
                status: Some(bandwidth_limiter_packet_control::Snapshot {
                    active: true,
                    flows: 3,
                    upload_submitted: 65536,
                    download_submitted: 131072,
                    ..Default::default()
                }),
                error: None,
            });
        } else {
            state.error = Some(tr(language, "The WinDivert service is unavailable. Install/start reviewed components separately; this app does not install or elevate.").into());
        }
    }
    fn system_tcp(&mut self, ui: &mut egui::Ui) {
        let palette = design::palette(self.config.theme);
        let language = self.config.language;
        use bandwidth_limiter::traffic::Command as SystemCommand;
        let state = self
            .runtime
            .system_tcp_state
            .try_lock()
            .map(|s| s.clone())
            .unwrap_or_default();
        egui::ScrollArea::vertical().id_salt("system-tcp-controls").show(ui, |ui| {
            design::section(ui, tr(language, "System TCP limits · experimental"));
            ui.label(tr(language, "Limit selected TCP applications through WinDivert, without configuring a proxy in each app."));
            ui.colored_label(palette.warning, tr(language, "Requires a separately prepared service and signed WinDivert 2.2 runtime. Live traffic validation is pending."));
            design::note(ui, tr(language, "Check and Apply are manual. This app never installs, starts or elevates the service or driver."));
            ui.add_space(8.0);
            if ui.add_enabled(!state.busy, egui::Button::new(tr(language, "Check service"))).clicked() { self.system_tcp_command(SystemCommand::Refresh); }
            if state.busy { ui.label(tr(language, "Waiting for the service…")); }
            if let Some(error) = &state.error { ui.colored_label(palette.warning, text(language, "Service error: {}", &[tr(language, error).to_string()])); }
            if let Some(response) = &state.response {
                if let Some(status) = &response.status {
                    ui.colored_label(if status.active { palette.accent } else { palette.muted }, if status.active { tr(language, "Last checked: System TCP limits active") } else { tr(language, "Last checked: System TCP paused") });
                    ui.label(text(language, "{} tracked connections · {} queued packets · {:.1} KiB buffered", &[(status.flows).to_string(), (status.queued_packets).to_string(), format!("{:.1}", status.queued_bytes as f64 / 1024.0)]));
                    ui.label(text(language, "Reinjected TCP payload: {:.1} KiB download · {:.1} KiB upload", &[format!("{:.1}", status.download_submitted as f64 / 1024.0), format!("{:.1}", status.upload_submitted as f64 / 1024.0)]));
                    design::note(ui, text(language, "{} unmatched/unknown packets passed · {} dropped · {} send errors", &[(status.unattributed_packets).to_string(), (status.dropped_packets).to_string(), (status.send_errors).to_string()]));
                    design::note(ui, tr(language, "Manual snapshot. Counters include retransmissions and reset on Apply; they are not wire rates or delivered-byte totals."));
                    if let Some(error) = &status.last_error { ui.colored_label(palette.warning, tr(language, error)); }
                }
                if !response.can_configure { ui.label(tr(language, "Read-only connection. Run an administrator-authorized client to apply or pause System TCP limits.")); }
            }
            ui.add_space(10.0); ui.separator(); design::section(ui, tr(language, "Apply saved application limits"));
            ui.label(tr(language, "Uses enabled application paths and download/upload caps from Applications. At most 32 rules; 0 in one direction means unlimited."));
            ui.label(tr(language, "TCP only. Enabled quotas/schedules and active Call mode are rejected together. UDP stays unaffected."));
            if self.dirty { ui.colored_label(palette.warning, tr(language, "Save your application changes first.")); }
            let permitted = !state.busy && !self.dirty && state.response.as_ref().is_some_and(|r| r.can_configure);
            let eligible = bandwidth_limiter::traffic::policy(&self.config, self.call_mode);
            if let Err(error) = &eligible { design::note(ui, tr(language, error)); }
            if ui.add_enabled(permitted && eligible.is_ok(), egui::Button::new(tr(language, "Apply saved TCP limits"))).clicked() { self.system_tcp_command(SystemCommand::Apply(self.config.clone(), self.call_mode)); }
            if ui.add_enabled(!state.busy && state.response.as_ref().is_some_and(|r| r.can_configure), egui::Button::new(tr(language, "Pause System TCP"))).clicked() { self.system_tcp_command(SystemCommand::Pause); }
            design::note(ui, tr(language, "Reconnect applications after Apply. Only newly observed, identified connections are limited; unmatched traffic passes through."));
            design::note(ui, tr(language, "Pause/reapply release queued traffic and may cause a short burst or reordering. The service restarts paused; Apply again afterward."));
            ui.add_space(8.0); ui.label(tr(language, "Quitting this GUI stops the proxy. System TCP continues independently until explicitly paused or its service stops."));
        });
    }
}
fn name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}
fn cap(ui: &mut egui::Ui, value: &mut u32) -> bool {
    ui.add(
        egui::DragValue::new(value)
            .range(0..=1_000_000)
            .suffix(" KiB/s"),
    )
    .changed()
}
fn time(ui: &mut egui::Ui, value: &mut u16) -> bool {
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
    )
    .changed()
}
