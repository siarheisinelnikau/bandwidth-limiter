#![cfg(windows)]
#[cfg(not(target_pointer_width = "64"))]
compile_error!("The WinDivert runtime adapter currently supports Windows x64 only");
mod abi;
mod controller;
mod ipc;
mod native;
mod packet;
mod security;
mod session;
mod shaper;
use bandwidth_limiter_packet_control::transport::Handle;
use bandwidth_limiter_packet_control::{Policy, SERVICE, Snapshot};
use std::{
    cell::RefCell,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
use windows_sys::Win32::System::Services::*;
use windows_sys::Win32::{Foundation::*, System::Threading::*};
static STOP: AtomicBool = AtomicBool::new(false);
static STOP_EVENT: OnceLock<Handle> = OnceLock::new();
struct Backend {
    session: RefCell<Option<session::Session>>,
    status: session::Shared,
}
impl Backend {
    fn new() -> Self {
        Self {
            session: RefCell::new(None),
            status: Arc::new(Mutex::new(Snapshot::default())),
        }
    }
}
impl controller::Backend for Backend {
    fn snapshot(&self) -> Result<Snapshot, String> {
        Ok(self.status.lock().unwrap().clone())
    }
    fn apply(&self, policy: &Policy) -> Result<(), String> {
        policy.validate()?;
        // Validate/load protected runtime before disrupting an existing policy. No driver open here.
        let api = native::Api::load()?;
        let was_active = self.status.lock().unwrap().active;
        let paused = self.pause();
        if was_active {
            paused?;
        }
        *self.session.borrow_mut() = Some(session::Session::start(
            api,
            policy.clone(),
            self.status.clone(),
        )?);
        Ok(())
    }
    fn pause(&self) -> Result<(), String> {
        if let Some(mut session) = self.session.borrow_mut().take() {
            session.stop();
            return self
                .status
                .lock()
                .unwrap()
                .last_error
                .clone()
                .map_or(Ok(()), Err);
        }
        Ok(())
    }
}
fn report(handle: SERVICE_STATUS_HANDLE, state: u32, error: bool) {
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: if state == SERVICE_RUNNING {
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
        } else {
            0
        },
        dwWin32ExitCode: if error { 1066 } else { 0 },
        dwServiceSpecificExitCode: u32::from(error),
        dwCheckPoint: if matches!(state, SERVICE_START_PENDING | SERVICE_STOP_PENDING) {
            1
        } else {
            0
        },
        dwWaitHint: 15000,
    };
    unsafe {
        SetServiceStatus(handle, &status);
    }
}
unsafe extern "system" fn control(code: u32) {
    if code == SERVICE_CONTROL_STOP || code == SERVICE_CONTROL_SHUTDOWN {
        STOP.store(true, Ordering::Release);
        if let Some(event) = STOP_EVENT.get() {
            event.signal();
        }
    }
}
fn run(handle: SERVICE_STATUS_HANDLE) -> Result<(), String> {
    use controller::Backend as _;
    let backend = Backend::new();
    // Starting/status do not load a DLL, open a driver, install or capture anything.
    let server = ipc::Server::new()?;
    let stopped = STOP_EVENT.get().ok_or("Stop event was not initialized")?;
    let events = [stopped.0, server.ready.0];
    report(handle, SERVICE_RUNNING, false);
    let result = (|| {
        while !STOP.load(Ordering::Acquire) {
            if unsafe { WaitForMultipleObjects(2, events.as_ptr(), 0, INFINITE) } == WAIT_FAILED {
                return Err(std::io::Error::last_os_error().to_string());
            }
            if STOP.load(Ordering::Acquire) {
                break;
            }
            // Reset before draining the channel, so a later send cannot lose its wakeup.
            unsafe {
                ResetEvent(server.ready.0);
            }
            loop {
                match server.incoming.try_recv() {
                    Ok(message) => {
                        let response =
                            controller::execute(&backend, message.request, message.administrator);
                        let _ = message.reply.try_send(response);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(_) => return Err("Control pipe worker stopped".into()),
                }
            }
        }
        Ok(())
    })();
    report(handle, SERVICE_STOP_PENDING, false);
    drop(server);
    let paused = backend.pause();
    result.and(paused)
}
unsafe extern "system" fn service_main(_: u32, _: *mut *mut u16) {
    let Ok(event) = Handle::event() else {
        return;
    };
    if STOP_EVENT.set(event).is_err() {
        return;
    }
    let name: Vec<_> = SERVICE.encode_utf16().chain(Some(0)).collect();
    let handle = unsafe { RegisterServiceCtrlHandlerW(name.as_ptr(), Some(control)) };
    if handle.is_null() {
        return;
    }
    report(handle, SERVICE_START_PENDING, false);
    let result = run(handle);
    if let Err(error) = &result {
        record_error(error);
    }
    report(handle, SERVICE_STOPPED, result.is_err());
}
fn record_error(error: &str) {
    use windows_sys::Win32::System::EventLog::*;
    let name: Vec<_> = SERVICE.encode_utf16().chain(Some(0)).collect();
    let message: Vec<_> = error.encode_utf16().take(8192).chain(Some(0)).collect();
    let strings = [message.as_ptr()];
    unsafe {
        let log = RegisterEventSourceW(std::ptr::null(), name.as_ptr());
        if !log.is_null() {
            ReportEventW(
                log,
                EVENTLOG_ERROR_TYPE,
                0,
                1,
                std::ptr::null_mut(),
                1,
                0,
                strings.as_ptr(),
                std::ptr::null(),
            );
            DeregisterEventSource(log);
        }
    }
}
fn command() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() == 2 && args[1] == "--service" {
        let mut name: Vec<_> = SERVICE.encode_utf16().chain(Some(0)).collect();
        let table = [
            SERVICE_TABLE_ENTRYW {
                lpServiceName: name.as_mut_ptr(),
                lpServiceProc: Some(service_main),
            },
            SERVICE_TABLE_ENTRYW::default(),
        ];
        if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        return Ok(());
    }
    Err("Usage: BandwidthLimiterPacketService.exe --service. SCM registration and signed WinDivert runtime staging are separate administrator actions. Service starts paused; only authenticated Apply opens existing driver handles with NO_INSTALL.".into())
}
fn main() {
    if let Err(error) = command() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
