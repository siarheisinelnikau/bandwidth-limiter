//! Explicit WinDivert loading only. NO_INSTALL is mandatory for every handle.
use crate::{abi::Address, security, shaper::Packet};
use bandwidth_limiter_packet_control::transport::Handle;
use std::{
    ffi::{CString, c_void},
    path::PathBuf,
    ptr::null_mut,
    sync::Arc,
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::*,
    System::{IO::*, LibraryLoader::*, Threading::*},
};
type Open = unsafe extern "C" fn(*const u8, i32, i16, u64) -> HANDLE;
type Recv = unsafe extern "C" fn(
    HANDLE,
    *mut c_void,
    u32,
    *mut u32,
    u64,
    *mut Address,
    *mut u32,
    *mut OVERLAPPED,
) -> i32;
type Send = unsafe extern "C" fn(
    HANDLE,
    *const c_void,
    u32,
    *mut u32,
    u64,
    *const Address,
    u32,
    *mut OVERLAPPED,
) -> i32;
type Shutdown = unsafe extern "C" fn(HANDLE, i32) -> i32;
type Close = unsafe extern "C" fn(HANDLE) -> i32;
type Checksums = unsafe extern "C" fn(*mut c_void, u32, *mut Address, u64) -> i32;
type GetParam = unsafe extern "C" fn(HANDLE, i32, *mut u64) -> i32;
type SetParam = unsafe extern "C" fn(HANDLE, i32, u64) -> i32;
struct Module(HMODULE);
impl Drop for Module {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.0);
        }
    }
}
pub struct Api {
    _module: Module,
    open: Open,
    recv: Recv,
    send: Send,
    shutdown: Shutdown,
    close: Close,
    checksums: Checksums,
    get: GetParam,
    set: SetParam,
}
// SAFETY: an immutable loaded module/function table stays alive through Arc; APIs use separate handles/OVERLAPPEDs.
unsafe impl std::marker::Send for Api {}
// SAFETY: WinDivert supports concurrent calls; no mutable library-owned application buffers are shared here.
unsafe impl Sync for Api {}
fn last_error(context: &str) -> String {
    format!("{context}: {}", std::io::Error::last_os_error())
}
pub fn directory() -> Result<PathBuf, String> {
    let root =
        PathBuf::from(std::env::var_os("ProgramFiles").ok_or("ProgramFiles is unavailable")?)
            .join("BandwidthLimiterPacket");
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if exe.parent().is_none_or(|p| {
        p.as_os_str().to_string_lossy().to_lowercase()
            != root.as_os_str().to_string_lossy().to_lowercase()
    }) {
        return Err("Stage the service and reviewed WinDivert runtime in Program Files/BandwidthLimiterPacket with protected ACLs before Apply".into());
    }
    for p in [
        &root,
        &exe,
        &root.join("WinDivert.dll"),
        &root.join("WinDivert64.sys"),
    ] {
        security::require_admin_owned(p)?;
    }
    Ok(root)
}
impl Api {
    pub fn load() -> Result<Arc<Self>, String> {
        use std::os::windows::ffi::OsStrExt;
        let path = directory()?.join("WinDivert.dll");
        let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let module = Module(unsafe {
            LoadLibraryExW(
                wide.as_ptr(),
                null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        });
        if module.0.is_null() {
            return Err(last_error("Cannot load protected x64 WinDivert.dll"));
        }
        unsafe fn symbol<T: Copy>(module: &Module, name: &str) -> Result<T, String> {
            let name = CString::new(name).unwrap();
            let address = unsafe { GetProcAddress(module.0, name.as_ptr().cast()) }
                .ok_or_else(|| last_error("Missing WinDivert 2.2 export"))?;
            assert_eq!(size_of::<T>(), size_of_val(&address));
            // SAFETY: each caller supplies the documented WinDivert 2.2 C signature for this exact export.
            Ok(unsafe { std::mem::transmute_copy(&address) })
        }
        // SAFETY: no function is called while resolving symbols; Module is retained by the function table.
        unsafe {
            Ok(Arc::new(Self {
                open: symbol(&module, "WinDivertOpen")?,
                recv: symbol(&module, "WinDivertRecvEx")?,
                send: symbol(&module, "WinDivertSendEx")?,
                shutdown: symbol(&module, "WinDivertShutdown")?,
                close: symbol(&module, "WinDivertClose")?,
                checksums: symbol(&module, "WinDivertHelperCalcChecksums")?,
                get: symbol(&module, "WinDivertGetParam")?,
                set: symbol(&module, "WinDivertSetParam")?,
                _module: module,
            }))
        }
    }
    pub fn open(self: &Arc<Self>, flow: bool) -> Result<Divert, String> {
        let filter = if flow {
            c"tcp and !loopback"
        } else {
            c"tcp and !loopback and !impostor"
        };
        // NO_INSTALL forbids WinDivert's implicit SCM registration/start fallback.
        let handle = unsafe {
            (self.open)(
                filter.as_ptr().cast(),
                if flow { 2 } else { 0 },
                0,
                0x10 | if flow { 0x1 | 0x4 } else { 0 },
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(last_error(
                "Cannot open WinDivert (NO_INSTALL); separately prepare/start the signed driver",
            ));
        }
        let divert = Divert {
            handle,
            api: self.clone(),
        };
        let (mut major, mut minor) = (0, 0);
        unsafe {
            if (self.get)(handle, 3, &mut major) == 0
                || (self.get)(handle, 4, &mut minor) == 0
                || major != 2
                || minor != 2
            {
                return Err("WinDivert driver 2.2 is required".into());
            }
            if (self.set)(handle, 0, 4096) == 0
                || (self.set)(handle, 1, 2000) == 0
                || (self.set)(handle, 2, 8 * 1024 * 1024) == 0
            {
                return Err(last_error("Cannot bound WinDivert kernel queues"));
            }
        }
        Ok(divert)
    }
}
pub struct Divert {
    pub handle: HANDLE,
    api: Arc<Api>,
}
impl Drop for Divert {
    fn drop(&mut self) {
        unsafe {
            (self.api.close)(self.handle);
        }
    }
}
impl Divert {
    pub fn shutdown_receive(&self) -> Result<(), String> {
        if unsafe { (self.api.shutdown)(self.handle, 1) } == 0 {
            Err(last_error("Cannot stop packet capture"))
        } else {
            Ok(())
        }
    }
    pub fn send(&self, p: &mut Packet, deadline: Instant) -> Result<(), String> {
        if unsafe {
            (self.api.checksums)(
                p.bytes.as_mut_ptr().cast(),
                p.bytes.len() as u32,
                &mut p.address,
                0,
            )
        } == 0
        {
            return Err("Cannot calculate packet checksums".into());
        }
        let event = Handle::event()?;
        let mut ov = OVERLAPPED {
            hEvent: event.0,
            ..Default::default()
        };
        let mut count = 0;
        let ok = unsafe {
            (self.api.send)(
                self.handle,
                p.bytes.as_ptr().cast(),
                p.bytes.len() as u32,
                &mut count,
                0,
                &p.address,
                80,
                &mut ov,
            )
        };
        if ok == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_IO_PENDING {
                return Err(format!("WinDivert send failed ({error})"));
            }
            let wait = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(250) as u32;
            if unsafe { WaitForSingleObject(event.0, wait) } != WAIT_OBJECT_0 {
                unsafe {
                    CancelIoEx(self.handle, &ov);
                    GetOverlappedResult(self.handle, &ov, &mut count, 1);
                }
                return Err("Packet injection timed out; canceled and reaped".into());
            }
        }
        if unsafe { GetOverlappedResult(self.handle, &ov, &mut count, 0) } == 0
            || count as usize != p.bytes.len()
        {
            return Err(last_error("Incomplete packet injection"));
        }
        Ok(())
    }
}
pub struct Read<'a> {
    divert: &'a Divert,
    pub event: Handle,
    ov: Box<OVERLAPPED>,
    address: Box<Address>,
    address_len: Box<u32>,
    count: Box<u32>,
    buffer: Vec<u8>,
    pending: bool,
}
impl<'a> Read<'a> {
    pub fn new(divert: &'a Divert, flow: bool) -> Result<Self, String> {
        Ok(Self {
            divert,
            event: Handle::event()?,
            ov: Box::default(),
            address: Box::default(),
            address_len: Box::new(80),
            count: Box::new(0),
            buffer: vec![0; if flow { 0 } else { 65575 }],
            pending: false,
        })
    }
    pub fn submit(&mut self) -> Result<(), u32> {
        assert!(!self.pending);
        unsafe {
            ResetEvent(self.event.0);
        }
        *self.ov = OVERLAPPED {
            hEvent: self.event.0,
            ..Default::default()
        };
        *self.address = Address::default();
        *self.address_len = 80;
        *self.count = 0;
        let data = if self.buffer.is_empty() {
            null_mut()
        } else {
            self.buffer.as_mut_ptr().cast()
        };
        let ok = unsafe {
            (self.divert.api.recv)(
                self.divert.handle,
                data,
                self.buffer.len() as u32,
                &mut *self.count,
                0,
                &mut *self.address,
                &mut *self.address_len,
                &mut *self.ov,
            )
        };
        if ok == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_IO_PENDING {
                return Err(error);
            }
        }
        self.pending = true;
        if ok != 0 {
            self.event.signal();
        }
        Ok(())
    }
    pub fn take(&mut self) -> Result<Option<(Vec<u8>, Address)>, u32> {
        if !self.pending || !self.event.signaled() {
            return Ok(None);
        }
        let mut count = 0;
        let result = unsafe { GetOverlappedResult(self.divert.handle, &*self.ov, &mut count, 0) };
        if result == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_IO_INCOMPLETE {
                return Ok(None);
            }
            self.pending = false;
            return Err(error);
        }
        self.pending = false;
        if *self.address_len != 80 || count as usize > self.buffer.len() {
            return Err(ERROR_INVALID_DATA);
        }
        Ok(Some((
            self.buffer[..count as usize].to_vec(),
            *self.address,
        )))
    }
    pub fn pending(&self) -> bool {
        self.pending
    }
    pub fn cancel(&mut self) -> bool {
        let mut received = false;
        if self.pending {
            unsafe {
                CancelIoEx(self.divert.handle, &*self.ov);
                let mut count = 0;
                received = GetOverlappedResult(self.divert.handle, &*self.ov, &mut count, 1) != 0;
            }
            self.pending = false;
        }
        received
    }
}
impl Drop for Read<'_> {
    fn drop(&mut self) {
        let _ = self.cancel();
    }
}
/// Events use QPC; process creation uses FILETIME. Sample both to reject PID reuse.
pub fn process_identity(pid: u32, event_qpc: i64) -> Option<String> {
    use windows_sys::Win32::System::{Performance::*, SystemInformation::*};
    unsafe {
        let (mut now, mut frequency) = (0, 0);
        let mut wall = FILETIME::default();
        GetSystemTimePreciseAsFileTime(&mut wall);
        if QueryPerformanceCounter(&mut now) == 0
            || QueryPerformanceFrequency(&mut frequency) == 0
            || frequency <= 0
            || event_qpc < 0
            || event_qpc > now
        {
            return None;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let process = Handle(handle);
        let (mut creation, mut exit, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        if GetProcessTimes(process.0, &mut creation, &mut exit, &mut kernel, &mut user) == 0 {
            return None;
        }
        let ticks = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
        let elapsed = ((now - event_qpc) as u128 * 10_000_000 / frequency as u128)
            .min(u128::from(u64::MAX)) as u64;
        if ticks(creation) > ticks(wall).saturating_sub(elapsed) {
            return None;
        }
        let mut path = vec![0u16; 32768];
        let mut count = path.len() as u32;
        if QueryFullProcessImageNameW(process.0, 0, path.as_mut_ptr(), &mut count) == 0 {
            return None;
        }
        String::from_utf16(&path[..count as usize]).ok()
    }
}
