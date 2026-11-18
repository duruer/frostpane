//! `NSScreen` reads (main thread) and the cached pixel space that render
//! threads and Quartz conversions use (porting design 2.4).

use crate::logic::{self, CocoaRect, RawScreen, Space};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSScreen;
use objc2_core_graphics::{CGDisplayModelNumber, CGDisplaySerialNumber, CGDisplayVendorNumber};
use objc2_foundation::{NSNumber, NSRect, ns_string};
use std::sync::{Mutex, PoisonError};

/// Last space read on the main thread; `None` before the first read.
static SPACE: Mutex<Option<Space>> = Mutex::new(None);

fn rect(r: NSRect) -> CocoaRect {
    CocoaRect::new(r.origin.x, r.origin.y, r.size.width, r.size.height)
}

/// `CGDirectDisplayID` of a screen (`deviceDescription["NSScreenNumber"]`).
pub fn display_id(s: &NSScreen) -> Option<u32> {
    let d = s.deviceDescription();
    let n = d.objectForKey(ns_string!("NSScreenNumber"))?;
    n.downcast::<NSNumber>().ok().map(|n| n.unsignedIntValue())
}

/// Stable id of a screen (monitor and wallpaper ids).
pub fn key(s: &NSScreen) -> String {
    match display_id(s) {
        Some(id) => {
            logic::display_key(CGDisplayVendorNumber(id), CGDisplayModelNumber(id), CGDisplaySerialNumber(id), id)
        }
        None => String::new(),
    }
}

/// `NSScreen.screens`, primary first.
pub fn all(mtm: MainThreadMarker) -> Vec<Retained<NSScreen>> {
    NSScreen::screens(mtm).iter().collect()
}

/// Every screen as plain data, and the space they define (cached).
pub fn read(mtm: MainThreadMarker) -> (Vec<RawScreen>, Space) {
    let raws: Vec<RawScreen> = all(mtm)
        .iter()
        .map(|s| RawScreen {
            key: key(s),
            name: s.localizedName().to_string(),
            frame: rect(s.frame()),
            visible: rect(s.visibleFrame()),
            backing: s.backingScaleFactor(),
        })
        .collect();
    let sp = logic::space(&raws);
    if !raws.is_empty() {
        *SPACE.lock().unwrap_or_else(PoisonError::into_inner) = Some(sp);
    }
    (raws, sp)
}

/// The cached space. Reads the screens first when nothing is cached and this
/// is the main thread; other threads then get the default (scale 1).
pub fn space() -> Space {
    if let Some(sp) = *SPACE.lock().unwrap_or_else(PoisonError::into_inner) {
        return sp;
    }
    MainThreadMarker::new().map(|m| read(m).1).unwrap_or_default()
}
