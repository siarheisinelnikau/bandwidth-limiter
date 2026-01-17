pub use bandwidth_limiter_control::Policy;
pub fn wire(policy: &Policy) -> crate::protocol::Config {
    let mut config = crate::protocol::Config::new(
        policy
            .rules
            .iter()
            .map(|r| u64::from(r.download_kib) * 1024),
    );
    for (index, rule) in policy.rules.iter().enumerate() {
        config.upload_bytes_per_second[index] = u64::from(rule.upload_kib) * 1024;
    }
    config
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_rates_reach_kernel_and_unsupported_fields_fail() {
        let policy = Policy::parse(br#"{"version":2,"rules":[{"executable":"C:\\App.exe","download_kib":0,"upload_kib":32}]}"#).unwrap();
        assert_eq!(wire(&policy).upload_bytes_per_second[0], 32768);
        assert!(Policy::parse(br#"{"version":2,"rules":[{"executable":"C:\\App.exe","download_kib":1,"upload_kib":1,"quota":2}]}"#).is_err());
    }
}
