use serde::{Deserialize, Serialize};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPOSITORY: &str = "siarheisinelnikau/bandwidth-limiter";

/// Windows executable identity, independent of PID and ASCII path casing.
pub fn app_key(path: &str) -> String {
    path.trim()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .to_lowercase()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Schedule {
    /// Monday is bit zero. The day on which an overnight window starts owns it.
    pub days: u8,
    pub start_minute: u16,
    pub end_minute: u16,
}

impl Schedule {
    pub fn active(&self, weekday: u8, minute: u16) -> bool {
        if weekday > 6 || minute >= 1440 {
            return false;
        }
        let today = self.days & (1 << weekday) != 0;
        if self.start_minute == self.end_minute {
            return today;
        }
        if self.start_minute < self.end_minute {
            today && minute >= self.start_minute && minute < self.end_minute
        } else {
            (today && minute >= self.start_minute)
                || (self.days & (1 << ((weekday + 6) % 7)) != 0 && minute < self.end_minute)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    pub path: String,
    pub enabled: bool,
    /// Zero means unlimited. Values use binary KiB/s, not kilobits.
    pub download_kib: u32,
    pub upload_kib: u32,
    pub daily_quota_mib: u32,
    pub call_background: bool,
    pub schedule: Option<Schedule>,
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            path: String::new(),
            enabled: true,
            download_kib: 1024,
            upload_kib: 256,
            daily_quota_mib: 0,
            call_background: false,
            schedule: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub schema: u32,
    pub proxy_port: u16,
    pub startup_update_check: bool,
    pub call_download_kib: u32,
    pub call_upload_kib: u32,
    pub rules: Vec<Rule>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema: 1,
            proxy_port: 1088,
            startup_update_check: true,
            call_download_kib: 256,
            call_upload_kib: 64,
            rules: vec![],
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1 {
            return Err("Unsupported settings schema; existing file was preserved".into());
        }
        if self.proxy_port == 0 {
            return Err("Proxy port must be between 1 and 65535".into());
        }
        if self.rules.len() > 256 {
            return Err("At most 256 application rules are supported".into());
        }
        if self.call_download_kib == 0 || self.call_upload_kib == 0 {
            return Err("Call mode caps must be greater than zero".into());
        }
        let mut paths = std::collections::HashSet::new();
        for rule in &self.rules {
            let path = app_key(&rule.path);
            let absolute =
                path.as_bytes().get(1) == Some(&b':') && path.as_bytes().get(2) == Some(&b'\\');
            if path.len() > 32767
                || !(absolute || path.starts_with(r"\\"))
                || !path.ends_with(".exe")
                || path.contains(['\n', '\r', '\0'])
            {
                return Err("Each rule needs a full Windows executable path ending in .exe".into());
            }
            if !paths.insert(path) {
                return Err("Duplicate executable path".into());
            }
            if let Some(s) = &rule.schedule
                && (s.days == 0 || s.days > 127 || s.start_minute >= 1440 || s.end_minute >= 1440)
            {
                return Err("Schedule needs at least one day and valid times".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Download = 0,
    Upload = 1,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Usage {
    pub day: String,
    pub apps: std::collections::BTreeMap<String, [u64; 2]>,
}
