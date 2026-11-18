//! `CGWindowListCopyWindowInfo` reads (thread-safe Quartz). Only number,
//! layer, bounds, alpha, owner pid and owner name are read: none of them needs
//! Screen Recording permission (`kCGWindowName` would, and is never read).

use crate::logic::{self, Key, RawWindow, WindowDict};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CFRetained, CFString};
use objc2_core_graphics::{
    CGWindowListCopyWindowInfo, CGWindowListOption, kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer, kCGWindowNumber,
    kCGWindowOwnerName, kCGWindowOwnerPID,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, ns_string};

type Dict = NSDictionary<NSString, AnyObject>;

/// A CF string constant as the toll-free bridged `NSString`.
fn ns(s: &'static CFString) -> &'static NSString {
    // SAFETY: CFString and NSString are toll-free bridged.
    unsafe { &*(s as *const CFString).cast::<NSString>() }
}

fn key(k: Key) -> &'static NSString {
    // SAFETY: reading immutable framework constants.
    unsafe {
        ns(match k {
            Key::Number => kCGWindowNumber,
            Key::Layer => kCGWindowLayer,
            Key::Alpha => kCGWindowAlpha,
            Key::OwnerPid => kCGWindowOwnerPID,
            Key::OwnerName => kCGWindowOwnerName,
        })
    }
}

struct Entry(Retained<Dict>);

fn number(d: &Dict, k: &NSString) -> Option<f64> {
    d.objectForKey(k)?.downcast::<NSNumber>().ok().map(|n| n.doubleValue())
}

impl WindowDict for Entry {
    fn number(&self, k: Key) -> Option<f64> {
        number(&self.0, key(k))
    }

    fn text(&self, k: Key) -> Option<String> {
        self.0.objectForKey(key(k))?.downcast::<NSString>().ok().map(|s| s.to_string())
    }

    /// The bounds dictionary uses the `CGRect` dictionary keys.
    fn bounds(&self) -> Option<[f64; 4]> {
        // SAFETY: reading an immutable framework constant.
        let b = self.0.objectForKey(ns(unsafe { kCGWindowBounds }))?;
        // SAFETY: kCGWindowBounds holds a CFDictionary of CFString -> CFNumber.
        let b: &Dict = unsafe { &*Retained::as_ptr(&b).cast::<Dict>() };
        Some([
            number(b, ns_string!("X"))?,
            number(b, ns_string!("Y"))?,
            number(b, ns_string!("Width"))?,
            number(b, ns_string!("Height"))?,
        ])
    }
}

/// Windows of `option` relative to `window`, front to back.
pub fn list(option: CGWindowListOption, window: u32) -> Vec<RawWindow> {
    let Some(arr) = CGWindowListCopyWindowInfo(option, window) else { return Vec::new() };
    // SAFETY: the array holds CFDictionaries (toll-free bridged); it stays
    // alive (`arr`) while the bridged reference is used.
    let items: &NSArray<Dict> = unsafe { &*CFRetained::as_ptr(&arr).as_ptr().cast::<NSArray<Dict>>() };
    items.iter().filter_map(|d| logic::raw_window(&Entry(d))).collect()
}

/// On-screen windows without desktop elements, front to back.
pub fn on_screen() -> Vec<RawWindow> {
    list(CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements, 0)
}

/// On-screen windows in front of `window`.
pub fn above(window: u32) -> Vec<RawWindow> {
    list(CGWindowListOption::OptionOnScreenAboveWindow | CGWindowListOption::ExcludeDesktopElements, window)
}

/// This process id, as the window list reports owners.
pub fn own_pid() -> i64 {
    i64::from(std::process::id())
}

/// Every on-screen window, desktop levels included (our behind panel and
/// desktop sprites), front to back.
pub fn all_on_screen() -> Vec<RawWindow> {
    list(CGWindowListOption::OptionOnScreenOnly, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Porting design 5.4: bounds, layer and owner come back without Screen
    /// Recording permission (CI runners never grant it).
    #[test]
    fn window_list_has_bounds_without_permission() {
        let all = all_on_screen();
        assert!(!all.is_empty(), "no on-screen windows (no GUI session?)");
        assert!(all.iter().any(|w| w.bounds.is_some_and(|b| b[2] > 0.0 && b[3] > 0.0) && w.pid > 0));
        assert!(all.iter().any(|w| !w.owner.is_empty()));
        // Front-to-back lists relative to a window never include it.
        let first = all[0].id as u32;
        assert!(above(first).iter().all(|w| w.id != all[0].id));
    }
}
