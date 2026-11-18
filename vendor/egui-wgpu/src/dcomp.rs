//! Frostpane patch: per-pixel transparent windows on Windows.
//!
//! DXGI swapchains created for an HWND (flip model) only offer
//! [`wgpu::CompositeAlphaMode::Opaque`], so a transparent window rendered
//! through one shows black wherever egui leaves alpha at zero. A swapchain
//! bound to a `DirectComposition` visual supports premultiplied alpha. This
//! module builds that visual tree for a window and hands the visual to wgpu
//! (DX12 only).

#![expect(unsafe_code)]

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice2, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::core::{IUnknown, Interface as _};
use winit::raw_window_handle::{HasWindowHandle as _, RawWindowHandle};

/// The composition tree behind one window's surface. Must outlive the
/// [`wgpu::Surface`] created from it.
pub(crate) struct Composition {
    device: IDCompositionDevice,
    _target: IDCompositionTarget,
    visual: IDCompositionVisual,
}

impl Composition {
    /// Shows changes to the tree. wgpu sets the visual's content (the
    /// swapchain) when the surface is configured but never commits it.
    pub(crate) fn commit(&self) {
        // SAFETY: a valid device owned by `self`.
        if let Err(err) = unsafe { self.device.Commit() } {
            log::warn!("DirectComposition commit failed: {err}");
        }
    }
}

/// Creates a composition target for `window` and a DX12 surface on its root
/// visual. `None` (with a log line) if any step fails; the caller then
/// falls back to the regular HWND surface.
pub(crate) fn create_surface(
    instance: &wgpu::Instance,
    window: &winit::window::Window,
) -> Option<(wgpu::Surface<'static>, Composition)> {
    let hwnd = match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => HWND(h.hwnd.get() as *mut _),
        _ => return None,
    };
    // SAFETY: DirectComposition calls on a live window owned by the caller;
    // the visual pointer handed to wgpu stays valid because the returned
    // `Composition` keeps the visual alive and the caller drops the surface
    // first.
    let built = unsafe {
        (|| -> windows::core::Result<_> {
            let device: IDCompositionDevice = DCompositionCreateDevice2(None::<&IUnknown>)?;
            let target = device.CreateTargetForHwnd(hwnd, true)?;
            let visual = device.CreateVisual()?;
            target.SetRoot(&visual)?;
            device.Commit()?;
            Ok(Composition {
                device,
                _target: target,
                visual,
            })
        })()
    };
    let composition = match built {
        Ok(c) => c,
        Err(err) => {
            log::warn!("DirectComposition setup failed, using an opaque surface: {err}");
            return None;
        }
    };
    // SAFETY: see above.
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CompositionVisual(
            composition.visual.as_raw(),
        ))
    };
    match surface {
        Ok(surface) => Some((surface, composition)),
        Err(err) => {
            log::warn!("Composition surface failed, using an opaque surface: {err}");
            None
        }
    }
}
