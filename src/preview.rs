//! Finite offscreen rendering of the actual application UI. Never opens a window.
use crate::{
    paint::Paint,
    runtime::View,
    ui::{App, Page},
};
use bandwidth_limiter::model::{Config, Rule, Schedule, app_key};
use std::{
    fs::File,
    io::BufWriter,
    path::Path,
    time::{Duration, Instant},
};

fn fixture(empty: bool) -> (Config, View) {
    let config = Config {
        startup_update_check: false,
        rules: if empty {
            vec![]
        } else {
            [
                r"C:\Apps\DownloadManager.exe",
                r"C:\Apps\CloudSync.exe",
                r"C:\Games\GameLibrary.exe",
            ]
            .iter()
            .enumerate()
            .map(|(index, path)| Rule {
                path: (*path).into(),
                download_kib: if index == 0 { 1024 } else { 512 },
                upload_kib: 128,
                daily_quota_mib: 4096,
                call_background: true,
                schedule: (index == 0).then_some(Schedule {
                    days: 31,
                    start_minute: 540,
                    end_minute: 1080,
                }),
                ..Rule::default()
            })
            .collect()
        },
        ..Config::default()
    };
    let mut view = View::default();
    view.usage.day = "2026-10-31".into();
    for (index, rule) in config.rules.iter().enumerate() {
        view.rates
            .insert(app_key(&rule.path), [((index + 1) * 102400) as f64, 8192.0]);
        view.usage
            .apps
            .insert(app_key(&rule.path), [245 * 1048576, 12 * 1048576]);
    }
    view.connection_note = "No real network inventory is accessed in preview mode.".into();
    (config, view)
}
pub fn render(directory: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !directory.is_absolute() {
        return Err("Preview output directory must be absolute".into());
    }
    std::fs::create_dir_all(directory)?;
    let start = Instant::now();
    for (name, page, empty, error, width, height, scale) in [
        (
            "applications",
            Page::Applications,
            false,
            false,
            980u32,
            720u32,
            1.0,
        ),
        ("settings", Page::Settings, false, false, 980, 720, 1.0),
        ("empty", Page::Applications, true, false, 980, 720, 1.0),
        ("error", Page::Applications, false, true, 980, 720, 1.0),
        (
            "applications-125",
            Page::Applications,
            false,
            false,
            1050,
            775,
            1.25,
        ),
    ] {
        if start.elapsed() > Duration::from_secs(20) {
            return Err("Preview exceeded its internal 20-second limit".into());
        }
        let ctx = egui::Context::default();
        crate::design::apply(&ctx);
        ctx.set_pixels_per_point(scale);
        let (config, view) = fixture(empty);
        let mut app = App::fixture(config, view, page, error);
        let mut paint = Paint::new();
        // Two bounded passes allow font texture creation and initial layout to settle.
        for frame in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width as f32 / scale, height as f32 / scale),
                )),
                time: Some(frame as f64 / 60.0),
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| app.draw(ctx));
            paint.raster(&ctx, &output, width, height);
        }
        let path = directory.join(format!("{name}.png"));
        let temporary = path.with_extension("png.tmp");
        let file = BufWriter::new(File::create(&temporary)?);
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        let bytes: Vec<_> = paint
            .pixels
            .iter()
            .flat_map(|p| p.iter().copied())
            .collect();
        writer.write_image_data(&bytes)?;
        writer.finish()?;
        bandwidth_limiter::storage::replace(&temporary, &path)?;
    }
    std::fs::write(
        directory.join("capture.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"), "backend": "egui_software_backend 0.0.2 CPU rasterizer", "native_window": false,
            "fixture_data": true, "frames_per_screen": 2, "screens": 5, "elapsed_ms": start.elapsed().as_millis(),
            "network_workers": 0, "user_data_access": false, "tray_created": false, "normal_runtime_gate": "not measured"
        }))?,
    )?;
    Ok(())
}
