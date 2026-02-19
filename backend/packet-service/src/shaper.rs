//! Deterministic bounded packet scheduler, independent of WinDivert and wall-clock APIs.
use crate::{
    abi::Address,
    packet::{self, FlowKey},
};
use bandwidth_limiter_packet_control::{Policy, Snapshot};
use std::collections::{HashMap, VecDeque};
pub const MAX_FLOWS: usize = 512;
pub const MAX_PACKETS: usize = 4096;
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_FLOW_PACKETS: usize = 128;
const MAX_AGE_US: u64 = 120_000_000;
#[derive(Debug)]
pub struct Packet {
    pub bytes: Vec<u8>,
    pub address: Address,
    pub payload: usize,
    pub direction: usize,
    pub selected: bool,
    queued_at: u64,
}
impl Packet {
    pub fn new(bytes: Vec<u8>, address: Address) -> Self {
        Self {
            bytes,
            address,
            payload: 0,
            direction: 0,
            selected: false,
            queued_at: 0,
        }
    }
}
#[derive(Clone, Copy)]
struct Bucket {
    rate: u64,
    credit: u128,
    last: u64,
}
impl Bucket {
    fn new(kib: u32) -> Self {
        Self {
            rate: u64::from(kib) * 1024,
            credit: 0,
            last: 0,
        }
    }
    fn refill(&mut self, now: u64) {
        self.credit = (self.credit
            + u128::from(now.saturating_sub(self.last)) * u128::from(self.rate))
        .min(u128::from((self.rate / 4).max(65536)) * 1_000_000);
        self.last = self.last.max(now);
    }
    fn wait(&mut self, bytes: usize, now: u64) -> u64 {
        self.refill(now);
        if self.rate == 0 {
            return 0;
        }
        let missing = (bytes as u128 * 1_000_000).saturating_sub(self.credit);
        missing.div_ceil(u128::from(self.rate)) as u64
    }
    fn take(&mut self, bytes: usize) {
        if self.rate != 0 {
            self.credit -= bytes as u128 * 1_000_000;
        }
    }
}
struct Flow {
    endpoint: u64,
    timestamp: i64,
    slot: usize,
    queues: [VecDeque<Packet>; 2],
}
pub struct Shaper {
    flows: HashMap<FlowKey, Flow>,
    active: VecDeque<(FlowKey, usize)>,
    budgets: Vec<[Bucket; 2]>,
    pub stats: Snapshot,
}
impl Shaper {
    pub fn new(policy: &Policy) -> Self {
        Self {
            flows: HashMap::new(),
            active: VecDeque::new(),
            budgets: policy
                .rules
                .iter()
                .map(|r| [Bucket::new(r.download_kib), Bucket::new(r.upload_kib)])
                .collect(),
            stats: Snapshot::default(),
        }
    }
    pub fn observe(&mut self, key: FlowKey, endpoint: u64, timestamp: i64, slot: usize) {
        if slot >= self.budgets.len()
            || self
                .flows
                .get(&key)
                .is_some_and(|f| f.timestamp >= timestamp)
        {
            return;
        }
        self.remove(key, None);
        if self.flows.len() >= MAX_FLOWS {
            return;
        }
        self.flows.insert(
            key,
            Flow {
                endpoint,
                timestamp,
                slot,
                queues: Default::default(),
            },
        );
        self.stats.flows = self.flows.len() as u32;
    }
    pub fn remove(&mut self, key: FlowKey, endpoint: Option<u64>) {
        if endpoint.is_some_and(|id| self.flows.get(&key).is_some_and(|f| f.endpoint != id)) {
            return;
        }
        if let Some(f) = self.flows.remove(&key) {
            for p in f.queues.into_iter().flatten() {
                self.release(&p);
                self.stats.dropped_packets += 1;
            }
            self.active.retain(|(k, _)| *k != key);
        }
        self.stats.flows = self.flows.len() as u32;
    }
    pub fn accept(&mut self, mut p: Packet, now: u64) -> Option<Packet> {
        let info = packet::parse(&p.bytes, p.address.outbound());
        let Some(info) = info else {
            self.stats.unattributed_packets += 1;
            return Some(p);
        };
        let Some(flow) = self
            .flows
            .get_mut(&info.key)
            .filter(|f| p.address.timestamp >= f.timestamp)
        else {
            self.stats.unattributed_packets += 1;
            return Some(p);
        };
        if self.stats.queued_packets as usize >= MAX_PACKETS
            || self.stats.queued_bytes as usize + p.bytes.len() > MAX_BYTES
            || flow.queues.iter().map(VecDeque::len).sum::<usize>() >= MAX_FLOW_PACKETS
        {
            self.stats.dropped_packets += 1;
            return None;
        }
        p.payload = info.payload;
        p.direction = info.direction;
        p.selected = true;
        p.queued_at = now;
        let queue = &mut flow.queues[info.direction];
        if queue.is_empty() {
            self.active.push_back((info.key, info.direction));
        }
        self.stats.queued_packets += 1;
        self.stats.queued_bytes += p.bytes.len() as u64;
        queue.push_back(p);
        None
    }
    fn release(&mut self, p: &Packet) {
        self.stats.queued_packets -= 1;
        self.stats.queued_bytes -= p.bytes.len() as u64;
    }
    pub fn pop_ready(&mut self, now: u64, flush: bool) -> Option<Packet> {
        for _ in 0..self.active.len() {
            let (key, d) = self.active.pop_front()?;
            let flow = self.flows.get_mut(&key).unwrap();
            while flow.queues[d]
                .front()
                .is_some_and(|p| now.saturating_sub(p.queued_at) >= MAX_AGE_US)
            {
                let expired = flow.queues[d].pop_front().unwrap();
                self.stats.dropped_packets += 1;
                self.stats.queued_packets -= 1;
                self.stats.queued_bytes -= expired.bytes.len() as u64;
            }
            let Some(front) = flow.queues[d].front() else {
                continue;
            };
            if flush || self.budgets[flow.slot][d].wait(front.payload, now) == 0 {
                let p = flow.queues[d].pop_front().unwrap();
                if !flush {
                    self.budgets[flow.slot][d].take(p.payload);
                }
                if !flow.queues[d].is_empty() {
                    self.active.push_back((key, d));
                }
                self.release(&p);
                return Some(p);
            }
            self.active.push_back((key, d));
        }
        None
    }
    pub fn wait_us(&mut self, now: u64) -> Option<u64> {
        self.active
            .iter()
            .filter_map(|(key, d)| {
                let f = self.flows.get(key)?;
                let p = f.queues[*d].front()?;
                Some(
                    self.budgets[f.slot][*d]
                        .wait(p.payload, now)
                        .min(MAX_AGE_US.saturating_sub(now.saturating_sub(p.queued_at))),
                )
            })
            .min()
    }
    pub fn sent(&mut self, p: &Packet, success: bool) {
        if !success {
            self.stats.send_errors += 1;
            self.stats.dropped_packets += 1;
        } else if p.selected {
            if p.direction == 0 {
                self.stats.download_submitted += p.payload as u64;
            } else {
                self.stats.upload_submitted += p.payload as u64;
            }
        }
    }
    pub fn discard_remaining(&mut self) {
        self.stats.dropped_packets += u64::from(self.stats.queued_packets);
        self.stats.queued_packets = 0;
        self.stats.queued_bytes = 0;
        self.flows.clear();
        self.active.clear();
        self.stats.flows = 0;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use bandwidth_limiter_packet_control::{Rule, VERSION};
    fn policy() -> Policy {
        Policy {
            version: VERSION,
            rules: vec![Rule {
                executable: r"C:\App.exe".into(),
                download_kib: 4,
                upload_kib: 8,
            }],
        }
    }
    fn pkt(size: usize, up: bool, port: u16, id: i64) -> Packet {
        let mut b = packet::fixture(size, up, port);
        b[24..28].copy_from_slice(&(id as u32).to_be_bytes());
        Packet::new(
            b,
            Address {
                timestamp: 2,
                flags: if up { 1 << 17 } else { 0 },
                ..Default::default()
            },
        )
    }
    fn register(s: &mut Shaper, port: u16, id: u64) {
        s.observe(
            packet::parse(&packet::fixture(0, true, port), true)
                .unwrap()
                .key,
            id,
            1,
            0,
        );
    }
    #[test]
    fn aggregate_independent_budgets_fifo_and_zero_token_start() {
        let mut s = Shaper::new(&policy());
        for port in 1000..1008 {
            register(&mut s, port, u64::from(port));
        }
        for n in 0..40 {
            for port in 1000..1008 {
                s.accept(pkt(1024, false, port, n), 0);
                s.accept(pkt(1024, true, port, n), 0);
            }
        }
        assert!(s.pop_ready(0, false).is_none());
        let mut sequences = HashMap::new();
        for tick in 0..=1000 {
            while let Some(p) = s.pop_ready(tick * 10_000, false) {
                let info = packet::parse(&p.bytes, p.address.outbound()).unwrap();
                let sequence = u32::from_be_bytes(p.bytes[24..28].try_into().unwrap());
                let prev = sequences.entry((info.key, info.direction)).or_insert(0);
                assert_eq!(*prev, sequence);
                *prev += 1;
                s.sent(&p, true);
            }
        }
        assert_eq!(s.stats.download_submitted, 40960);
        assert_eq!(s.stats.upload_submitted, 81920);
    }
    #[test]
    fn attribution_generation_limits_expiry_and_pause_flush() {
        let mut s = Shaper::new(&policy());
        let p = pkt(100, true, 1000, 0);
        assert!(s.accept(p, 0).is_some());
        register(&mut s, 1000, 7);
        let key = packet::parse(&packet::fixture(0, true, 1000), true)
            .unwrap()
            .key;
        for n in 0..130 {
            s.accept(pkt(100, true, 1000, n), 0);
        }
        assert_eq!(s.stats.queued_packets, 128);
        assert_eq!(s.stats.dropped_packets, 2);
        s.remove(key, Some(6));
        assert_eq!(s.stats.flows, 1);
        let p = s.pop_ready(0, true).unwrap();
        s.sent(&p, true);
        assert_eq!(s.stats.upload_submitted, 100);
        while s.pop_ready(MAX_AGE_US, false).is_some() {}
        assert_eq!(s.stats.queued_packets, 0);
        s.accept(pkt(100, true, 1000, 0), MAX_AGE_US);
        s.observe(key, 8, 3, 0);
        let p = pkt(100, true, 1000, 0);
        assert!(s.accept(p, MAX_AGE_US).is_some());
        s.discard_remaining();
        assert_eq!(s.stats.queued_bytes, 0);
    }
    #[test]
    fn controls_preserve_order_and_zero_direction_is_unlimited() {
        let mut p = policy();
        p.rules[0].upload_kib = 0;
        let mut s = Shaper::new(&p);
        register(&mut s, 1000, 1);
        s.accept(pkt(100, true, 1000, 0), 0);
        assert!(s.pop_ready(0, false).is_some());
        s.accept(pkt(100, false, 1000, 0), 0);
        s.accept(pkt(0, false, 1000, 1), 0);
        assert!(s.pop_ready(0, false).is_none());
        assert_eq!(s.pop_ready(25_000, false).unwrap().payload, 100);
        assert_eq!(s.pop_ready(25_000, false).unwrap().payload, 0);
        assert!(s.wait_us(25_000).is_none());
    }
}
