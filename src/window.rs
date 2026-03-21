use crate::{
    paint::Paint,
    scheduling::Scheduling,
    ui::{App, Page},
};
use bandwidth_limiter::model::{Config, Usage};
use std::{
    fs::File,
    num::NonZeroU32,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::{Window, WindowId},
};

#[derive(Debug)]
pub enum Wake {
    Repaint { delay: Duration, pass: u64 },
    Menu(String),
}
pub fn run(
    bootstrap: Result<(File, Config, Usage), String>,
    directory: PathBuf,
    diagnostics: Option<PathBuf>,
    cleanup: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let events = EventLoop::<Wake>::with_user_event().build()?;
    events.set_control_flow(ControlFlow::Wait);
    let mut host = Host {
        proxy: events.create_proxy(),
        bootstrap: Some(bootstrap),
        directory,
        window: None,
        surface: None,
        input: None,
        ctx: egui::Context::default(),
        app: None,
        startup_error: None,
        painter: Paint::new(),
        scheduling: Scheduling::new(),
        minimized: false,
        tray: None,
        shown: false,
        layout_count: 0,
        present_count: 0,
        instance: None,
        diagnostics,
        cleanup,
        segments: vec![],
        segment_start: Instant::now(),
        segment_layout: 0,
        segment_present: 0,
        layout_ns: 0,
        raster_ns: 0,
        present_ns: 0,
        hidden_redraws: 0,
    };
    events.run_app(&mut host)?;
    Ok(())
}
struct Host {
    proxy: EventLoopProxy<Wake>,
    bootstrap: Option<Result<(File, Config, Usage), String>>,
    directory: PathBuf,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    input: Option<egui_winit::State>,
    ctx: egui::Context,
    app: Option<App>,
    startup_error: Option<String>,
    painter: Paint,
    scheduling: Scheduling,
    minimized: bool,
    tray: Option<crate::tray::Tray>,
    shown: bool,
    layout_count: u64,
    present_count: u64,
    instance: Option<crate::instance::Instance>,
    diagnostics: Option<PathBuf>,
    cleanup: Option<PathBuf>,
    segments: Vec<serde_json::Value>,
    segment_start: Instant,
    segment_layout: u64,
    segment_present: u64,
    layout_ns: u128,
    raster_ns: u128,
    present_ns: u128,
    hidden_redraws: u64,
}
impl Host {
    fn record_segment(&mut self) {
        if self.diagnostics.is_some() && self.segments.len() < 64 {
            self.segments.push(serde_json::json!({ "visible": self.scheduling.visible,
                "duration_ms": self.segment_start.elapsed().as_millis(), "layouts": self.layout_count - self.segment_layout,
                "presents": self.present_count - self.segment_present }));
        }
        self.segment_start = Instant::now();
        self.segment_layout = self.layout_count;
        self.segment_present = self.present_count;
    }
    fn visibility(&mut self, visible: bool) {
        if visible != self.scheduling.visible {
            self.record_segment();
        }
        self.scheduling.visibility(visible);
    }
    fn poll(&mut self, e: &ActiveEventLoop) {
        if let Some(app) = &mut self.app {
            app.poll();
            if let Some(tray) = &mut self.tray {
                tray.sync(app.running(), app.call_mode);
            }
            if app.can_close {
                e.exit();
            }
        }
    }
    fn restore(&mut self) {
        if let Some(window) = &self.window {
            window.set_visible(true);
            window.set_minimized(false);
            window.focus_window();
            self.minimized = false;
            self.visibility(true);
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
    fn paint(&mut self, e: &ActiveEventLoop) -> Result<(), Box<dyn std::error::Error>> {
        self.poll(e);
        if !self.scheduling.visible || self.minimized {
            self.hidden_redraws += 1;
            return Ok(());
        }
        let Some(window) = &self.window else {
            return Ok(());
        };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return Ok(());
        };
        let input = self.input.as_mut().ok_or("Input adapter missing")?;
        if self.scheduling.dirty
            || self.painter.pixels.len() != size.width as usize * size.height as usize
        {
            self.scheduling.dirty = false;
            self.scheduling.deadline = None;
            let layout_start = Instant::now();
            let output = self.ctx.run(input.take_egui_input(window), |ctx| {
                if let Some(app) = &mut self.app { app.draw(ctx); }
                else { egui::CentralPanel::default().show(ctx, |ui| { ui.heading("Could not open BandwidthLimiter"); ui.label(self.startup_error.as_deref().unwrap_or("Unknown startup error")); ui.label("Existing settings have been preserved. Close another instance or inspect the data directory."); }); }
            });
            self.layout_ns += layout_start.elapsed().as_nanos();
            input.handle_platform_output(window, output.platform_output.clone());
            let raster_start = Instant::now();
            self.painter
                .raster(&self.ctx, &output, size.width, size.height);
            self.raster_ns += raster_start.elapsed().as_nanos();
            self.layout_count += 1;
            if let Some(viewport) = output.viewport_output.get(&egui::ViewportId::ROOT) {
                self.scheduling.wake(Instant::now(), viewport.repaint_delay);
            }
        }
        let present_start = Instant::now();
        let surface = self
            .surface
            .as_mut()
            .ok_or("Presentation surface missing")?;
        surface.resize(w, h)?;
        let mut buffer = surface.buffer_mut()?;
        for (target, pixel) in buffer.iter_mut().zip(&self.painter.pixels) {
            *target =
                (u32::from(pixel[0]) << 16) | (u32::from(pixel[1]) << 8) | u32::from(pixel[2]);
        }
        buffer.present()?;
        self.present_ns += present_start.elapsed().as_nanos();
        self.present_count += 1;
        if !self.shown {
            window.set_visible(true);
            self.shown = true;
        }
        Ok(())
    }
}
impl ApplicationHandler<Wake> for Host {
    fn resumed(&mut self, e: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let result: Result<(), Box<dyn std::error::Error>> = (|| {
            let window = Rc::new(
                e.create_window(
                    Window::default_attributes()
                        .with_title("BandwidthLimiter")
                        .with_window_icon(
                            winit::window::Icon::from_rgba(crate::icons::pixels(32), 32, 32).ok(),
                        )
                        .with_inner_size(winit::dpi::LogicalSize::new(980.0, 720.0))
                        .with_min_inner_size(winit::dpi::LogicalSize::new(840.0, 620.0))
                        .with_visible(false),
                )?,
            );
            let context = softbuffer::Context::new(window.clone())?;
            self.surface = Some(softbuffer::Surface::new(&context, window.clone())?);
            self.input = Some(egui_winit::State::new(
                self.ctx.clone(),
                egui::ViewportId::ROOT,
                window.as_ref(),
                Some(window.scale_factor() as f32),
                None,
                None,
            ));
            let proxy = self.proxy.clone();
            self.ctx.set_request_repaint_callback(move |info| {
                if info.delay != Duration::MAX {
                    let _ = proxy.send_event(Wake::Repaint {
                        delay: info.delay,
                        pass: info.current_cumulative_pass_nr,
                    });
                }
            });
            crate::design::apply(&self.ctx);
            match self.bootstrap.take().ok_or("Startup state missing")? {
                Ok((guard, config, usage)) => {
                    self.instance =
                        crate::instance::Instance::new(&self.directory, self.proxy.clone()).ok();
                    self.app = Some(App::new(
                        config,
                        usage,
                        self.directory.clone(),
                        guard,
                        self.ctx.clone(),
                    ))
                }
                Err(error) => self.startup_error = Some(error),
            }
            if self.app.is_some()
                && let Some(directory) = self.cleanup.take()
                && let Ok(config) = bandwidth_limiter::updater::config()
            {
                rust_auto_updator::cleanup_after_update(directory, config.install);
            }
            self.tray = crate::tray::Tray::new(self.proxy.clone()).ok();
            if let Some(app) = &mut self.app {
                app.tray_available = self.tray.is_some();
            }
            window.request_redraw();
            self.window = Some(window);
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("CPU window initialization failed: {error}");
            e.exit();
        }
    }
    fn user_event(&mut self, e: &ActiveEventLoop, event: Wake) {
        self.poll(e);
        match event {
            Wake::Repaint { delay, pass } => {
                self.scheduling.repaint_callback(
                    Instant::now(),
                    delay,
                    pass,
                    self.ctx.cumulative_pass_nr(),
                );
            }
            Wake::Menu(id) => {
                match id.as_str() {
                    "open" => self.restore(),
                    "settings" => {
                        if let Some(app) = &mut self.app {
                            app.page = Page::Settings;
                        }
                        self.restore();
                    }
                    "call" => {
                        if let Some(app) = &mut self.app {
                            app.toggle_call();
                        }
                    }
                    "proxy" => {
                        if let Some(app) = &mut self.app {
                            app.toggle_proxy();
                        }
                    }
                    "quit" => {
                        if let Some(app) = &mut self.app {
                            app.request_quit();
                        } else {
                            e.exit();
                        }
                    }
                    _ => {}
                }
                self.scheduling.invalidate();
                self.poll(e);
            }
        }
    }
    fn window_event(&mut self, e: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        self.poll(e);
        match event {
            WindowEvent::CloseRequested => {
                if self.tray.is_some() && self.app.as_ref().is_some_and(|a| !a.closing) {
                    if let Some(w) = &self.window {
                        w.set_visible(false);
                    }
                    self.visibility(false);
                } else if let Some(app) = &mut self.app {
                    app.request_quit();
                } else {
                    e.exit();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.paint(e) {
                    eprintln!("CPU presentation failed: {error}");
                    e.exit();
                }
            }
            other => {
                if let WindowEvent::Resized(size) = &other {
                    self.minimized = size.width == 0
                        || size.height == 0
                        || self
                            .window
                            .as_ref()
                            .is_some_and(|w| w.is_minimized() == Some(true));
                    self.visibility(
                        !self.minimized
                            && (!self.shown
                                || self
                                    .window
                                    .as_ref()
                                    .is_some_and(|w| w.is_visible() == Some(true))),
                    );
                }
                if let WindowEvent::Occluded(occluded) = &other {
                    self.visibility(
                        !occluded
                            && !self.minimized
                            && (!self.shown
                                || self
                                    .window
                                    .as_ref()
                                    .is_some_and(|w| w.is_visible() == Some(true))),
                    );
                }
                if let (Some(input), Some(window)) = (&mut self.input, &self.window)
                    && input.on_window_event(window, &other).repaint
                {
                    self.scheduling.invalidate();
                }
            }
        }
    }
    fn about_to_wait(&mut self, e: &ActiveEventLoop) {
        self.poll(e);
        if self.scheduling.due(Instant::now())
            && let Some(w) = &self.window
        {
            w.request_redraw();
        }
        e.set_control_flow(if self.scheduling.visible {
            self.scheduling
                .deadline
                .map_or(ControlFlow::Wait, ControlFlow::WaitUntil)
        } else {
            ControlFlow::Wait
        });
    }
    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.record_segment();
        if let Some(path) = &self.diagnostics {
            let report = serde_json::json!({ "backend": "egui CPU rasterizer + softbuffer Win32 GDI", "layouts": self.layout_count,
                "presents": self.present_count, "layout_ms": self.layout_ns as f64 / 1e6,
                "raster_ms": self.raster_ns as f64 / 1e6, "present_ms": self.present_ns as f64 / 1e6,
                "hidden_redraw_requests_skipped": self.hidden_redraws, "segments": self.segments,
                "gpu_measurement": "not collected; use separate process sampling for the runtime gate" });
            if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
                let _ = std::fs::write(path, bytes);
            }
        }
        self.instance.take();
        self.tray.take();
        self.surface.take();
        eprintln!(
            "Backend: egui CPU + softbuffer Win32 GDI; layouts={}, presents={}",
            self.layout_count, self.present_count
        );
    }
}
