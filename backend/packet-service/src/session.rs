//! One event-driven capture/pacing worker per active policy; status never opens a driver.
use crate::{
    abi::Address,
    native::{self, Api, Read},
    packet::FlowKey,
    shaper::{Packet, Shaper},
};
use bandwidth_limiter_packet_control::{Policy, Snapshot, transport::Handle};
use std::{
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows_sys::Win32::{Foundation::*, System::Threading::*};
pub struct Session {
    stop: Arc<Handle>,
    worker: Option<JoinHandle<()>>,
}
pub type Shared = Arc<Mutex<Snapshot>>;
fn micros(start: Instant) -> u64 {
    start.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}
fn key(address: Address) -> FlowKey {
    let f = address.flow();
    FlowKey {
        local: f.local,
        remote: f.remote,
        local_port: f.local_port,
        remote_port: f.remote_port,
        ipv6: address.ipv6(),
    }
}
fn publish(shape: &Shaper, shared: &Shared, active: bool, error: Option<String>) {
    let mut s = shape.stats.clone();
    s.active = active;
    s.last_error = error;
    *shared.lock().unwrap() = s;
}
impl Session {
    pub fn start(api: Arc<Api>, policy: Policy, shared: Shared) -> Result<Self, String> {
        let stop = Arc::new(Handle::event()?);
        let worker_stop = stop.clone();
        let (ready, rx) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("packet-capture-pacer".into())
            .spawn(move || {
                let result = run(api, policy, &worker_stop, &shared, &ready);
                if let Err(error) = result {
                    let _ = ready.try_send(Err(error.clone()));
                    let mut s = shared.lock().unwrap();
                    s.active = false;
                    s.last_error = Some(error);
                }
            })
            .map_err(|e| e.to_string())?;
        let mut session = Self {
            stop,
            worker: Some(worker),
        };
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(session),
            Ok(Err(error)) => {
                session.stop();
                Err(error)
            }
            Err(_) => {
                session.stop();
                Err("WinDivert activation did not complete within 10 seconds".into())
            }
        }
    }
    pub fn stop(&mut self) {
        self.stop.signal();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}
fn run(
    api: Arc<Api>,
    policy: Policy,
    stop: &Handle,
    shared: &Shared,
    ready: &mpsc::SyncSender<Result<(), String>>,
) -> Result<(), String> {
    // FLOW opens before NETWORK so new connections can be identified as early as possible.
    let flow = api.open(true)?;
    let mut flows = Read::new(&flow, true)?;
    flows
        .submit()
        .map_err(|e| format!("FLOW receive failed ({e})"))?;
    let network = api.open(false)?;
    let mut packets = Read::new(&network, false)?;
    packets
        .submit()
        .map_err(|e| format!("NETWORK receive failed ({e})"))?;
    let start = Instant::now();
    let mut last_publish = Instant::now();
    let mut shape = Shaper::new(&policy);
    let mut last_stats = shape.stats.clone();
    publish(&shape, shared, true, None);
    let _ = ready.send(Ok(()));
    let handles = [stop.0, flows.event.0, packets.event.0];
    let processing = (|| {
        while !stop.signaled() {
            // At most one flow and one packet receive per turn; neither stream can starve the other.
            if let Some((_, address)) = flows
                .take()
                .map_err(|e| format!("FLOW read failed ({e})"))?
            {
                if address.flags & 255 != 2 {
                    return Err("Unexpected WinDivert FLOW address".into());
                }
                let f = address.flow();
                if f.protocol == 6 {
                    if address.event() == 2 {
                        shape.remove(key(address), Some(f.endpoint));
                    } else if address.event() == 1
                        && let Some(path) = native::process_identity(f.pid, address.timestamp)
                        && let Some(slot) = policy.rules.iter().position(|r| {
                            r.executable.replace('/', "\\").to_lowercase() == path.to_lowercase()
                        })
                    {
                        shape.observe(key(address), f.endpoint, address.timestamp, slot);
                    }
                }
                flows
                    .submit()
                    .map_err(|e| format!("FLOW resubmit failed ({e})"))?;
            }
            if let Some((bytes, address)) = packets
                .take()
                .map_err(|e| format!("NETWORK read failed ({e})"))?
            {
                if address.flags & 255 != 0 || address.event() != 0 {
                    return Err("Unexpected WinDivert NETWORK address".into());
                }
                if let Some(mut p) = shape.accept(Packet::new(bytes, address), micros(start)) {
                    let result = network.send(&mut p, Instant::now() + Duration::from_millis(250));
                    shape.sent(&p, result.is_ok());
                    result?;
                }
                packets
                    .submit()
                    .map_err(|e| format!("NETWORK resubmit failed ({e})"))?;
            }
            for _ in 0..64 {
                if stop.signaled() {
                    break;
                }
                let Some(mut p) = shape.pop_ready(micros(start), false) else {
                    break;
                };
                let result = network.send(&mut p, Instant::now() + Duration::from_millis(250));
                shape.sent(&p, result.is_ok());
                result?;
            }
            let dirty = shape.stats != last_stats;
            if dirty && last_publish.elapsed() >= Duration::from_millis(200) {
                publish(&shape, shared, true, None);
                last_stats = shape.stats.clone();
                last_publish = Instant::now();
            }
            // No work => an infinite event wait. Token waits use the next actual deadline, rounded up.
            let mut timeout = shape.wait_us(micros(start)).map_or(INFINITE, |us| {
                us.div_ceil(1000).min(u64::from(u32::MAX - 1)) as u32
            });
            if shape.stats != last_stats {
                let remaining = Duration::from_millis(200).saturating_sub(last_publish.elapsed());
                timeout = timeout.min(remaining.as_millis().max(1) as u32);
            }
            let result = unsafe {
                WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, timeout)
            };
            if result == WAIT_FAILED {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        Ok(())
    })();
    let shutdown = network.shutdown_receive();
    // Stop flow monitoring, but reap the completed packet read before releasing its buffer.
    let _ = flows.cancel();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut cleanup_error = None;
    // Flush queued selected packets first, preserving each flow/direction FIFO; caps cease on Pause.
    while Instant::now() < deadline {
        let Some(mut p) = shape.pop_ready(micros(start), true) else {
            break;
        };
        let result = network.send(&mut p, deadline);
        shape.sent(&p, result.is_ok());
        if let Err(error) = result {
            cleanup_error = Some(error);
            break;
        }
    }
    // Drain already captured kernel packets without pacing. New packets now bypass this handle.
    while Instant::now() < deadline && shutdown.is_ok() {
        if !packets.pending() {
            match packets.submit() {
                Ok(()) => {}
                Err(ERROR_NO_DATA) => break,
                Err(e) => {
                    cleanup_error = Some(format!("Drain receive failed ({e})"));
                    break;
                }
            }
        }
        match packets.take() {
            Ok(Some((bytes, address))) => {
                let mut p = Packet::new(bytes, address);
                let result = network.send(&mut p, deadline);
                shape.sent(&p, result.is_ok());
                if let Err(error) = result {
                    cleanup_error = Some(error);
                    break;
                }
                match packets.submit() {
                    Ok(()) => {}
                    Err(ERROR_NO_DATA) => break,
                    Err(e) => {
                        cleanup_error = Some(format!("Drain receive failed ({e})"));
                        break;
                    }
                }
            }
            Ok(None) => {
                let remaining = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(50) as u32;
                unsafe {
                    WaitForSingleObject(packets.event.0, remaining);
                }
            }
            Err(ERROR_NO_DATA) => break,
            Err(e) => {
                cleanup_error = Some(format!("Drain read failed ({e})"));
                break;
            }
        }
    }
    if packets.cancel() {
        shape.stats.dropped_packets += 1;
    }
    if shape.stats.queued_packets != 0 {
        cleanup_error =
            Some("Pause flush exceeded its bound; remaining owned packets were discarded".into());
    }
    shape.discard_remaining();
    let result = processing
        .and(shutdown)
        .and(cleanup_error.map_or(Ok(()), Err));
    publish(&shape, shared, false, result.as_ref().err().cloned());
    result
}
