mod ui;

use std::sync::Arc;

use ct_core::settings::SettingsStore;
use ct_media::session::RoomSession;
use eframe::egui;
use tokio::runtime::Runtime;
use tracing_subscriber::EnvFilter;

fn main() -> eframe::Result<()> {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("[PANIC] {info}");
    }));

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let profile = std::env::args()
        .find_map(|a| a.strip_prefix("--profile=").map(ToString::to_string));

    let data_dir = SettingsStore::default_data_dir(profile.as_deref());

    let rt = Arc::new(Runtime::new().expect("Falha ao criar runtime Tokio"));
    let session = RoomSession::new(data_dir);

    let app_icon = ui::theme::load_window_icon();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(ui::theme::APP_NAME)
        .with_inner_size([1320.0, 820.0])
        .with_min_inner_size([920.0, 580.0]);
    if let Some(ref icon) = app_icon {
        viewport = viewport.with_icon(icon.clone());
    }

    let options = eframe::NativeOptions {
        viewport,
        vsync: true,
        multisampling: 0,
        ..Default::default()
    };

    eframe::run_native(
        ui::theme::APP_NAME,
        options,
        Box::new(move |cc| {
            let initial_theme = session.subscribe().borrow().settings.theme;
            ui::theme::configure_theme(&cc.egui_ctx, initial_theme);
            let ctx_for_cb = cc.egui_ctx.clone();
            session.set_repaint_callback(Arc::new(move || {
                ctx_for_cb.request_repaint();
            }));
            Ok(Box::new(ui::CompartilharApp::new(
                &cc.egui_ctx,
                rt,
                session,
                app_icon,
            )))
        }),
    )
}
