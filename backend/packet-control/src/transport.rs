//! Windows-only bounded overlapped pipe transport. No operation is performed on import.
use crate::{Action, MAX_FRAME, PIPE, Request, Response, SERVICE, VERSION};
use std::{
    os::windows::io::AsRawHandle,
    path::Path,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{IO::*, Pipes::*, Services::*, Threading::*},
};

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
pub struct Handle(pub HANDLE);
// SAFETY: owned kernel handles support cross-thread waits and cancellation; never close while I/O is pending.
unsafe impl Send for Handle {}
// SAFETY: Windows kernel handle operations are thread-safe; Drop runs only after the last owner.
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
impl Handle {
    pub fn event() -> Result<Self, String> {
        let h = unsafe { CreateEventW(null(), 1, 0, null()) };
        if h.is_null() {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(Self(h))
        }
    }
    pub fn signal(&self) {
        unsafe {
            SetEvent(self.0);
        }
    }
    pub fn signaled(&self) -> bool {
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_OBJECT_0 }
    }
}
pub struct Pipe(pub Handle);
impl Pipe {
    pub fn server() -> Result<Self, String> {
        // Explicit individual IU rights omit FILE_CREATE_PIPE_INSTANCE, unlike GENERIC_WRITE.
        let sddl = wide("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x00100003;;;IU)");
        let mut descriptor = null_mut();
        unsafe {
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            ) == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let attributes = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            };
            let handle = CreateNamedPipeW(
                wide(PIPE).as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                MAX_FRAME as u32,
                MAX_FRAME as u32,
                0,
                &attributes,
            );
            LocalFree(descriptor);
            if handle == INVALID_HANDLE_VALUE {
                Err(format!(
                    "Cannot own System TCP control pipe: {}",
                    std::io::Error::last_os_error()
                ))
            } else {
                Ok(Self(Handle(handle)))
            }
        }
    }
    fn operation(
        &self,
        stop: &Handle,
        deadline: Option<Instant>,
        start: impl FnOnce(*mut OVERLAPPED) -> i32,
    ) -> Result<u32, String> {
        if stop.signaled() || deadline.is_some_and(|end| Instant::now() >= end) {
            return Err("System TCP pipe deadline expired".into());
        }
        let event = Handle::event()?;
        let mut overlapped = OVERLAPPED {
            hEvent: event.0,
            ..Default::default()
        };
        let ok = start(&mut overlapped);
        if ok == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_PIPE_CONNECTED {
                return Ok(0);
            }
            if error != ERROR_IO_PENDING {
                return Err(std::io::Error::from_raw_os_error(error as i32).to_string());
            }
            let remaining = deadline.map_or(INFINITE, |end| {
                end.saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(u128::from(u32::MAX - 1)) as u32
            });
            let handles = [event.0, stop.0];
            let result = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, remaining) };
            if result != WAIT_OBJECT_0 {
                // Cancel and reap before OVERLAPPED, event or buffers go out of scope.
                unsafe {
                    CancelIoEx(self.0.0, &overlapped);
                    let mut count = 0;
                    GetOverlappedResult(self.0.0, &overlapped, &mut count, 1);
                }
                return Err("System TCP pipe operation canceled or timed out".into());
            }
        }
        let mut count = 0;
        if unsafe { GetOverlappedResult(self.0.0, &overlapped, &mut count, 0) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(count)
    }
    pub fn accept(&self, stop: &Handle) -> Result<(), String> {
        self.operation(stop, None, |ov| unsafe { ConnectNamedPipe(self.0.0, ov) })
            .map(|_| ())
    }
    pub fn disconnect(&self) {
        unsafe {
            DisconnectNamedPipe(self.0.0);
        }
    }
    fn read_exact(&self, bytes: &mut [u8], stop: &Handle, end: Instant) -> Result<(), String> {
        let mut offset = 0;
        while offset < bytes.len() {
            let count = self.operation(stop, Some(end), |ov| unsafe {
                ReadFile(
                    self.0.0,
                    bytes[offset..].as_mut_ptr(),
                    (bytes.len() - offset) as u32,
                    null_mut(),
                    ov,
                )
            })? as usize;
            if count == 0 {
                return Err("System TCP pipe closed before completing a message".into());
            }
            offset += count;
        }
        Ok(())
    }
    pub fn read(&self, stop: &Handle, timeout: Duration) -> Result<Vec<u8>, String> {
        let end = Instant::now() + timeout;
        let mut length = [0; 4];
        self.read_exact(&mut length, stop, end)?;
        let count = u32::from_le_bytes(length) as usize;
        if count == 0 || count > MAX_FRAME {
            return Err("Invalid System TCP control frame length".into());
        }
        let mut bytes = vec![0; count];
        self.read_exact(&mut bytes, stop, end)?;
        Ok(bytes)
    }
    pub fn write(&self, bytes: &[u8], stop: &Handle) -> Result<(), String> {
        let end = Instant::now() + Duration::from_secs(2);
        let mut offset = 0;
        while offset < bytes.len() {
            let count = self.operation(stop, Some(end), |ov| unsafe {
                WriteFile(
                    self.0.0,
                    bytes[offset..].as_ptr(),
                    (bytes.len() - offset) as u32,
                    null_mut(),
                    ov,
                )
            })? as usize;
            if count == 0 {
                return Err("System TCP pipe write made no progress".into());
            }
            offset += count;
        }
        Ok(())
    }
    pub fn administrator(&self) -> bool {
        unsafe {
            if ImpersonateNamedPipeClient(self.0.0) == 0 {
                return false;
            }
            let mut sid = [0u64; 16];
            let mut size = size_of_val(&sid) as u32;
            let mut member = 0;
            let ok = CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                null_mut(),
                sid.as_mut_ptr().cast(),
                &mut size,
            ) != 0
                && CheckTokenMembership(null_mut(), sid.as_mut_ptr().cast(), &mut member) != 0
                && member != 0;
            // Never let a service thread continue with an unexpected impersonation token.
            if RevertToSelf() == 0 {
                std::process::abort();
            }
            ok
        }
    }
}
pub fn exchange(action: Action) -> Result<Response, String> {
    use std::os::windows::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new().access_mode(FILE_READ_DATA | FILE_WRITE_DATA | SYNCHRONIZE)
        .custom_flags(FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION).open(Path::new(PIPE))
        .map_err(|e| format!("System TCP service is not available: {e}. Install/start reviewed components separately; this app does not install or elevate."))?;
    let handle = file.as_raw_handle();
    // Verify the pipe endpoint belongs to the SCM-registered running service before sending any policy.
    unsafe {
        let scm = OpenSCManagerW(null(), null(), SC_MANAGER_CONNECT);
        if scm.is_null() {
            return Err("Cannot verify System TCP service identity".into());
        }
        let svc = OpenServiceW(scm, wide(SERVICE).as_ptr(), SERVICE_QUERY_STATUS);
        CloseServiceHandle(scm);
        if svc.is_null() {
            return Err("System TCP pipe has no matching installed service".into());
        }
        let mut status = SERVICE_STATUS_PROCESS::default();
        let mut needed = 0;
        let mut server_pid = 0;
        let valid = QueryServiceStatusEx(
            svc,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
            size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        ) != 0
            && status.dwCurrentState == SERVICE_RUNNING
            && GetNamedPipeServerProcessId(handle, &mut server_pid) != 0
            && server_pid != 0
            && status.dwProcessId == server_pid;
        CloseServiceHandle(svc);
        if !valid {
            return Err(
                "System TCP pipe server identity did not match the running SCM service".into(),
            );
        }
    }
    use std::os::windows::io::IntoRawHandle;
    let pipe = Pipe(Handle(file.into_raw_handle()));
    let stop = Handle::event()?;
    pipe.write(
        &crate::encode(&Request {
            version: VERSION,
            action,
        })?,
        &stop,
    )?;
    let payload = pipe.read(&stop, Duration::from_secs(38))?;
    let response: Response = serde_json::from_slice(&payload).map_err(|e| e.to_string())?;
    if response.version != VERSION || response.backend != crate::BACKEND {
        return Err("Incompatible System TCP service protocol".into());
    }
    // Acknowledge the response before the server disconnects its buffered pipe.
    pipe.write(&crate::encode(&true)?, &stop)?;
    Ok(response)
}
