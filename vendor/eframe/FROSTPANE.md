# Vendored eframe 0.37.0

This is eframe 0.37.0 from crates.io (MIT OR Apache-2.0, see `LICENSE-*` upstream
at https://github.com/emilk/egui) with two changes in `src/native/`.

## 1. Painting invisible windows (`run.rs`)

`WinitAppWrapper::new_events` now runs `check_redraw_requests` inside
`event_loop_context::with_event_loop_context`.

Why: on Windows, invisible windows never get `RedrawRequested`, so eframe paints
them directly from `new_events`. That path had no current event loop. When the
app opened an immediate viewport (`show_viewport_immediate`) during such a pass,
eframe could not create the viewport's window, never called the user callback,
and egui panicked with "egui backend is implemented incorrectly - the user
callback was never called".

## 2. Transparent viewport windows on Windows (`wgpu_integration.rs`)

Goes with the vendored egui-wgpu (see `vendor/egui-wgpu/FROSTPANE.md`), which
paints transparent viewports through DirectComposition on Windows.

- `Viewport::initialize_window` tells the painter whether the viewport is
  transparent (`Painter::set_viewport_transparent`, from the viewport's
  `ViewportBuilder::transparent`) before the first `set_window`. Upstream only
  passes the root viewport's transparency, once, for every window.
- New `create_viewport_window`: on Windows, when the viewport is transparent and
  the painter runs on DX12, the window is created with
  `with_no_redirection_bitmap(true)` (`WS_EX_NOREDIRECTIONBITMAP`), without the
  undecorated-window shadow (its 1px top line and DWM shadow would frame the
  transparent margin), without the DWM border and without rounded-corner
  clipping. Without these the window's own surface (white) and frame show
  through around the composition visual. Everything else still goes through
  `egui_winit::create_window`.

Remove the `[patch.crates-io]` entry in the workspace `Cargo.toml` and this
directory once an upstream release includes both fixes.
