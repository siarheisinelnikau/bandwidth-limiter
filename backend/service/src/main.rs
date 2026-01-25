#![cfg(windows)]
mod controller;
mod device;
mod filters;
mod ipc;
mod policy;
mod protocol;
mod security;

use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use windows_sys::Win32::System::Services::*;
static STOP: AtomicBool = AtomicBool::new(false);
const NAME: &str = "BandwidthLimiterService";

fn read_policy(path: &Path) -> Result<policy::Policy, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| format!("Cannot open policy: {e}"))?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    policy::Policy::parse(&bytes)
}
fn service_policy_path() -> Result<std::path::PathBuf, String> {
    let base = std::env::var_os("ProgramFiles").ok_or("ProgramFiles environment is unavailable")?;
    Ok(std::path::PathBuf::from(base)
        .join("BandwidthLimiterWfp")
        .join("policy.json"))
}
fn report(handle: SERVICE_STATUS_HANDLE, state: u32, code: u32, checkpoint: u32) {
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: if state == SERVICE_RUNNING {
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
        } else {
            0
        },
        dwWin32ExitCode: code,
        dwServiceSpecificExitCode: u32::from(code != 0),
        dwCheckPoint: checkpoint,
        dwWaitHint: if state == SERVICE_START_PENDING || state == SERVICE_STOP_PENDING {
            60000
        } else {
            0
        },
    };
    // SAFETY: SCM supplied this handle; status is initialized and valid for the synchronous call.
    unsafe {
        SetServiceStatus(handle, &status);
    }
}
unsafe extern "system" fn control(code: u32) {
    if code == SERVICE_CONTROL_STOP || code == SERVICE_CONTROL_SHUTDOWN {
        STOP.store(true, Ordering::Release);
    }
}
fn shutdown(device: &device::Device, engine: &filters::Engine) -> Result<(), String> {
    device.drain()?;
    engine.remove_owned()?;
    device.clear_flows()
}
struct NativeBackend {
    device: device::Device,
    engine: filters::Engine,
}
impl controller::Backend for NativeBackend {
    fn snapshot(&self) -> Result<bandwidth_limiter_control::Snapshot, String> {
        let s = self.device.status()?;
        Ok(bandwidth_limiter_control::Snapshot {
            armed: s.state == protocol::ARMED,
            flows: s.flows,
            queued_segments: s.pending.saturating_sub(s.continuations),
            in_flight: s.continuations,
            queued_bytes: s.queued_bytes,
            download_submitted: s.admitted_bytes,
            upload_submitted: s.upload_bytes,
            injection_failures: s.inject_failures,
            untracked_flows: s.untracked_flows,
        })
    }
    fn apply(&self, policy: &policy::Policy) -> Result<(), String> {
        policy.validate()?;
        shutdown(&self.device, &self.engine)?;
        self.device.configure(&policy::wire(policy))?;
        if let Err(error) = self.engine.install(policy) {
            let cleanup = shutdown(&self.device, &self.engine);
            return Err(format!("{error}; cleanup: {cleanup:?}"));
        }
        self.device.command(protocol::HEARTBEAT)
    }
    fn pause(&self) -> Result<(), String> {
        shutdown(&self.device, &self.engine)
    }
}
fn run(handle: SERVICE_STATUS_HANDLE) -> Result<(), String> {
    let path = service_policy_path()?;
    // Startup policy comes only from the administrator-owned installation directory.
    // Authenticated pipe changes are in-memory and never overwrite this file.
    // Deployment must preserve the documented SYSTEM/Administrators-only write ACLs.
    for target in [
        path.as_path(),
        path.parent().ok_or("Invalid policy directory")?,
    ] {
        security::require_admin_owned(target)?;
    }
    let policy = read_policy(&path)?;
    use controller::Backend;
    let backend = NativeBackend {
        device: device::Device::open()?,
        engine: filters::Engine::open()?,
    };
    let server = ipc::Server::new()?;
    backend.apply(&policy)?;
    report(handle, SERVICE_RUNNING, 0, 0);
    let mut failure = None;
    while !STOP.load(Ordering::Acquire) {
        if backend.snapshot()?.armed
            && let Err(error) = backend.device.command(protocol::HEARTBEAT)
        {
            failure = Some(format!("Heartbeat failed; draining: {error}"));
            break;
        }
        match server.incoming.recv_timeout(Duration::from_millis(500)) {
            Ok(message) => {
                let response =
                    controller::execute(&backend, message.request, message.administrator);
                let _ = message.reply.try_send(response);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                failure = Some(
                    "WFP pipe worker stopped; draining instead of polling a closed queue".into(),
                );
                break;
            }
            Err(_) => {}
        }
    }
    report(handle, SERVICE_STOP_PENDING, 0, 1);
    drop(server);
    backend.pause()?;
    if let Some(error) = failure {
        Err(error)
    } else {
        Ok(())
    }
}
unsafe extern "system" fn service_main(_: u32, _: *mut *mut u16) {
    let name: Vec<_> = NAME.encode_utf16().chain(Some(0)).collect();
    // SAFETY: stable service name and a static function callback; no borrowed callback context.
    let handle = unsafe { RegisterServiceCtrlHandlerW(name.as_ptr(), Some(control)) };
    if handle.is_null() {
        return;
    }
    report(handle, SERVICE_START_PENDING, 0, 1);
    let result = run(handle);
    if let Err(error) = &result {
        eprintln!("{error}");
        record_error(error);
    }
    // Error leaves owned filters resident; driver cleanup/lease expiration aborts tracked TCP.
    // A later service start or --recover performs the guarded removal sequence.
    report(
        handle,
        SERVICE_STOPPED,
        if result.is_ok() { 0 } else { 1066 },
        0,
    );
}
fn record_error(message: &str) {
    use windows_sys::Win32::System::EventLog::*;
    let source: Vec<_> = NAME.encode_utf16().chain(Some(0)).collect();
    let message: Vec<_> = message.encode_utf16().take(8192).chain(Some(0)).collect();
    let strings = [message.as_ptr()];
    // SAFETY: strings live throughout the call. No registry source registration or log clearing.
    unsafe {
        let log = RegisterEventSourceW(std::ptr::null(), source.as_ptr());
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
fn main() {
    if let Err(error) = command() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn command() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().collect();
    match args.get(1).and_then(|s| s.to_str()) {
        Some("--validate-policy") if args.len() == 3 => {
            let policy = read_policy(Path::new(&args[2]))?;
            println!("Valid version 2 bidirectional TCP policy: {} executable rules. No device or WFP access.", policy.rules.len());
            Ok(())
        }
        Some("--service") if args.len() == 2 => {
            let mut name: Vec<_> = NAME.encode_utf16().chain(Some(0)).collect();
            let table = [SERVICE_TABLE_ENTRYW { lpServiceName: name.as_mut_ptr(), lpServiceProc: Some(service_main) }, SERVICE_TABLE_ENTRYW::default()];
            // SAFETY: table is terminated and remains live throughout SCM dispatch.
            if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 { Err(std::io::Error::last_os_error().to_string()) } else { Ok(()) }
        }
        Some("--status") if args.len() == 2 => { println!("{:?}", device::Device::open()?.status()?); Ok(()) }
        Some("--recover" | "--prepare-unload") if args.len() == 2 => {
            let device = device::Device::open()?; let engine = filters::Engine::open()?;
            shutdown(&device, &engine)?;
            if args[1] == "--prepare-unload" { device.command(protocol::QUIESCE)?; }
            println!("{:?}", device.status()?); Ok(())
        }
        _ => Err("Usage: BandwidthLimiterService.exe --service | --validate-policy FILE | --status | --recover | --prepare-unload. Recovery/unload commands modify only this product's WFP objects; administrator action is required.".into()),
    }
}
