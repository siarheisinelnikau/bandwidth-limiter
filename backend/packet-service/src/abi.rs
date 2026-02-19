//! Independently declared WinDivert 2.2 x64 ABI; no upstream implementation is bundled.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Address {
    pub timestamp: i64,
    pub flags: u32,
    pub reserved: u32,
    pub data: [u64; 8],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct FlowData {
    pub endpoint: u64,
    pub parent: u64,
    pub pid: u32,
    pub local: [u32; 4],
    pub remote: [u32; 4],
    pub local_port: u16,
    pub remote_port: u16,
    pub protocol: u8,
}
impl Address {
    pub fn outbound(&self) -> bool {
        self.flags & (1 << 17) != 0
    }
    pub fn event(&self) -> u32 {
        (self.flags >> 8) & 255
    }
    pub fn ipv6(&self) -> bool {
        self.flags & (1 << 20) != 0
    }
    pub fn flow(&self) -> FlowData {
        // SAFETY: union storage is 64 bytes and aligned to 8; FlowData is 64 bytes.
        // Only call for a FLOW event, after checking its layer/event in the native adapter.
        unsafe { self.data.as_ptr().cast::<FlowData>().read() }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn documented_x64_abi_and_bitfields() {
        assert_eq!(size_of::<Address>(), 80);
        assert_eq!(align_of::<Address>(), 8);
        assert_eq!(std::mem::offset_of!(Address, data), 16);
        assert_eq!(size_of::<FlowData>(), 64);
        assert_eq!(std::mem::offset_of!(FlowData, protocol), 56);
        let a = Address {
            flags: 2 | (1 << 8) | (1 << 17) | (1 << 20),
            ..Default::default()
        };
        assert!(a.outbound() && a.ipv6());
        assert_eq!(a.event(), 1);
    }
}
