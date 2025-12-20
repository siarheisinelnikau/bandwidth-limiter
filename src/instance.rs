//! Same-session, data-directory-specific wake signal. No socket or message polling.
use crate::window::Wake;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, WAIT_OBJECT_0},
    System::Threading::{
        CreateEventW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent, WaitForSingleObject,
    },
};
fn name(directory: &Path) -> Vec<u16> {
    let digest = Sha256::digest(directory.to_string_lossy().to_lowercase().as_bytes());
    format!("Local\\BandwidthLimiter-{:x}", digest)
        .encode_utf16()
        .chain(Some(0))
        .collect()
}
pub fn request_open(directory: &Path) -> bool {
    // SAFETY: event is opened with signal-only rights, never used to send policy or privileged commands.
    unsafe {
        let event = OpenEventW(EVENT_MODIFY_STATE, 0, name(directory).as_ptr());
        if event.is_null() {
            return false;
        }
        let ok = SetEvent(event) != 0;
        CloseHandle(event);
        ok
    }
}
pub struct Instance {
    handle: usize,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Instance {
    pub fn new(
        directory: &Path,
        proxy: winit::event_loop::EventLoopProxy<Wake>,
    ) -> Result<Self, String> {
        // SAFETY: auto-reset event, default current-user object security, session-local name.
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, name(directory).as_ptr()) };
        if event.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let handle = event as usize;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = std::thread::Builder::new()
            .name("instance-open".into())
            .spawn(move || {
                // SAFETY: owner retains the handle until this worker has been signaled, stopped and joined.
                while unsafe { WaitForSingleObject(handle as _, INFINITE) } == WAIT_OBJECT_0 {
                    if stopping.load(Ordering::Acquire) {
                        break;
                    }
                    let _ = proxy.send_event(Wake::Menu("open".into()));
                }
            })
            .map_err(|e| {
                unsafe {
                    CloseHandle(event);
                }
                e.to_string()
            })?;
        Ok(Self {
            handle,
            stop,
            worker: Some(worker),
        })
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // SAFETY: wake/join before closing the owned event; this cannot affect another app's event.
        unsafe {
            SetEvent(self.handle as _);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        unsafe {
            CloseHandle(self.handle as _);
        }
    }
}
