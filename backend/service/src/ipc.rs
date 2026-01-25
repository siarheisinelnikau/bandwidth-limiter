use bandwidth_limiter_control::{
    self as control, Action, Request, Response,
    transport::{Handle, Pipe},
};
use std::{
    sync::{
        Arc,
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
pub struct Incoming {
    pub request: Request,
    pub administrator: bool,
    pub reply: SyncSender<Response>,
}
pub struct Server {
    stop: Arc<Handle>,
    worker: Option<JoinHandle<()>>,
    pub incoming: Receiver<Incoming>,
}
impl Server {
    pub fn new() -> Result<Self, String> {
        let pipe = Pipe::server()?;
        let stop = Arc::new(Handle::event()?);
        let worker_stop = stop.clone();
        let (tx, incoming) = mpsc::sync_channel(4);
        let worker = thread::Builder::new().name("wfp-control-pipe".into()).spawn(move || {
            while !worker_stop.signaled() {
                if pipe.accept(&worker_stop).is_err() { break; }
                let response = (|| {
                    let payload = pipe.read(&worker_stop, Duration::from_secs(2))?;
                    let request = control::decode_request(&payload)?;
                    let administrator = pipe.administrator();
                    if !matches!(request.action, Action::Status {}) && !administrator { return Err("Administrator authorization is required to change WFP limits. No elevation is started automatically.".into()); }
                    let (reply, result) = mpsc::sync_channel(1);
                    tx.try_send(Incoming { request, administrator, reply }).map_err(|_| "WFP controller is busy".to_string())?;
                    let deadline = Instant::now() + Duration::from_secs(35);
                    loop { match result.recv_timeout(Duration::from_millis(100)) {
                        Ok(response) => return Ok(response),
                        Err(mpsc::RecvTimeoutError::Disconnected) => return Err("WFP controller stopped".into()),
                        Err(_) if worker_stop.signaled() || Instant::now() >= deadline => return Err("WFP control request timed out".into()),
                        Err(_) => {}
                    }}
                })();
                let response = response.unwrap_or_else(|error: String| Response { version: control::VERSION, can_configure: false, status: None, error: Some(error) });
                if let Ok(frame) = control::encode(&response)
                    && pipe.write(&frame, &worker_stop).is_ok() { let _ = pipe.read(&worker_stop, Duration::from_secs(2)); }
                pipe.disconnect();
            }
        }).map_err(|e| e.to_string())?;
        Ok(Self {
            stop,
            worker: Some(worker),
            incoming,
        })
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.signal();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
