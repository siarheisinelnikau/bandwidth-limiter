#![cfg(windows)]
mod device;
mod filters;
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
fn run(handle: SERVICE_STATUS_HANDLE) -> Result<(), String> {
    let path = service_policy_path()?;
    // The service has no user-facing IPC and reads only its administrator-owned installation directory.
    // Deployment must preserve the documented SYSTEM/Administrators-only write ACLs.
    for target in [
        path.as_path(),
        path.parent().ok_or("Invalid policy directory")?,
    ] {
        security::require_admin_owned(target)?;
    }
    let policy = read_policy(&path)?;
    let device = device::Device::open()?;
    let engine = filters::Engine::open()?;
    shutdown(&device, &engine)?; // recover a previous broker's non-dynamic objects only after drain
    device.configure(&policy.wire())?;
    if let Err(error) = engine.install(&policy) {
        let cleanup = shutdown(&device, &engine);
        return Err(format!("{error}; rollback/drain result: {cleanup:?}"));
    }
    if device.status()?.state != protocol::ARMED {
        shutdown(&device, &engine)?;
        return Err("Driver lease expired during filter setup; policy was removed".into());
    }
    report(handle, SERVICE_RUNNING, 0, 0);
    let mut failure = None;
    while !STOP.load(Ordering::Acquire) {
        if let Err(error) = device.command(protocol::HEARTBEAT) {
            STOP.store(true, Ordering::Release);
            failure = Some(format!("Heartbeat failed; draining: {error}"));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    report(handle, SERVICE_STOP_PENDING, 0, 1);
    shutdown(&device, &engine)?;
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
    // Error leaves owned filters resident; driver cleanup/lease expiration releases backpressure.
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
            println!("Valid version 1 receive-only TCP policy: {} executable rules. No device or WFP access.", policy.rules.len());
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
