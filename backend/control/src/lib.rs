use serde::{Deserialize, Serialize};
pub mod transport;
pub const VERSION: u32 = 2;
pub const MAX_FRAME: usize = 128 * 1024;
pub const PIPE: &str = r"\\.\pipe\BandwidthLimiter.Control.v2";
pub const SERVICE: &str = "BandwidthLimiterService";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub executable: String,
    pub download_kib: u32,
    pub upload_kib: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub version: u32,
    pub rules: Vec<Rule>,
}
impl Policy {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_FRAME {
            return Err("WFP policy exceeds 128 KiB".into());
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != VERSION || self.rules.is_empty() || self.rules.len() > 32 {
            return Err("WFP requires protocol 2 and 1..32 application rules".into());
        }
        let mut seen = std::collections::HashSet::new();
        for r in &self.rules {
            let path = r.executable.replace('/', "\\").to_lowercase();
            let b = path.as_bytes();
            if b.len() < 7
                || b.len() > 32767
                || !b[0].is_ascii_alphabetic()
                || b[1] != b':'
                || b[2] != b'\\'
                || !path.ends_with(".exe")
                || path[2..].contains([':', '\0', '\n', '\r'])
                || path.split('\\').any(|s| s == "..")
                || !seen.insert(path)
            {
                return Err("Each WFP rule needs a unique absolute local executable path".into());
            }
            if r.download_kib > 1_048_576
                || r.upload_kib > 1_048_576
                || (r.download_kib == 0 && r.upload_kib == 0)
            {
                return Err(
                    "WFP rates must be 0..1048576 KiB/s, with at least one nonzero direction"
                        .into(),
                );
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", deny_unknown_fields)]
pub enum Action {
    Status,
    Apply { policy: Policy },
    Pause,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub action: Action,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub armed: bool,
    pub flows: u32,
    pub queued_segments: u32,
    pub in_flight: u32,
    pub queued_bytes: u64,
    pub download_submitted: u64,
    pub upload_submitted: u64,
    pub injection_failures: u64,
    pub untracked_flows: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u32,
    pub can_configure: bool,
    pub status: Option<Snapshot>,
    pub error: Option<String>,
}
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let payload = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if payload.len() > MAX_FRAME {
        return Err("Control message exceeds 128 KiB".into());
    }
    let mut bytes = (payload.len() as u32).to_le_bytes().to_vec();
    bytes.extend(payload);
    Ok(bytes)
}
pub fn decode_request(payload: &[u8]) -> Result<Request, String> {
    if payload.len() > MAX_FRAME {
        return Err("Control message exceeds 128 KiB".into());
    }
    let request: Request = serde_json::from_slice(payload).map_err(|e| e.to_string())?;
    if request.version != VERSION {
        return Err(
            "Incompatible WFP control protocol; update GUI, service and driver together".into(),
        );
    }
    if let Action::Apply { policy } = &request.action {
        policy.validate()?;
    }
    Ok(request)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_bidirectional_policy_and_frames() {
        let p = Policy::parse(br#"{"version":2,"rules":[{"executable":"C:\\App.exe","download_kib":0,"upload_kib":64}]}"#).unwrap();
        let frame = encode(&Request {
            version: VERSION,
            action: Action::Apply { policy: p },
        })
        .unwrap();
        assert_eq!(
            u32::from_le_bytes(frame[..4].try_into().unwrap()) as usize,
            frame.len() - 4
        );
        assert!(matches!(
            decode_request(&frame[4..]).unwrap().action,
            Action::Apply { .. }
        ));
        assert!(decode_request(br#"{"version":1,"action":{"command":"Pause"}}"#).is_err());
        assert!(Policy::parse(br#"{"version":2,"rules":[{"executable":"C:\\App.exe","download_kib":0,"upload_kib":0}]}"#).is_err());
        assert!(Policy::parse(br#"{"version":2,"rules":[{"executable":"C:\\App.exe","download_kib":1,"upload_kib":1,"quota":1}]}"#).is_err());
    }
}
