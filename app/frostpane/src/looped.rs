//! The event loop off Windows (porting design 3.4.1). winit's macOS, X11 and
//! Wayland loops have no timer for [`crate::core::Core::tick`], and an egui
//! pass every tick would undo the 1 s hidden repaint cadence. So `main`
//! builds the loop itself, runs eframe's application inside this wrapper and
//! runs the tick slot about 64 times a second (the cadence the Windows USER
//! timer gives) between eframe's own wake-ups.

use fp_ui::eframe::{self, UserEvent};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

/// The tick the wrapper runs; empty while the platform's driver ticks itself.
pub type TickSlot = Rc<RefCell<Option<Box<dyn FnMut()>>>>;

/// 64 Hz.
const PERIOD: Duration = Duration::from_micros(15_625);

/// The next wake-up: eframe's own (if any) or our next tick, whichever is
/// first. `None`: the slot is empty (the platform's driver ticks, macOS),
/// so eframe's flow stays as it is and an idle app does not wake 64 times a
/// second for nothing.
fn merged(flow: ControlFlow, next_tick: Option<Instant>) -> ControlFlow {
    let Some(next_tick) = next_tick else { return flow };
    match flow {
        ControlFlow::Poll => ControlFlow::Poll,
        ControlFlow::Wait => ControlFlow::WaitUntil(next_tick),
        ControlFlow::WaitUntil(t) => ControlFlow::WaitUntil(t.min(next_tick)),
    }
}

struct Wrap<'a> {
    app: eframe::EframeWinitApplication<'a>,
    tick: TickSlot,
    next: Instant,
}

impl Wrap<'_> {
    fn run_due(&mut self) {
        let now = Instant::now();
        if now < self.next {
            return;
        }
        self.next = (self.next + PERIOD).max(now);
        // Skipped while the slot is borrowed (a tick that re-enters the loop).
        if let Ok(mut slot) = self.tick.try_borrow_mut()
            && let Some(t) = slot.as_mut()
        {
            t();
        }
    }
}

impl ApplicationHandler<UserEvent> for Wrap<'_> {
    fn new_events(&mut self, el: &ActiveEventLoop, cause: StartCause) {
        self.app.new_events(el, cause);
        self.run_due();
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.app.resumed(el);
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: UserEvent) {
        self.app.user_event(el, event);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.app.window_event(el, id, event);
    }

    fn device_event(&mut self, el: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        self.app.device_event(el, id, event);
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.app.about_to_wait(el);
        // A borrowed slot is a tick in progress: it still holds a tick.
        let ticking = self.tick.try_borrow().map_or(true, |s| s.is_some());
        el.set_control_flow(merged(el.control_flow(), ticking.then_some(self.next)));
    }

    fn suspended(&mut self, el: &ActiveEventLoop) {
        self.app.suspended(el);
    }

    fn exiting(&mut self, el: &ActiveEventLoop) {
        self.app.exiting(el);
    }

    fn memory_warning(&mut self, el: &ActiveEventLoop) {
        self.app.memory_warning(el);
    }
}

/// Runs the app like `eframe::run_native`, plus the tick slot.
pub fn run(
    name: &str,
    opts: eframe::NativeOptions,
    creator: eframe::AppCreator<'_>,
    tick: TickSlot,
    ui_x11: bool,
) -> eframe::Result {
    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "linux")]
    if ui_x11 {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }
    #[cfg(not(target_os = "linux"))]
    let _ = ui_x11;
    // A menu bar agent: no Dock icon and no app menu, also when started
    // outside the bundle (whose Info.plist sets LSUIElement).
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    let event_loop = builder.build().map_err(eframe::Error::WinitEventLoop)?;
    let app = eframe::create_native(name, opts, creator, &event_loop);
    let mut wrap = Wrap { app, tick, next: Instant::now() };
    event_loop.run_app(&mut wrap).map_err(eframe::Error::WinitEventLoop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_ups_merge() {
        let t = Instant::now() + Duration::from_secs(1);
        let early = Instant::now();
        let tick = Some(t);
        assert_eq!(merged(ControlFlow::Poll, tick), ControlFlow::Poll);
        assert_eq!(merged(ControlFlow::Wait, tick), ControlFlow::WaitUntil(t));
        assert_eq!(merged(ControlFlow::WaitUntil(early), tick), ControlFlow::WaitUntil(early));
        assert_eq!(merged(ControlFlow::WaitUntil(t + PERIOD), tick), ControlFlow::WaitUntil(t));
        // An empty slot never adds a wake-up.
        assert_eq!(merged(ControlFlow::Wait, None), ControlFlow::Wait);
        assert_eq!(merged(ControlFlow::WaitUntil(t), None), ControlFlow::WaitUntil(t));
    }
}
