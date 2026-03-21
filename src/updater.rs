//! BandwidthLimiter configuration and presentation state for the shared updater.
use crate::model::VERSION;
pub use rust_auto_updator::{Prepared, Release};

#[derive(Clone, Debug, Default)]
pub enum State {
    #[default]
    Idle,
    Checking,
    Current,
    Available(Box<Release>),
    Downloading {
        received: u64,
        total: u64,
    },
    Ready,
    Error(String),
}

pub fn config() -> Result<rust_auto_updator::Config, String> {
    let mut config = rust_auto_updator::Config::new(
        "siarheisinelnikau",
        "bandwidth-limiter",
        VERSION,
        "BandwidthLimiter",
    )?;
    config.expected_subsystem = Some(2);
    config.checksum_name = "SHA256SUMS".into();
    config.max_executable_size = 100 * 1024 * 1024;
    config.install.stage_prefix = ".bandwidth-limiter-update-".into();
    Ok(config)
}

pub fn check() -> Result<Option<Release>, String> {
    rust_auto_updator::check_latest(&config()?)
}

pub fn download(release: &Release, progress: impl FnMut(u64, u64)) -> Result<Prepared, String> {
    rust_auto_updator::prepare(release, &config()?, progress)
}

pub fn install(prepared: &mut Prepared) -> Result<(), String> {
    rust_auto_updator::handoff(prepared)
}
