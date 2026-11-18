//! Notification observers and the main-thread repeating timer, each removed
//! when its guard drops.

use block2::RcBlock;
use objc2::Message;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSObjectProtocol, NSOperationQueue, NSRunLoop, NSRunLoopCommonModes,
    NSString, NSTimer,
};
use std::cell::RefCell;
use std::ptr::NonNull;

type Token = Retained<ProtocolObject<dyn NSObjectProtocol>>;

/// Block observers on notification centers, delivered on the main queue.
#[derive(Default)]
pub struct Observers {
    list: Vec<(Retained<NSNotificationCenter>, Token)>,
}

impl Observers {
    /// Calls `f` with the notification's name for every `name` posted to
    /// `center`. `f` runs on the main thread (main operation queue).
    pub fn add(&mut self, center: &NSNotificationCenter, name: &str, f: impl Fn(&str) + 'static) {
        let block = RcBlock::new(move |n: NonNull<NSNotification>| {
            // SAFETY: the center passes a live notification for the call.
            let n = unsafe { n.as_ref() };
            f(&n.name().to_string());
        });
        let ns = NSString::from_str(name);
        let queue = NSOperationQueue::mainQueue();
        // SAFETY: `obj` is nil and the queue is the main queue, so the block
        // (which holds main-thread-only state) only ever runs on the main thread.
        let token = unsafe { center.addObserverForName_object_queue_usingBlock(Some(&ns), None, Some(&queue), &block) };
        self.list.push((center.retain(), token));
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

impl Drop for Observers {
    fn drop(&mut self) {
        for (center, token) in self.list.drain(..) {
            // SAFETY: `token` was returned by this center's addObserver call.
            unsafe { center.removeObserver(token.as_ref()) };
        }
    }
}

/// A repeating `NSTimer` on the main run loop in the common modes, so it also
/// fires while a menu is tracking. Invalidated on drop.
pub struct Timer {
    timer: Retained<NSTimer>,
}

impl Timer {
    /// Must run on the main thread: the timer joins the current run loop.
    pub fn start(every_ms: u32, tick: Box<dyn FnMut()>) -> Timer {
        let tick = RefCell::new(tick);
        let block = RcBlock::new(move |_: NonNull<NSTimer>| {
            // Skip a tick instead of re-entering one that runs a nested loop.
            if let Ok(mut t) = tick.try_borrow_mut() {
                t();
            }
        });
        let secs = f64::from(every_ms.max(1)) / 1000.0;
        // SAFETY: the timer is added to the current (main) run loop below, so
        // the non-Send block only runs on this thread.
        let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(secs, true, &block) };
        // Some slack lets the OS coalesce wake-ups.
        timer.setTolerance(secs * 0.1);
        // SAFETY: plain run-loop registration with the documented mode constant.
        unsafe { NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
        Timer { timer }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        self.timer.invalidate();
    }
}
