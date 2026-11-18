# Vendored egui-wgpu 0.37.0

This is egui-wgpu 0.37.0 from crates.io (MIT OR Apache-2.0, see `LICENSE-MIT`
and upstream at https://github.com/emilk/egui) with one change: per-pixel
transparent viewports (on Windows through DirectComposition).

Why: egui-wgpu creates every window surface with
`wgpu::Instance::create_surface(window)`. On Windows that is a DXGI flip-model
swapchain on the HWND, and those only offer `CompositeAlphaMode::Opaque`. A
transparent viewport (Frostpane's cards: a glass card with a soft shadow in a
transparent margin) therefore rendered its margin as a black rectangle, on
DX12, Vulkan and GL alike. Upstream also only honours transparency for the
root viewport, so on every platform a transparent child viewport got
`CompositeAlphaMode::Auto` (opaque) when the root window is opaque.

What changed:

- `src/dcomp.rs` (new, Windows only): creates an `IDCompositionDevice`
  (`DCompositionCreateDevice2` without a DXGI device), a target for the
  window's HWND (`CreateTargetForHwnd`, topmost), a root visual, and a wgpu
  surface on that visual (`SurfaceTargetUnsafe::CompositionVisual`). DX12
  composition swapchains support premultiplied alpha. Frostpane's weather
  overlay (`crates/fp-win/src/overlay.rs`) uses the same setup.
- `src/winit.rs`:
  - `Painter::set_viewport_transparent(viewport_id, bool)` marks a viewport as
    transparent; the vendored eframe calls it per viewport. Transparency (the
    alpha mode choice: premultiplied, else postmultiplied, when the surface
    offers it) now applies per viewport on every platform instead of only
    through the painter-wide `support_transparent_backbuffer`.
  - Surfaces are created by `create_window_surface`: on Windows, a transparent
    viewport gets the composition surface once the adapter (chosen through the
    first window's regular surface) is known to be DX12; anything else, or any
    failure in the composition setup, falls back to the regular HWND surface.
  - `SurfaceState` keeps the composition tree alive (dropped after the
    surface) and `configure_surface` commits it after every configure: wgpu
    sets the visual's content to the new swapchain but never commits.
  - `gc_viewports` also forgets transparency marks of closed viewports.
- `Cargo.toml`: `windows` 0.62 (DirectComposition) as a Windows-only
  dependency.

The window itself must have no redirection bitmap, or its own surface shows
through the transparent parts of the visual; the vendored eframe creates
transparent viewport windows that way (see `vendor/eframe/FROSTPANE.md`).

Remove the `[patch.crates-io]` entry in the workspace `Cargo.toml` and this
directory once upstream supports transparent windows on Windows.
