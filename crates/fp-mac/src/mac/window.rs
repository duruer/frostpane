//! The borderless, non-activating, transparent panel shared by overlay panels
//! and sprite windows, with a layer-hosting content view.

use crate::logic::CocoaRect;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApp, NSBackingStoreType, NSColor, NSPanel, NSView, NSWindow, NSWindowAnimationBehavior,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_quartz_core::CALayer;

pub fn ns_rect(r: CocoaRect) -> NSRect {
    NSRect::new(NSPoint::new(r.x, r.y), NSSize::new(r.w.max(1.0), r.h.max(1.0)))
}

/// A hidden click-through panel at `frame` whose content view hosts `layer`
/// (layer-hosting: AppKit never draws into it). Visible on every Space and
/// next to full-screen apps; never activates the app, never in the window
/// cycle, no shadow, no show/hide animation.
pub fn panel(mtm: MainThreadMarker, frame: CocoaRect, layer: &CALayer, level: i64) -> Retained<NSPanel> {
    let rect = ns_rect(frame);
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
    // `defer: false` gives the window its window number at once.
    let p = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        rect,
        style,
        NSBackingStoreType::Buffered,
        false,
    );
    // SAFETY: we keep our own strong reference; `close` must not free it.
    unsafe { p.setReleasedWhenClosed(false) };
    p.setOpaque(false);
    p.setBackgroundColor(Some(&NSColor::clearColor()));
    p.setHasShadow(false);
    p.setIgnoresMouseEvents(true);
    p.setHidesOnDeactivate(false);
    p.setCanHide(false);
    p.setAnimationBehavior(NSWindowAnimationBehavior::None);
    p.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    p.setLevel(level as isize);
    let view = NSView::initWithFrame(NSView::alloc(mtm), NSRect::new(NSPoint::new(0.0, 0.0), rect.size));
    // Layer first, then wantsLayer: a layer-hosting view.
    view.setLayer(Some(layer));
    view.setWantsLayer(true);
    layer.setFrame(view.bounds());
    p.setContentView(Some(&view));
    p
}

/// Resizes the hosted layer with the window (sprites change size).
pub fn fit_layer(w: &NSWindow, layer: &CALayer) {
    if let Some(v) = w.contentView() {
        layer.setFrame(v.bounds());
    }
}

/// One of this app's windows by window number.
pub fn by_number(mtm: MainThreadMarker, n: i64) -> Option<Retained<NSWindow>> {
    NSApp(mtm).windowWithWindowNumber(n as isize)
}

/// `obj` as an untyped object (`CALayer.contents`).
pub fn any<T>(obj: &T) -> &AnyObject {
    // SAFETY: only used for Objective-C / toll-free bridged CF objects.
    unsafe { &*(obj as *const T).cast::<AnyObject>() }
}
