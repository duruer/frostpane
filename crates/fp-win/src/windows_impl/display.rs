//! Monitor enumeration with display-config names (desktop-native 3.6).

use crate::logic::{RawDisplay, assemble_displays};
use fp_core::desktop::MonitorInfo;
use fp_core::geom::PixelRect;
use std::collections::HashMap;
use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::*;
use windows::core::BOOL;

unsafe extern "system" fn monitor_cb(h: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
    // SAFETY: lp points at the Vec passed by `raw_displays`.
    let out = unsafe { &mut *(lp.0 as *mut Vec<RawDisplay>) };
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: mi is a correctly sized MONITORINFOEXW.
    if !unsafe { GetMonitorInfoW(h, &mut mi as *mut _ as *mut MONITORINFO) }.as_bool() {
        return TRUE;
    }
    let (mut dx, mut dy) = (0u32, 0u32);
    // SAFETY: valid monitor handle and out pointers.
    let scale = match unsafe { GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) } {
        Ok(()) if dx > 0 => dx as f64 / 96.0,
        _ => 1.0,
    };
    let r = mi.monitorInfo.rcMonitor;
    let wk = mi.monitorInfo.rcWork;
    let n = mi.szDevice.iter().position(|c| *c == 0).unwrap_or(mi.szDevice.len());
    out.push(RawDisplay {
        gdi: String::from_utf16_lossy(&mi.szDevice[..n]),
        pixels: PixelRect::new(r.left, r.top, r.right, r.bottom),
        work: PixelRect::new(wk.left, wk.top, wk.right, wk.bottom),
        scale,
        primary: mi.monitorInfo.dwFlags & 1 != 0,
    });
    TRUE
}

fn raw_displays() -> Vec<RawDisplay> {
    let mut v: Vec<RawDisplay> = Vec::new();
    // SAFETY: callback only writes into `v` during the call.
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(monitor_cb), LPARAM(&mut v as *mut _ as isize));
    }
    v
}

fn wstr(b: &[u16]) -> String {
    let n = b.iter().position(|c| *c == 0).unwrap_or(b.len());
    String::from_utf16_lossy(&b[..n])
}

/// Lower-cased GDI name -> (friendly name, device path). Empty on any failure.
fn display_names() -> HashMap<String, (String, String)> {
    let mut map = HashMap::new();
    let (mut np, mut nm) = (0u32, 0u32);
    // SAFETY: valid out pointers.
    if unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) } != ERROR_SUCCESS {
        return map;
    }
    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np.max(1) as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm.max(1) as usize];
    // SAFETY: buffers sized from the counts passed in.
    let r = unsafe {
        QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None)
    };
    if r != ERROR_SUCCESS {
        return map;
    }
    for p in paths.iter().take(np as usize) {
        let mut src = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
        src.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
        src.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
        src.header.adapterId = p.sourceInfo.adapterId;
        src.header.id = p.sourceInfo.id;
        let mut tgt = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
        tgt.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
        tgt.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
        tgt.header.adapterId = p.targetInfo.adapterId;
        tgt.header.id = p.targetInfo.id;
        // SAFETY: headers are initialised with matching sizes.
        let (a, b) =
            unsafe { (DisplayConfigGetDeviceInfo(&mut src.header), DisplayConfigGetDeviceInfo(&mut tgt.header)) };
        if a == 0 && b == 0 {
            map.insert(
                wstr(&src.viewGdiDeviceName).to_lowercase(),
                (wstr(&tgt.monitorFriendlyDeviceName), wstr(&tgt.monitorDevicePath)),
            );
        }
    }
    map
}

/// All monitors: primary first, then by left edge, with stable ids and names.
pub fn monitors() -> Vec<MonitorInfo> {
    let raws = raw_displays();
    if raws.is_empty() {
        return Vec::new();
    }
    assemble_displays(raws, &display_names())
}
