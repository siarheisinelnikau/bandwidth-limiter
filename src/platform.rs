//! Read-only Windows IP Helper integration. Never enables ETW/EStats, filters or QoS.
use std::net::SocketAddr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    pub pid: u32,
    pub path: Option<String>,
    pub protocol: &'static str,
    pub local: SocketAddr,
    pub remote: Option<SocketAddr>,
    pub state: String,
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        ffi::c_void,
        mem::size_of,
        net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddrV6},
    };
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        NetworkManagement::IpHelper::*,
        System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
    };

    type Query = unsafe extern "system" fn(*mut c_void, *mut u32, i32, u32, i32, u32) -> u32;

    fn table<T: Copy>(query: Query, family: u32, class: i32) -> Result<Vec<T>, String> {
        let mut size = 0u32;
        // SAFETY: the documented size probe accepts a null buffer and a valid size pointer.
        let code = unsafe { query(std::ptr::null_mut(), &mut size, 0, family, class, 0) };
        if code != 0 && code != 122 {
            return Err(format!("IP Helper size query failed ({code})"));
        }
        for _ in 0..3 {
            if size > 16 * 1024 * 1024 {
                return Err("IP Helper table exceeds safety bound".into());
            }
            let mut buffer = vec![0u64; (size as usize).div_ceil(8).max(1)];
            // SAFETY: the aligned allocation covers size bytes; Windows updates size on a race.
            let code = unsafe { query(buffer.as_mut_ptr().cast(), &mut size, 0, family, class, 0) };
            if code == 122 {
                continue;
            }
            if code != 0 {
                return Err(format!("IP Helper query failed ({code})"));
            }
            let bytes = buffer.len() * 8;
            let count = buffer[0] as u32 as usize;
            if count > bytes.saturating_sub(4) / size_of::<T>() {
                return Err("Invalid IP Helper table length".into());
            }
            let mut rows = Vec::with_capacity(count);
            for index in 0..count {
                // SAFETY: the bounds above cover every row. These Windows table layouts have
                // a DWORD count followed by DWORD-aligned rows; unaligned reads avoid assumptions.
                rows.push(unsafe {
                    buffer
                        .as_ptr()
                        .cast::<u8>()
                        .add(4 + index * size_of::<T>())
                        .cast::<T>()
                        .read_unaligned()
                });
            }
            return Ok(rows);
        }
        Err("Connection table changed repeatedly; refresh again".into())
    }

    fn address(ip: u32, port: u32) -> SocketAddr {
        SocketAddr::new(
            IpAddr::V4(Ipv4Addr::from(ip.to_ne_bytes())),
            u16::from_be(port as u16),
        )
    }
    fn address6(ip: [u8; 16], scope: u32, port: u32) -> SocketAddr {
        SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::from(ip),
            u16::from_be(port as u16),
            0,
            scope,
        ))
    }

    fn process_path(pid: u32) -> Option<String> {
        // SAFETY: read-only query rights; each successful open is closed on every path.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buffer = vec![0u16; 32768];
            let mut length = buffer.len() as u32;
            let success = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length);
            CloseHandle(handle);
            (success != 0).then(|| String::from_utf16_lossy(&buffer[..length as usize]))
        }
    }

    pub fn owner(peer: SocketAddr, server: SocketAddr) -> Result<String, String> {
        let rows = table::<MIB_TCPROW_OWNER_PID>(GetExtendedTcpTable, 2, TCP_TABLE_OWNER_PID_ALL)?;
        let row = rows
            .iter()
            .find(|r| {
                address(r.dwLocalAddr, r.dwLocalPort) == peer
                    && address(r.dwRemoteAddr, r.dwRemotePort) == server
            })
            .ok_or("Cannot resolve the connecting process; connection refused")?;
        process_path(row.dwOwningPid)
            .ok_or("Cannot read executable identity; connection refused".into())
    }

    pub fn snapshot() -> Result<Vec<Connection>, String> {
        let mut result = Vec::new();
        for r in table::<MIB_TCPROW_OWNER_PID>(GetExtendedTcpTable, 2, TCP_TABLE_OWNER_PID_ALL)? {
            result.push(Connection {
                pid: r.dwOwningPid,
                path: None,
                protocol: "TCPv4",
                local: address(r.dwLocalAddr, r.dwLocalPort),
                remote: Some(address(r.dwRemoteAddr, r.dwRemotePort)),
                state: tcp_state(r.dwState).into(),
            });
        }
        for r in table::<MIB_TCP6ROW_OWNER_PID>(GetExtendedTcpTable, 23, TCP_TABLE_OWNER_PID_ALL)? {
            result.push(Connection {
                pid: r.dwOwningPid,
                path: None,
                protocol: "TCPv6",
                local: address6(r.ucLocalAddr, r.dwLocalScopeId, r.dwLocalPort),
                remote: Some(address6(r.ucRemoteAddr, r.dwRemoteScopeId, r.dwRemotePort)),
                state: tcp_state(r.dwState).into(),
            });
        }
        for r in table::<MIB_UDPROW_OWNER_PID>(GetExtendedUdpTable, 2, UDP_TABLE_OWNER_PID)? {
            result.push(Connection {
                pid: r.dwOwningPid,
                path: None,
                protocol: "UDPv4",
                local: address(r.dwLocalAddr, r.dwLocalPort),
                remote: None,
                state: "Endpoint".into(),
            });
        }
        for r in table::<MIB_UDP6ROW_OWNER_PID>(GetExtendedUdpTable, 23, UDP_TABLE_OWNER_PID)? {
            result.push(Connection {
                pid: r.dwOwningPid,
                path: None,
                protocol: "UDPv6",
                local: address6(r.ucLocalAddr, r.dwLocalScopeId, r.dwLocalPort),
                remote: None,
                state: "Endpoint".into(),
            });
        }
        result.truncate(4096);
        let mut paths = std::collections::HashMap::new();
        for r in &mut result {
            r.path = paths
                .entry(r.pid)
                .or_insert_with(|| process_path(r.pid))
                .clone();
        }
        result.sort_by(|a, b| a.path.cmp(&b.path).then(a.pid.cmp(&b.pid)));
        Ok(result)
    }

    fn tcp_state(state: u32) -> &'static str {
        match state {
            1 => "Closed",
            2 => "Listen",
            3 => "SYN sent",
            4 => "SYN received",
            5 => "Established",
            6 => "FIN wait 1",
            7 => "FIN wait 2",
            8 => "Close wait",
            9 => "Closing",
            10 => "Last ACK",
            11 => "Time wait",
            12 => "Delete TCB",
            _ => "Unknown",
        }
    }
}

#[cfg(windows)]
pub use windows::{owner, snapshot};
#[cfg(not(windows))]
pub fn snapshot() -> Result<Vec<Connection>, String> {
    Err("Windows IP Helper is unavailable on this platform".into())
}
#[cfg(not(windows))]
pub fn owner(_: SocketAddr, _: SocketAddr) -> Result<String, String> {
    Err("Windows process identity is unavailable".into())
}
