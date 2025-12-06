//! Cooperative shaping of TCP payloads. This is not a transparent system filter.
use crate::{
    model::Direction,
    policy::{Engine, Permit},
};
use chrono::{Datelike, Local, Timelike};
use std::{
    io::{self, Read, Write},
    net::{
        IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs,
    },
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_FLOWS: usize = 32;
type Identity = dyn Fn(SocketAddr, SocketAddr) -> Result<String, String> + Send + Sync;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub running: bool,
    pub active: usize,
    pub accepted: u64,
    pub rejected: u64,
    pub last_error: String,
    pub forwarded: std::collections::BTreeMap<String, [u64; 2]>,
}

pub struct Server {
    pub address: SocketAddr,
    stop: Arc<AtomicBool>,
    listener: Option<JoinHandle<()>>,
}

impl Server {
    pub fn start(
        port: u16,
        engine: Arc<Mutex<Engine>>,
        status: Arc<Mutex<Status>>,
    ) -> Result<Self, String> {
        #[cfg(not(windows))]
        {
            let _ = (port, engine, status);
            Err("Enforcement requires Windows process identity".into())
        }
        #[cfg(windows)]
        Self::with_identity(port, engine, status, Arc::new(crate::platform::owner))
    }

    fn with_identity(
        port: u16,
        engine: Arc<Mutex<Engine>>,
        status: Arc<Mutex<Status>>,
        identity: Arc<Identity>,
    ) -> Result<Self, String> {
        let socket = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .map_err(|e| format!("Cannot bind loopback proxy: {e}"))?;
        let address = socket.local_addr().map_err(|e| e.to_string())?;
        socket.set_nonblocking(true).map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let shutdown = stop.clone();
        status.lock().unwrap().running = true;
        let spawn_status = status.clone();
        let listener = thread::Builder::new()
            .name("proxy-accept".into())
            .spawn(move || {
                let live = Arc::new(AtomicUsize::new(0));
                let mut workers: Vec<JoinHandle<()>> = vec![];
                // A bounded timed wait also works if loopback wake connections are prohibited.
                loop {
                    if shutdown.load(Ordering::Acquire) {
                        break;
                    }
                    workers.retain(|w| !w.is_finished());
                    let client = match socket.accept() {
                        Ok((c, _)) => c,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            thread::park_timeout(Duration::from_millis(250));
                            continue;
                        }
                        Err(e) => {
                            status.lock().unwrap().last_error = e.to_string();
                            break;
                        }
                    };
                    if live.load(Ordering::Acquire) >= MAX_FLOWS {
                        status.lock().unwrap().rejected += 1;
                        continue;
                    }
                    let count = live.fetch_add(1, Ordering::AcqRel) + 1;
                    status.lock().unwrap().active = count;
                    let (engine, status, identity, stop, live) = (
                        engine.clone(),
                        status.clone(),
                        identity.clone(),
                        shutdown.clone(),
                        live.clone(),
                    );
                    let status_on_failure = status.clone();
                    let live_on_failure = live.clone();
                    match thread::Builder::new()
                        .name("proxy-flow".into())
                        .spawn(move || {
                            let result =
                                serve(client, address, engine, status.clone(), stop, identity);
                            let remaining = live.fetch_sub(1, Ordering::AcqRel) - 1;
                            let mut state = status.lock().unwrap();
                            state.active = remaining;
                            if let Err(error) = result {
                                state.rejected += 1;
                                state.last_error = error.to_string();
                            }
                        }) {
                        Ok(worker) => workers.push(worker),
                        Err(e) => {
                            let remaining = live_on_failure.fetch_sub(1, Ordering::AcqRel) - 1;
                            let mut state = status_on_failure.lock().unwrap();
                            state.active = remaining;
                            state.last_error = e.to_string();
                        }
                    }
                }
                for worker in workers {
                    let _ = worker.join();
                }
                let mut state = status.lock().unwrap();
                state.running = false;
                state.active = 0;
            })
            .map_err(|e| {
                spawn_status.lock().unwrap().running = false;
                e.to_string()
            })?;
        Ok(Self {
            address,
            stop,
            listener: Some(listener),
        })
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(listener) = self.listener.take() {
            listener.thread().unpark();
            let _ = listener.join();
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

fn failure(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

fn serve(
    mut client: TcpStream,
    server: SocketAddr,
    engine: Arc<Mutex<Engine>>,
    status: Arc<Mutex<Status>>,
    stop: Arc<AtomicBool>,
    identity: Arc<Identity>,
) -> io::Result<()> {
    client.set_read_timeout(Some(Duration::from_secs(3)))?;
    client.set_write_timeout(Some(Duration::from_secs(3)))?;
    let path = identity(client.peer_addr()?, server).map_err(failure)?;
    if !engine.lock().unwrap().matched(&path) {
        return Err(failure("Executable has no enabled rule; proxy refused it"));
    }
    let target = handshake(&mut client)?;
    let addresses: Vec<_> = target.to_socket_addrs()?.take(8).collect();
    let mut upstream = None;
    for address in addresses {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        if address.port() == server.port() && address.ip().is_loopback() {
            continue;
        }
        if let Ok(stream) = TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
            upstream = Some(stream);
            break;
        }
    }
    let Some(upstream) = upstream else {
        reply(&mut client, 5)?;
        return Err(failure(
            "Upstream connection failed or proxy loop was refused",
        ));
    };
    let bound = upstream.local_addr()?;
    let mut response = vec![5, 0, 0];
    match bound.ip() {
        IpAddr::V4(ip) => {
            response.push(1);
            response.extend(ip.octets());
        }
        IpAddr::V6(ip) => {
            response.push(4);
            response.extend(ip.octets());
        }
    }
    response.extend(bound.port().to_be_bytes());
    client.write_all(&response)?;
    for stream in [&client, &upstream] {
        stream.set_read_timeout(Some(Duration::from_millis(250)))?;
        stream.set_write_timeout(Some(Duration::from_millis(250)))?;
        stream.set_nodelay(true)?;
    }
    {
        let mut state = status.lock().unwrap();
        state.accepted += 1;
    }
    let down_client = client.try_clone()?;
    let down_upstream = upstream.try_clone()?;
    let cancel = Arc::new(AtomicBool::new(false));
    let context = Arc::new(Relay {
        engine,
        status,
        stop,
        cancel: cancel.clone(),
        path,
    });
    let down_context = context.clone();
    let downloader = thread::Builder::new()
        .name("proxy-download".into())
        .spawn(move || {
            pump(
                down_upstream,
                down_client,
                Direction::Download,
                down_context,
            )
        })?;
    let result = pump(client, upstream, Direction::Upload, context);
    let down_result = downloader
        .join()
        .map_err(|_| failure("Download worker stopped unexpectedly"))?;
    result.and(down_result)
}

fn reply(client: &mut impl Write, code: u8) -> io::Result<()> {
    client.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0])
}

fn handshake(client: &mut (impl Read + Write)) -> io::Result<(String, u16)> {
    let mut greeting = [0u8; 2];
    client.read_exact(&mut greeting)?;
    if greeting[0] != 5 || greeting[1] == 0 {
        return Err(failure("Expected SOCKS5 greeting"));
    }
    let mut methods = vec![0; greeting[1] as usize];
    client.read_exact(&mut methods)?;
    if !methods.contains(&0) {
        client.write_all(&[5, 255])?;
        return Err(failure(
            "Only SOCKS5 no-auth method is supported on loopback",
        ));
    }
    client.write_all(&[5, 0])?;
    let mut request = [0; 4];
    client.read_exact(&mut request)?;
    if request[0] != 5 || request[2] != 0 {
        return Err(failure("Invalid SOCKS5 request"));
    }
    if request[1] != 1 {
        reply(client, 7)?;
        return Err(failure(
            "Only TCP CONNECT is supported; UDP and BIND are unavailable",
        ));
    }
    let host = match request[3] {
        1 => {
            let mut ip = [0; 4];
            client.read_exact(&mut ip)?;
            IpAddr::V4(Ipv4Addr::from(ip)).to_string()
        }
        4 => {
            let mut ip = [0; 16];
            client.read_exact(&mut ip)?;
            IpAddr::V6(Ipv6Addr::from(ip)).to_string()
        }
        3 => {
            let mut length = [0];
            client.read_exact(&mut length)?;
            let mut name = vec![0; length[0] as usize];
            client.read_exact(&mut name)?;
            let name = String::from_utf8(name).map_err(|_| failure("Invalid hostname encoding"))?;
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            {
                return Err(failure("Invalid DNS hostname"));
            }
            name
        }
        _ => {
            reply(client, 8)?;
            return Err(failure("Unsupported SOCKS5 address type"));
        }
    };
    let mut port = [0; 2];
    client.read_exact(&mut port)?;
    let port = u16::from_be_bytes(port);
    if port == 0 {
        reply(client, 1)?;
        return Err(failure("Destination port must be nonzero"));
    }
    Ok((host, port))
}

struct Relay {
    engine: Arc<Mutex<Engine>>,
    status: Arc<Mutex<Status>>,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    path: String,
}

fn pump(
    mut source: TcpStream,
    mut destination: TcpStream,
    direction: Direction,
    context: Arc<Relay>,
) -> io::Result<()> {
    let mut buffer = [0u8; 4096];
    let result = (|| {
        loop {
            if context.stop.load(Ordering::Acquire) || context.cancel.load(Ordering::Acquire) {
                break;
            }
            let count = match source.read(&mut buffer) {
                Ok(0) => {
                    let _ = destination.shutdown(Shutdown::Write);
                    return Ok(());
                }
                Ok(n) => n,
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    continue;
                }
                Err(e) => return Err(e),
            };
            let mut offset = 0;
            while offset < count {
                if context.stop.load(Ordering::Acquire) || context.cancel.load(Ordering::Acquire) {
                    return Ok(());
                }
                let local = Local::now();
                let day = local.format("%Y-%m-%d").to_string();
                let now = monotonic_now();
                let permit = context.engine.lock().unwrap().reserve(
                    &context.path,
                    direction,
                    count - offset,
                    now,
                    &day,
                    local.weekday().num_days_from_monday() as u8,
                    (local.hour() * 60 + local.minute()) as u16,
                );
                match permit {
                    Permit::Wait => thread::park_timeout(Duration::from_millis(20)),
                    Permit::Unmatched => {
                        return Err(failure(
                            "Rule disabled or removed; existing flow was closed",
                        ));
                    }
                    Permit::QuotaReached => {
                        return Err(failure("Daily quota reached; existing flow was closed"));
                    }
                    Permit::Bytes(bytes) => {
                        match destination.write(&buffer[offset..offset + bytes]) {
                            Ok(written) => {
                                context.engine.lock().unwrap().refund(
                                    &context.path,
                                    direction,
                                    bytes - written,
                                    &day,
                                );
                                if written == 0 {
                                    return Err(failure("Upstream stopped accepting bytes"));
                                }
                                context
                                    .status
                                    .lock()
                                    .unwrap()
                                    .forwarded
                                    .entry(crate::model::app_key(&context.path))
                                    .or_default()[direction as usize] += written as u64;
                                offset += written;
                            }
                            Err(e) => {
                                context.engine.lock().unwrap().refund(
                                    &context.path,
                                    direction,
                                    bytes,
                                    &day,
                                );
                                if e.kind() != io::ErrorKind::WouldBlock
                                    && e.kind() != io::ErrorKind::TimedOut
                                {
                                    return Err(e);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    })();
    if result.is_err() || context.stop.load(Ordering::Acquire) {
        context.cancel.store(true, Ordering::Release);
        let _ = source.shutdown(Shutdown::Both);
        let _ = destination.shutdown(Shutdown::Both);
    }
    result
}

fn monotonic_now() -> Duration {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Config, Rule, Usage};
    struct Script {
        input: io::Cursor<Vec<u8>>,
        output: Vec<u8>,
    }
    impl Script {
        fn new(input: Vec<u8>) -> Self {
            Self {
                input: io::Cursor::new(input),
                output: vec![],
            }
        }
    }
    impl Read for Script {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            let count = b.len().min(1);
            self.input.read(&mut b[..count])
        }
    }
    impl Write for Script {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.output.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn fragmented_socks_requests_parse_without_network_access() {
        let mut dns = vec![5, 2, 2, 0, 5, 1, 0, 3, 12];
        dns.extend(b"example.test");
        dns.extend(443u16.to_be_bytes());
        let mut script = Script::new(dns);
        assert_eq!(
            handshake(&mut script).unwrap(),
            ("example.test".into(), 443)
        );
        assert_eq!(script.output, [5, 0]);
        let mut ipv6 = vec![5, 1, 0, 5, 1, 0, 4];
        ipv6.extend(Ipv6Addr::LOCALHOST.octets());
        ipv6.extend(80u16.to_be_bytes());
        assert_eq!(
            handshake(&mut Script::new(ipv6)).unwrap(),
            ("::1".into(), 80)
        );
        assert!(handshake(&mut Script::new(vec![5, 1, 0, 5, 1, 0, 1, 127])).is_err());
    }
    #[test]
    fn unsupported_protocol_and_authentication_are_rejected() {
        let mut udp = Script::new(vec![5, 1, 0, 5, 3, 0, 1]);
        assert!(handshake(&mut udp).is_err());
        assert_eq!(udp.output[3], 7);
        let mut auth = Script::new(vec![5, 1, 2]);
        assert!(handshake(&mut auth).is_err());
        assert_eq!(auth.output, [5, 255]);
    }
    fn fixture() -> (Arc<Mutex<Engine>>, Arc<Mutex<Status>>) {
        (
            Arc::new(Mutex::new(Engine::new(
                Config {
                    rules: vec![Rule {
                        path: r"C:\Fixture.exe".into(),
                        download_kib: 0,
                        upload_kib: 0,
                        ..Rule::default()
                    }],
                    ..Config::default()
                },
                Usage::default(),
            ))),
            Arc::new(Mutex::new(Status::default())),
        )
    }
    fn connect(proxy: SocketAddr, port: u16) -> TcpStream {
        let mut client = TcpStream::connect(proxy).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        client.write_all(&[5, 1, 0]).unwrap();
        let mut hello = [0; 2];
        client.read_exact(&mut hello).unwrap();
        assert_eq!(hello, [5, 0]);
        let mut request = vec![5, 1, 0, 1, 127, 0, 0, 1];
        request.extend(port.to_be_bytes());
        client.write_all(&request).unwrap();
        let mut response = [0; 10];
        client.read_exact(&mut response).unwrap();
        assert_eq!(response[1], 0);
        client
    }
    #[test]
    #[ignore = "Requires loopback socket permission; unavailable in the unattended sandbox"]
    fn forwards_loopback_payload_and_preserves_half_close() {
        let endpoint = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = endpoint.local_addr().unwrap().port();
        let endpoint_worker = thread::spawn(move || {
            let (mut socket, _) = endpoint.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            let mut request = Vec::new();
            socket.read_to_end(&mut request).unwrap();
            assert_eq!(request, b"fixture");
            socket.write_all(b"response").unwrap();
        });
        let (engine, status) = fixture();
        let mut proxy = Server::with_identity(
            0,
            engine,
            status.clone(),
            Arc::new(|_, _| Ok(r"C:\Fixture.exe".into())),
        )
        .unwrap();
        let mut client = connect(proxy.address, port);
        client.write_all(b"fixture").unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        assert_eq!(response, b"response");
        endpoint_worker.join().unwrap();
        proxy.stop();
        assert_eq!(
            status.lock().unwrap().forwarded[&crate::model::app_key(r"C:\Fixture.exe")],
            [8, 7]
        );
    }
    #[test]
    #[ignore = "Requires loopback socket permission; unavailable in the unattended sandbox"]
    fn unknown_identity_is_refused_before_connecting() {
        let (engine, status) = fixture();
        let mut proxy = Server::with_identity(
            0,
            engine,
            status.clone(),
            Arc::new(|_, _| Err("Fixture identity denied".into())),
        )
        .unwrap();
        let mut client = TcpStream::connect(proxy.address).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut byte = [0];
        assert_eq!(client.read(&mut byte).unwrap(), 0);
        proxy.stop();
        assert_eq!(status.lock().unwrap().rejected, 1);
    }
}
