use crate::protocol;
use std::{
    fs::{File, OpenOptions},
    os::windows::io::AsRawHandle,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::IO::DeviceIoControl;

pub struct Device(File);
impl Device {
    pub fn open() -> Result<Self, String> {
        OpenOptions::new().read(true).write(true).open(r"\\.\BandwidthLimiterWfp").map(Self)
            .map_err(|e| format!("Cannot open the restricted WFP device; driver must be loaded and caller must be an administrator: {e}"))
    }
    fn request(&self, code: u32, input: &[u8], output: &mut [u8]) -> Result<u32, String> {
        let mut returned = 0;
        // SAFETY: synchronous METHOD_BUFFERED request; buffers are live, bounded and writable where required.
        let success = unsafe {
            DeviceIoControl(
                self.0.as_raw_handle(),
                code,
                input.as_ptr().cast(),
                input.len() as u32,
                output.as_mut_ptr().cast(),
                output.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if success == 0 {
            Err(format!(
                "Device request {code:#x}: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(returned)
        }
    }
    pub fn command(&self, code: u32) -> Result<(), String> {
        self.request(code, &[], &mut []).map(|_| ())
    }
    pub fn configure(&self, config: &protocol::Config) -> Result<(), String> {
        // SAFETY: repr(C), fixed-width fields and no padding/embedded pointers; layout has a fixture test.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (config as *const protocol::Config).cast(),
                size_of::<protocol::Config>(),
            )
        };
        self.request(protocol::CONFIGURE, bytes, &mut [])
            .map(|_| ())
    }
    pub fn status(&self) -> Result<protocol::Status, String> {
        let mut status = protocol::Status::default();
        // SAFETY: all fields are plain integers, and the buffer covers the full initialized object.
        let bytes = unsafe {
            std::slice::from_raw_parts_mut(
                (&mut status as *mut protocol::Status).cast(),
                size_of::<protocol::Status>(),
            )
        };
        let returned = self.request(protocol::QUERY, &[], bytes)?;
        if returned as usize != size_of::<protocol::Status>()
            || status.version != 2
            || status.size as usize != size_of::<protocol::Status>()
        {
            return Err("Incompatible WFP driver protocol".into());
        }
        Ok(status)
    }
    pub fn drain(&self) -> Result<(), String> {
        self.command(protocol::DRAIN)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let status = self.status()?;
            if matches!(
                status.state,
                protocol::DRAINING | protocol::QUIESCING | protocol::QUIESCED
            ) && status.pending == 0
                && status.continuations == 0
                && status.flows == 0
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "Drain did not finish; filters were retained: {status:?}"
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    pub fn clear_flows(&self) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match self.command(protocol::CLEAR_FLOWS) {
                Ok(()) => return Ok(()),
                Err(e) if Instant::now() >= deadline => return Err(e),
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}
