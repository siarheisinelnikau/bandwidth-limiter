use std::time::{Duration, Instant};
/// Pure scheduling state, independent of any native window or renderer.
pub struct Scheduling {
    pub visible: bool,
    pub dirty: bool,
    pub deadline: Option<Instant>,
}
impl Scheduling {
    pub fn new() -> Self {
        Self {
            visible: true,
            dirty: true,
            deadline: None,
        }
    }
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
    pub fn visibility(&mut self, visible: bool) {
        self.visible = visible;
        self.dirty = true;
        if !visible {
            self.deadline = None;
        }
    }
    pub fn wake(&mut self, now: Instant, delay: Duration) {
        if !self.visible {
            self.dirty = true;
            return;
        }
        if delay.is_zero() {
            self.dirty = true;
        } else if delay != Duration::MAX
            && let Some(at) = now.checked_add(delay)
        {
            self.deadline = Some(self.deadline.map_or(at, |old| old.min(at)));
        }
    }
    pub fn due(&mut self, now: Instant) -> bool {
        if self.visible && self.deadline.is_some_and(|at| at <= now) {
            self.deadline = None;
            self.dirty = true;
        }
        self.visible && self.dirty
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hidden_wakes_do_not_schedule_frames_but_restore_is_dirty() {
        let mut s = Scheduling::new();
        s.visibility(false);
        for _ in 0..100 {
            s.wake(Instant::now(), Duration::ZERO);
            assert!(!s.due(Instant::now()));
        }
        assert!(s.deadline.is_none());
        s.visibility(true);
        assert!(s.due(Instant::now()));
    }
    #[test]
    fn finite_deadlines_settle_and_keep_the_earliest_request() {
        let now = Instant::now();
        let mut s = Scheduling::new();
        s.dirty = false;
        s.wake(now, Duration::from_secs(2));
        s.wake(now, Duration::from_secs(1));
        assert!(!s.due(now));
        assert!(s.due(now + Duration::from_secs(1)));
        s.dirty = false;
        assert!(!s.due(now + Duration::from_secs(5)));
        assert!(s.deadline.is_none());
    }
}
