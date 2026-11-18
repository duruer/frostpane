//! Embeds the app icon into frostpane.exe (Windows targets only).

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/windows/frostpane.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed();
    }
}

#[cfg(windows)]
fn embed() {
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../packaging/windows/frostpane.ico");
    res.compile().expect("embed the Windows icon resource");
}

#[cfg(not(windows))]
fn embed() {}
