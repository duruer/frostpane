# Vendored eframe 0.37.0

This is eframe 0.37.0 from crates.io (MIT OR Apache-2.0, see `LICENSE-*` upstream
at https://github.com/emilk/egui) with one change in `src/native/run.rs`:

`WinitAppWrapper::new_events` now runs `check_redraw_requests` inside
`event_loop_context::with_event_loop_context`.

Why: on Windows, invisible windows never get `RedrawRequested`, so eframe paints
them directly from `new_events`. That path had no current event loop. When the
app opened an immediate viewport (`show_viewport_immediate`) during such a pass,
eframe could not create the viewport's window, never called the user callback,
and egui panicked with "egui backend is implemented incorrectly - the user
callback was never called".

Remove the `[patch.crates-io]` entry in the workspace `Cargo.toml` and this
directory once an upstream release includes the fix.
