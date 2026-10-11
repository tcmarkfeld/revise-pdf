mod app;
mod capture;
mod cli;
mod findbar;
mod icons;
mod pagesetup;
mod persist;
mod spell;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod menus;
mod theme;
mod toolbar;
mod updater;
mod view;
mod worker;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = cli::run(&args) {
        std::process::exit(code);
    }
    install_crash_log();
    #[cfg(target_os = "macos")]
    macos::register_open_documents();
    let file = args.first().map(PathBuf::from);
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 900.0])
            .with_min_inner_size([760.0, 480.0])
            .with_title("Revise")
            // Unified title bar: content runs under the traffic lights.
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        ..Default::default()
    };
    eframe::run_native("Revise", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, file)))))
}

/// Writes panics (with a backtrace) to ~/Library/Logs/Revise so crashes seen
/// in normal use can be diagnosed.
fn install_crash_log() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(home) = std::env::var_os("HOME") {
            let dir = PathBuf::from(home).join("Library/Logs/Revise");
            let _ = std::fs::create_dir_all(&dir);
            let bt = std::backtrace::Backtrace::force_capture();
            let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("crash-{secs}.log")));
            if let Ok(mut f) = log {
                use std::io::Write;
                let _ = writeln!(f, "{info}\n\n{bt}\n");
            }
        }
        default(info);
    }));
}
