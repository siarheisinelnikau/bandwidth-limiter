#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod design;
mod instance;
mod paint;
mod preview;
mod runtime;
mod scheduling;
mod tray;
mod ui;
mod window;

use bandwidth_limiter::{storage, updater};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|a| a == "--render-preview") {
        if args.len() != 3 {
            return Err("Usage: --render-preview ABSOLUTE_OUTPUT_DIRECTORY".into());
        }
        preview::render(std::path::Path::new(&args[2]))?;
        return Ok(());
    }
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
    let diagnostics = if args.len() == 3 && args[1] == "--diagnostics" {
        let path = std::path::PathBuf::from(&args[2]);
        if !path.is_absolute() {
            return Err("Diagnostics path must be absolute".into());
        }
        Some(path)
    } else if args.len() == 1 {
        None
    } else {
        return Err(
            "Unknown arguments. Use --render-preview DIRECTORY or --diagnostics FILE.".into(),
        );
    };
    let bootstrap = match storage::lock(&directory) {
        Ok(guard) => storage::load(&directory).map(|(config, usage)| (guard, config, usage)),
        Err(error) => {
            if instance::request_open(&directory) {
                return Ok(());
            }
            Err(error)
        }
    };
    window::run(bootstrap, directory, diagnostics)
}
