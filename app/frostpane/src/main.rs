//! Frostpane: weather on your desktop.
//!
//! Startup order follows core-app 2.1 adapted (see `docs/ARCHITECTURE.md`):
//! args, platform backend (`platform.rs`), DPI awareness, single instance,
//! crash marker, prefs and settings,
//! language, then inside the eframe creation callback the tray, world,
//! sources, day-and-night and (while a weather is shown) the overlay, and
//! finally the window (hidden for a quiet start). Everything runs on the UI
//! thread except the per-monitor render threads, the HTTP worker and the
//! second-launch listener.
//!
//! Flags: `--quiet` start in the tray without the window (once onboarding is
//! done); `--seconds N` exit by itself after N seconds (smoke runs);
//! `--open-picker` open the decoration picker one second after start.
//! Environment: `FROSTPANE_DATA_DIR` overrides the data folder (tests);
//! `FROSTPANE_LOG` sets the log level.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod args;
mod core;
mod crash;
mod decode;
mod factory;
mod logger;
#[cfg(not(windows))]
mod looped;
mod outside;
mod packs;
mod platform;
mod react;
mod sources;
mod wallpaper;

use crate::core::{Config, Core};
use fp_core::effect::Lang;
use fp_core::prefs::Prefs;
use fp_core::rng::{Lcg64, mix_seed};
use fp_core::settings::Settings;
use fp_platform::{Guard, Instance, Services};
use fp_ui::eframe::{self, egui};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Driver period; the USER timer gives about 64 Hz.
const DRIVER_MS: u32 = 10;
/// A start within this long after boot counts as "at login" (core-app 2.1).
const LOGIN_UPTIME_MS: u64 = 300_000;

/// The eframe app: a thin shell around [`Core`].
struct Host {
    core: Rc<RefCell<Core>>,
    driver: Option<Guard>,
}

impl eframe::App for Host {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.core.borrow_mut().logic();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.core.borrow_mut().ui(ui);
    }

    /// Transparent, so the card viewports are see-through.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn on_exit(&mut self) {
        // Stop ticking first, then save and tear down on the UI thread.
        self.driver = None;
        let mut core = self.core.borrow_mut();
        core.cleanup();
        let (panels, frames) = core.overlay_stats();
        log::info!("exit: {panels} overlay panel(s), {frames} frames presented");
        println!("frostpane: exit after {} ticks, {panels} overlay panel(s), {frames} frames presented", core.ticks);
    }
}

fn data_dir(os: &dyn Services) -> PathBuf {
    std::env::var_os("FROSTPANE_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| os.data_dir())
}

/// System uptime below 5 minutes counts as a start at login.
fn at_login(os: &dyn Services) -> bool {
    os.uptime_ms() < LOGIN_UPTIME_MS
}

fn main() -> eframe::Result {
    logger::init();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let demo_cards = argv.iter().any(|a| a == "--open-picker");
    let args = args::parse(argv);
    let backend = match platform::select() {
        Ok(b) => b,
        Err(e) => {
            log::error!("{e}");
            eprintln!("frostpane: {e}");
            std::process::exit(1);
        }
    };
    if backend.limits != fp_platform::Limits::default() {
        log::info!("platform limits: {:?}", backend.limits);
    }
    let os = backend.services.clone();
    os.init_process();

    let instance = match os.single_instance(args.quiet) {
        Instance::First(i) => i,
        Instance::Second => {
            println!("frostpane: already running");
            return Ok(());
        }
    };

    let dir = data_dir(&*os);
    let mut rng = Lcg64::new(mix_seed(std::process::id().into()));
    let session = fp_core::marker::session_id(&mut rng);
    let launched = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let previous = crash::start(&dir, &session, launched, env!("CARGO_PKG_VERSION"));
    crash::install_panic_hook(dir.clone());
    if let Some(m) = &previous.marker {
        log::warn!("the previous run (version {}, session {}) did not exit cleanly", m.version, m.session);
    }
    if let Some(text) = &previous.crash {
        log::warn!("the previous run left a crash report ({} characters)", text.chars().count());
    }

    let prefs = Prefs::load(dir.join("settings.json"));
    let (settings, dirty) = Settings::from_map(prefs.map());
    let mut prefs = prefs;
    if dirty {
        let s = settings.clone();
        if let Err(e) = prefs.edit(|m| s.write_into(m)) {
            log::warn!("rewriting settings failed: {e}");
        }
    }
    let lang = Lang::startup(settings.language.map(|l| l.code()), &os.os_language());
    let quiet_start = settings.seen_welcome && (args.quiet || at_login(&*os));
    // Only the first window size depends on it: the first Home frame fits
    // the window to the monitors `Core` reads once the event loop exists,
    // and `Core` re-reads an empty list every few seconds.
    let work_height = {
        let monitors = os.monitors();
        if monitors.is_empty() {
            log::warn!("no monitors before the event loop; sizing the window for 1080 points");
        }
        monitors
            .iter()
            .find(|m| m.primary)
            .or(monitors.first())
            .map_or(1080.0, |m| (m.work.height() as f64 / m.scale.max(0.1)) as f32)
    };

    let opts = eframe::NativeOptions {
        viewport: fp_ui::viewport_builder(lang, settings.seen_welcome, work_height, true, !quiet_start),
        wgpu_options: fp_ui::wgpu_options(),
        ..Default::default()
    };
    let open_request = Arc::new(AtomicBool::new(false));
    // The tick the event loop wrapper runs when the platform has no driver timer.
    #[cfg(not(windows))]
    let slot: looped::TickSlot = Rc::default();
    #[cfg(not(windows))]
    let fill = slot.clone();
    let platform::Backend { services, sprites, overlay, ui_x11, limits } = backend;
    let creator: eframe::AppCreator<'_> = Box::new(move |cc| {
        let ctx = cc.egui_ctx.clone();
        fp_ui::install(&ctx);
        let cfg = Config {
            args,
            data_dir: dir,
            prefs,
            open_request: open_request.clone(),
            demo_cards,
            services: services.clone(),
            sprites,
            overlay,
            limits,
        };
        let core = Rc::new(RefCell::new(Core::new(ctx.clone(), cfg, quiet_start, lang, settings)));
        let tick = || -> Box<dyn FnMut()> {
            let ticker = core.clone();
            Box::new(move || {
                // Skipped while the window loop holds the core.
                if let Ok(mut c) = ticker.try_borrow_mut() {
                    c.tick();
                }
            })
        };
        let driver = services.start(DRIVER_MS, tick());
        #[cfg(not(windows))]
        if driver.is_none() {
            *fill.borrow_mut() = Some(tick());
        }
        #[cfg(windows)]
        if driver.is_none() {
            log::warn!("driver timer unavailable");
        }
        {
            let flag = open_request;
            let wake = ctx.clone();
            instance.listen(Box::new(move || {
                flag.store(true, Ordering::Relaxed);
                wake.request_repaint();
            }));
            // Keep the mutex for the whole process lifetime.
            std::mem::forget(instance);
        }
        {
            let c = core.borrow();
            let (panels, _) = c.overlay_stats();
            println!(
                "frostpane: started, {} monitor(s), {panels} overlay panel(s), window {}",
                c.monitor_count(),
                if quiet_start { "hidden" } else { "shown" }
            );
        }
        Ok(Box::new(Host { core, driver }))
    });
    #[cfg(windows)]
    let _ = ui_x11;
    #[cfg(windows)]
    return eframe::run_native("Frostpane", opts, creator);
    #[cfg(not(windows))]
    looped::run("Frostpane", opts, creator, slot, ui_x11)
}
