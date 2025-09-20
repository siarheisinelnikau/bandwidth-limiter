#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod runtime;
mod ui;
mod window;

use bandwidth_limiter::{storage, updater};
use eframe::egui;

fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|a| a == "--apply-update") {
        let result = (|| {
            if args.len() != 6 {
                return Err("Invalid update helper arguments".to_string());
            }
            let pid = args[2]
                .to_string_lossy()
                .parse::<u32>()
                .map_err(|e| e.to_string())?;
            updater::apply_update(
                pid,
                std::path::Path::new(&args[3]),
                std::path::Path::new(&args[4]),
                &args[5].to_string_lossy(),
            )
        })();
        if let Err(error) = result {
            if let Some(staged) = args.get(3).and_then(|p| std::path::Path::new(p).parent()) {
                let _ = std::fs::write(staged.join("last-update-error.txt"), error);
            }
            std::process::exit(1);
        }
        return Ok(());
    }
    let directory = storage::data_directory();
    let bootstrap = storage::lock(&directory)
        .and_then(|guard| storage::load(&directory).map(|(config, usage)| (guard, config, usage)));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 760.0])
            .with_min_inner_size([900.0, 620.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    window::run(
        "BandwidthLimiter",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            match bootstrap {
                Ok((guard, config, usage)) => Ok(Box::new(ui::App::new(
                    config,
                    usage,
                    directory,
                    guard,
                    cc.egui_ctx.clone(),
                ))),
                Err(message) => Ok(Box::new(StartupError(message))),
            }
        }),
    )
}

struct StartupError(String);
impl eframe::App for StartupError {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("BandwidthLimiter could not start"); ui.label(&self.0);
            ui.label("Existing settings were preserved. Close another instance or inspect the files in LocalAppData/BandwidthLimiter.");
        });
    }
}
