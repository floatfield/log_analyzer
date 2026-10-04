//! log_analyzer - a cross-platform GUI log explorer.

mod app;
mod log_file;
mod persistence;
mod query;

fn main() -> eframe::Result {
    // Optional convenience: `log_analyzer [path]` opens the file immediately
    // instead of going through the Open dialog.
    let initial_path = std::env::args().nth(1);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("log_analyzer"),
        ..Default::default()
    };
    eframe::run_native(
        "log_analyzer",
        options,
        Box::new(move |cc| {
            // Light theme from startup (spec: app-appearance).
            cc.egui_ctx.set_visuals(eframe::egui::Visuals::light());
            let mut app = app::LogAnalyzerApp::new();
            if let Some(path) = initial_path {
                app.open_path(std::path::Path::new(&path));
            }
            Ok(Box::new(app))
        }),
    )
}
