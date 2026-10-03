//! log_analyzer - a cross-platform GUI log explorer.

mod app;
mod log_file;
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
        Box::new(move |_cc| {
            let mut app = app::LogAnalyzerApp::new();
            if let Some(path) = initial_path {
                app.open_path(std::path::Path::new(&path));
            }
            Ok(Box::new(app))
        }),
    )
}
