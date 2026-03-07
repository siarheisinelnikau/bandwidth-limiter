//! Explicit service control; importing or constructing the UI never probes/starts a service.
use crate::model::Config;
use bandwidth_limiter_packet_control::{Action, Policy, Response, Rule, VERSION};
#[derive(Clone)]
pub enum Command {
    Refresh,
    Apply(Config, bool),
    Pause,
}
#[derive(Clone, Default)]
pub struct State {
    pub busy: bool,
    pub response: Option<Response>,
    pub error: Option<String>,
}
pub fn policy(config: &Config, call_mode: bool) -> Result<Policy, String> {
    config.validate()?;
    if call_mode {
        return Err("Turn off proxy Call mode before applying static System TCP limits. System TCP Call mode is not implemented.".into());
    }
    let mut rules = vec![];
    for r in config.rules.iter().filter(|r| r.enabled) {
        if r.daily_quota_mib != 0 || r.schedule.is_some() {
            return Err("System TCP does not support quotas or schedules yet. Remove them from enabled rules before applying; the current service policy was not changed.".into());
        }
        if r.download_kib != 0 || r.upload_kib != 0 {
            rules.push(Rule {
                executable: r.path.clone(),
                download_kib: r.download_kib,
                upload_kib: r.upload_kib,
            });
        }
    }
    let policy = Policy {
        version: VERSION,
        rules,
    };
    policy.validate()?;
    Ok(policy)
}
pub fn action(command: Command) -> Result<Action, String> {
    match command {
        Command::Refresh => Ok(Action::Status {}),
        Command::Pause => Ok(Action::Pause {}),
        Command::Apply(config, call) => Ok(Action::Apply {
            policy: policy(&config, call)?,
        }),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_rules_do_not_become_silent_partial_policies() {
        let mut c = Config {
            rules: vec![crate::model::Rule {
                path: r"C:\App.exe".into(),
                download_kib: 0,
                upload_kib: 32,
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(policy(&c, false).unwrap().rules[0].upload_kib, 32);
        c.rules[0].daily_quota_mib = 1;
        assert!(policy(&c, false).is_err());
        c.rules[0].daily_quota_mib = 0;
        assert!(policy(&c, true).is_err());
        c.rules[0].enabled = false;
        assert!(policy(&c, false).is_err());
    }
}
