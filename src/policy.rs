use crate::model::{Config, Direction, Usage, app_key};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Default)]
struct Bucket {
    tokens: f64,
    at: Duration,
    rate: u64,
}

impl Bucket {
    fn available(&mut self, rate: u64, now: Duration) -> usize {
        if self.rate != rate {
            self.tokens = 0.0;
            self.at = now;
            self.rate = rate;
        }
        let elapsed = now.saturating_sub(self.at).as_secs_f64();
        // A shared quarter-second burst across all connections of this application.
        self.tokens = (self.tokens + elapsed * rate as f64).min((rate as f64 / 4.0).max(1.0));
        self.at = now;
        self.tokens.floor() as usize
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Permit {
    Bytes(usize),
    Wait,
    QuotaReached,
    Unmatched,
}

pub struct Engine {
    pub config: Config,
    pub call_mode: bool,
    pub usage: Usage,
    buckets: HashMap<(String, usize), Bucket>,
}

impl Engine {
    pub fn new(config: Config, usage: Usage) -> Self {
        Self {
            config,
            call_mode: false,
            usage,
            buckets: HashMap::new(),
        }
    }

    pub fn configure(&mut self, config: Config) {
        self.config = config;
        self.buckets.clear();
    }

    pub fn set_call_mode(&mut self, enabled: bool) {
        self.call_mode = enabled;
        self.buckets.clear();
    }

    pub fn matched(&self, path: &str) -> bool {
        let key = app_key(path);
        self.config
            .rules
            .iter()
            .any(|r| r.enabled && app_key(&r.path) == key)
    }

    pub fn rates(&self, path: &str, weekday: u8, minute: u16) -> Option<[u64; 2]> {
        let key = app_key(path);
        let rule = self
            .config
            .rules
            .iter()
            .find(|r| r.enabled && app_key(&r.path) == key)?;
        let active = rule
            .schedule
            .as_ref()
            .is_none_or(|s| s.active(weekday, minute));
        let mut rates = if active {
            [
                u64::from(rule.download_kib) * 1024,
                u64::from(rule.upload_kib) * 1024,
            ]
        } else {
            [0, 0]
        };
        if self.call_mode && rule.call_background {
            for (rate, call) in rates
                .iter_mut()
                .zip([self.config.call_download_kib, self.config.call_upload_kib])
            {
                let call = u64::from(call) * 1024;
                *rate = if *rate == 0 { call } else { (*rate).min(call) };
            }
        }
        Some(rates)
    }

    /// Reserve before writing: quotas and token budgets are shared by all flows.
    /// `day` is the local calendar date, `now` is monotonic and independent of DST.
    pub fn reserve(
        &mut self,
        path: &str,
        direction: Direction,
        requested: usize,
        now: Duration,
        day: &str,
        weekday: u8,
        minute: u16,
    ) -> Permit {
        let key = app_key(path);
        let Some(rates) = self.rates(&key, weekday, minute) else {
            return Permit::Unmatched;
        };
        if self.usage.day != day {
            self.usage.day = day.into();
            self.usage.apps.clear();
        }
        let quota = self
            .config
            .rules
            .iter()
            .find(|r| app_key(&r.path) == key)
            .map_or(0, |r| u64::from(r.daily_quota_mib) * 1024 * 1024);
        let usage = self.usage.apps.entry(key.clone()).or_default();
        let used = usage[0].saturating_add(usage[1]);
        if quota != 0 && used >= quota {
            return Permit::QuotaReached;
        }
        let mut count = requested.min(4096);
        if quota != 0 {
            count = count.min((quota - used).min(usize::MAX as u64) as usize);
        }
        let index = direction as usize;
        if rates[index] != 0 {
            let bucket = self.buckets.entry((key, index)).or_default();
            count = count.min(bucket.available(rates[index], now));
            if count == 0 {
                return Permit::Wait;
            }
            bucket.tokens -= count as f64;
        }
        usage[index] = usage[index].saturating_add(count as u64);
        Permit::Bytes(count)
    }

    pub fn refund(&mut self, path: &str, direction: Direction, bytes: usize, day: &str) {
        if self.usage.day == day
            && let Some(usage) = self.usage.apps.get_mut(&app_key(path))
        {
            usage[direction as usize] = usage[direction as usize].saturating_sub(bytes as u64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Rule, Schedule};
    const APP: &str = r"C:\Apps\Download.exe";
    fn engine() -> Engine {
        Engine::new(
            Config {
                rules: vec![Rule {
                    path: APP.into(),
                    download_kib: 4,
                    upload_kib: 0,
                    daily_quota_mib: 1,
                    call_background: true,
                    ..Rule::default()
                }],
                ..Config::default()
            },
            Usage::default(),
        )
    }
    #[test]
    fn overnight_window_belongs_to_start_day() {
        let s = Schedule {
            days: 1 << 6,
            start_minute: 1380,
            end_minute: 60,
        };
        assert!(s.active(6, 1439));
        assert!(s.active(0, 0));
        assert!(!s.active(0, 60));
        assert!(!s.active(6, 30));
    }
    #[test]
    fn all_day_and_exclusive_end() {
        let s = Schedule {
            days: 1,
            start_minute: 60,
            end_minute: 120,
        };
        assert!(s.active(0, 60));
        assert!(!s.active(0, 120));
        assert!(
            Schedule {
                start_minute: 0,
                end_minute: 0,
                ..s
            }
            .active(0, 1439)
        );
    }
    #[test]
    fn shared_bucket_cannot_be_multiplied_by_connections() {
        let mut e = engine();
        let reserve = |e: &mut Engine, t| {
            e.reserve(
                APP,
                Direction::Download,
                4096,
                Duration::from_millis(t),
                "2026-10-31",
                5,
                0,
            )
        };
        assert_eq!(reserve(&mut e, 0), Permit::Wait);
        assert_eq!(reserve(&mut e, 250), Permit::Bytes(1024));
        assert_eq!(reserve(&mut e, 250), Permit::Wait);
        assert_eq!(reserve(&mut e, 500), Permit::Bytes(1024));
    }
    #[test]
    fn quota_is_bidirectional_and_resets_at_midnight() {
        let mut e = engine();
        e.usage.day = "2026-10-31".into();
        e.usage.apps.insert(app_key(APP), [1024 * 1024 - 3, 0]);
        assert_eq!(
            e.reserve(
                APP,
                Direction::Upload,
                40,
                Duration::ZERO,
                "2026-10-31",
                5,
                0
            ),
            Permit::Bytes(3)
        );
        assert_eq!(
            e.reserve(
                APP,
                Direction::Download,
                40,
                Duration::ZERO,
                "2026-10-31",
                5,
                0
            ),
            Permit::QuotaReached
        );
        assert_eq!(
            e.reserve(
                APP,
                Direction::Upload,
                40,
                Duration::ZERO,
                "2026-11-01",
                6,
                0
            ),
            Permit::Bytes(40)
        );
    }
    #[test]
    fn call_caps_never_raise_a_stricter_cap() {
        let mut e = engine();
        e.set_call_mode(true);
        assert_eq!(e.rates(APP, 0, 0), Some([4096, 64 * 1024]));
        e.config.rules[0].schedule = Some(Schedule {
            days: 2,
            start_minute: 0,
            end_minute: 0,
        });
        assert_eq!(e.rates(APP, 0, 0), Some([256 * 1024, 64 * 1024]));
        e.config.rules[0].enabled = false;
        assert!(!e.matched(APP));
    }
    #[test]
    fn normalization_and_validation() {
        let mut c = engine().config;
        c.rules.push(Rule {
            path: "c:/apps/DOWNLOAD.exe".into(),
            ..Rule::default()
        });
        assert!(c.validate().is_err());
        assert_eq!(app_key(r"\\?\C:\Apps\DOWNLOAD.exe"), app_key(APP));
    }
}
