//! Preserve timed repaints while preventing eframe 0.33's leftover Poll state.
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::WindowId,
};

pub fn run(
    name: &str,
    options: eframe::NativeOptions,
    creator: eframe::AppCreator<'_>,
) -> eframe::Result {
    let events = EventLoop::<eframe::UserEvent>::with_user_event().build()?;
    events.set_control_flow(ControlFlow::Wait);
    let inner = eframe::create_native(name, options, creator, &events);
    events.run_app(&mut Reactive(inner))?;
    Ok(())
}

struct Reactive<'a>(eframe::EframeWinitApplication<'a>);
impl ApplicationHandler<eframe::UserEvent> for Reactive<'_> {
    fn resumed(&mut self, e: &ActiveEventLoop) {
        self.0.resumed(e);
    }
    fn suspended(&mut self, e: &ActiveEventLoop) {
        self.0.suspended(e);
    }
    fn exiting(&mut self, e: &ActiveEventLoop) {
        self.0.exiting(e);
    }
    fn memory_warning(&mut self, e: &ActiveEventLoop) {
        self.0.memory_warning(e);
    }
    fn new_events(&mut self, e: &ActiveEventLoop, cause: StartCause) {
        self.0.new_events(e, cause);
    }
    fn user_event(&mut self, e: &ActiveEventLoop, event: eframe::UserEvent) {
        self.0.user_event(e, event);
    }
    fn device_event(&mut self, e: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        self.0.device_event(e, id, event);
    }
    fn window_event(&mut self, e: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.0.window_event(e, id, event);
    }
    fn about_to_wait(&mut self, e: &ActiveEventLoop) {
        self.0.about_to_wait(e);
        // request_redraw already enqueues the frame. Keep WaitUntil deadlines intact.
        if e.control_flow() == ControlFlow::Poll {
            e.set_control_flow(ControlFlow::Wait);
        }
    }
}
