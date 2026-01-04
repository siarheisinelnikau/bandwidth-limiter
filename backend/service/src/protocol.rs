//! Pointer-free layout shared with backend/shared/protocol.h.
pub const MAX_RULES: usize = 32;
pub const CONFIGURE: u32 = 0x22e000;
pub const HEARTBEAT: u32 = 0x22e004;
pub const DRAIN: u32 = 0x22e008;
pub const QUERY: u32 = 0x22e00c;
pub const CLEAR_FLOWS: u32 = 0x22e010;
pub const QUIESCE: u32 = 0x22e014;
pub const DRAINING: u32 = 1;
pub const ARMED: u32 = 2;
pub const QUIESCED: u32 = 3;
pub const QUIESCING: u32 = 4;
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub version: u32,
    pub size: u32,
    pub count: u32,
    pub lease_ms: u32,
    pub download_bytes_per_second: [u64; MAX_RULES],
    pub upload_bytes_per_second: [u64; MAX_RULES],
}
impl Config {
    pub fn new(rates: impl IntoIterator<Item = u64>) -> Self {
        let mut value = Self {
            version: 2,
            size: size_of::<Self>() as u32,
            count: 0,
            lease_ms: 5000,
            download_bytes_per_second: [0; MAX_RULES],
            upload_bytes_per_second: [0; MAX_RULES],
        };
        for rate in rates {
            value.download_bytes_per_second[value.count as usize] = rate;
            value.count += 1;
        }
        value
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Status {
    pub version: u32,
    pub size: u32,
    pub state: u32,
    pub pending: u32,
    pub flows: u32,
    pub filters: u32,
    pub continuations: u32,
    pub reserved: u32,
    pub admitted_bytes: u64,
    pub upload_bytes: u64,
    pub queued_bytes: u64,
    pub queued_peak: u64,
    pub injections: u64,
    pub inject_failures: u64,
    pub untracked_flows: u64,
    pub aborted_flows: u64,
}
pub fn guid(id: u32) -> windows_sys::core::GUID {
    windows_sys::core::GUID {
        data1: id,
        data2: 0x752d,
        data3: 0x4ae7,
        data4: [0xb3, 0x5f, 0x91, 0x83, 0x2c, 0x65, 0x07, 0x11],
    }
}
pub const PROVIDER: u32 = 0xb1070001;
pub const SUBLAYER: u32 = 0xb1070002;
pub const CALLOUTS: [u32; 4] = [0xb1070010, 0xb1070011, 0xb1070012, 0xb1070013];
pub const STREAM_FILTERS: [u32; 2] = [0xb1070020, 0xb1070021];
pub fn app_filter(slot: usize, ipv6: bool) -> u32 {
    0xb1070100 + slot as u32 * 2 + u32::from(ipv6)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn c_and_rust_wire_layout_agree() {
        assert_eq!(size_of::<Config>(), 528);
        assert_eq!(size_of::<Status>(), 96);
        assert_eq!(std::mem::offset_of!(Status, admitted_bytes), 32);
        let header = include_str!("../../shared/protocol.h");
        assert!(header.contains("#define BL_PROVIDER_ID 0xb1070001u"));
        for id in CALLOUTS {
            assert!(header.contains(&format!("0x{id:08x}u")));
        }
        assert_eq!(CONFIGURE, (0x22 << 16) | (3 << 14) | (0x800 << 2));
        assert_eq!(QUIESCE - CONFIGURE, 20);
    }
}
