mod ui;

use std::path::PathBuf;
use std::sync::Arc;

use ct_core::settings::SettingsStore;
use ct_media::session::RoomSession;
use eframe::egui;
use tokio::runtime::Runtime;
use tracing_subscriber::EnvFilter;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let profile = std::env::args()
        .find_map(|a| a.strip_prefix("--profile=").map(ToString::to_string));

    let data_dir = SettingsStore::default_data_dir(profile.as_deref());
    let bin_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("bin")))
        .unwrap_or_else(|| PathBuf::from("resources/bin"));

    let rt = Arc::new(Runtime::new().expect("Falha ao criar runtime Tokio"));
    let session = RoomSession::new(data_dir, bin_dir);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("CompartilhadorDeTela")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "CompartilhadorDeTela",
        options,
        Box::new(move |cc| {
            ui::configure_theme(&cc.egui_ctx);
            Ok(Box::new(ui::CompartilharApp::new(rt, session)))
        }),
    )
}
