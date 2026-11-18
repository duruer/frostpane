//! Prints what the Wayland backend sees: the layer-shell probe, the KWin
//! bus probe and, with layer shell, the monitors and foreign windows of the
//! selected window source. Used by `smoke/smoke-wayland.sh`.

#[cfg(target_os = "linux")]
fn main() {
    use fp_platform::select::WindowSource;
    let layer = fp_wayland::layer_shell_available();
    println!("layer_shell={layer}");
    if !layer {
        return;
    }
    let source = match std::env::args().nth(1).as_deref() {
        Some("sway") => WindowSource::Sway,
        Some("hyprland") => WindowSource::Hyprland,
        Some("kwin") => WindowSource::KWin,
        _ => WindowSource::None,
    };
    let parts = fp_wayland::backend(source);
    println!("limits={:?}", parts.limits);
    for m in parts.services.monitors() {
        println!("monitor id={} pixels={:?} work={:?} scale={}", m.id, m.pixels, m.work, m.scale);
    }
    println!("has_geometry={}", parts.services.has_geometry());
    for w in parts.services.windows() {
        println!("window id={} bounds={:?} cloaked={} owner={}", w.hwnd, w.bounds, w.cloaked, w.process_name);
    }
    println!("shelves={:?}", parts.services.taskbar_shelves());
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("layer_shell=false (not Linux)");
}
