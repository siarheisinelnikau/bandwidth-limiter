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
    pub fn repaint_callback(
        &mut self,
        now: Instant,
        delay: Duration,
        requested_pass: u64,
        completed_pass: u64,
    ) {
        // The completed frame's viewport output covers callbacks sent during that frame.
        if requested_pass >= completed_pass {
            self.wake(now, delay);
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

#[cfg(test)]
mod egui_regressions {
    use super::*;
    #[test]
    fn real_egui_callbacks_settle_after_discarding_consumed_passes() {
        let ctx = egui::Context::default();
        let (send, receive) = std::sync::mpsc::channel();
        ctx.set_request_repaint_callback(move |info| {
            let _ = send.send(info);
        });
        let mut schedule = Scheduling::new();
        let mut frames = 0;
        let mut queued = Vec::new();
        while (schedule.due(Instant::now()) || !queued.is_empty()) && frames < 16 {
            if schedule.due(Instant::now()) {
                schedule.dirty = false;
                schedule.deadline = None;
                let output = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.label("Settled utility");
                    });
                });
                frames += 1;
                schedule.wake(
                    Instant::now(),
                    output.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
                );
            }
            // Deliver callbacks a loop iteration late, as a native event queue can.
            let callbacks = std::mem::replace(&mut queued, receive.try_iter().collect());
            for callback in callbacks {
                schedule.repaint_callback(
                    Instant::now(),
                    callback.delay,
                    callback.current_cumulative_pass_nr,
                    ctx.cumulative_pass_nr(),
                );
            }
        }
        assert_eq!(
            frames, 2,
            "already consumed repaint callbacks must not add frames"
        );
        assert!(!schedule.dirty);
        ctx.request_repaint();
        for callback in receive.try_iter() {
            schedule.repaint_callback(
                Instant::now(),
                callback.delay,
                callback.current_cumulative_pass_nr,
                ctx.cumulative_pass_nr(),
            );
        }
        assert!(
            schedule.due(Instant::now()),
            "a new worker update still wakes the UI"
        );
    }
}
