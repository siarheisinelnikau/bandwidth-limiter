use serde::Deserialize;
use std::{collections::HashSet, path::Path};
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub version: u32,
    pub rules: Vec<Rule>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub executable: String,
    pub download_kib: u32,
}
impl Policy {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 1024 * 1024 {
            return Err("Policy exceeds one MiB".into());
        }
        let policy: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if policy.version != 1
            || policy.rules.is_empty()
            || policy.rules.len() > crate::protocol::MAX_RULES
        {
            return Err("Policy version must be 1 with between 1 and 32 rules".into());
        }
        let mut paths = HashSet::new();
        for rule in &policy.rules {
            let key = rule.executable.to_lowercase();
            if key.len() > 32767
                || key.contains(['\0', '\r', '\n'])
                || key.chars().skip(2).any(|c| c == ':')
                || !key.ends_with(".exe")
                || !Path::new(&rule.executable).is_absolute()
                || key.starts_with(r"\\")
                || key.split(['\\', '/']).any(|part| part == "..")
            {
                return Err("Each executable needs an absolute local .exe path; UNC/device/parent paths are refused".into());
            }
            if rule.download_kib == 0
                || rule.download_kib > 1_048_576
                || !paths.insert(key.replace('/', "\\"))
            {
                return Err(
                    "Download rate must be 1..1048576 KiB/s and paths must be unique".into(),
                );
            }
        }
        Ok(policy)
    }
    pub fn wire(&self) -> crate::protocol::Config {
        crate::protocol::Config::new(self.rules.iter().map(|r| u64::from(r.download_kib) * 1024))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_controls_are_rejected_instead_of_ignored() {
        let input =
            br#"{"version":1,"rules":[{"executable":"C:\\Fixture.exe","download_kib":64}]}"#;
        assert_eq!(
            Policy::parse(input)
                .unwrap()
                .wire()
                .download_bytes_per_second[0],
            65536
        );
        let unsupported = br#"{"version":1,"rules":[{"executable":"C:\\Fixture.exe","download_kib":64,"upload_kib":1}]}"#;
        assert!(Policy::parse(unsupported).is_err());
        assert!(Policy::parse(br#"{"version":1,"rules":[]}"#).is_err());
    }
    #[test]
    fn duplicate_and_relative_targets_are_rejected() {
        assert!(Policy::parse(br#"{"version":1,"rules":[{"executable":"C:\\Fixture.exe:stream.exe","download_kib":1}]}"#).is_err());
        assert!(
            Policy::parse(
                br#"{"version":1,"rules":[{"executable":"Fixture.exe","download_kib":1}]}"#
            )
            .is_err()
        );
        assert!(Policy::parse(br#"{"version":1,"rules":[{"executable":"C:\\Fixture.exe","download_kib":1},{"executable":"c:/fixture.exe","download_kib":2}]}"#).is_err());
    }
}
