#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod ui;

use std::sync::Arc;

use ct_core::settings::SettingsStore;
use ct_media::session::RoomSession;
use eframe::egui;
use tokio::runtime::Runtime;
use tracing_subscriber::EnvFilter;

fn main() -> eframe::Result<()> {
    let profile = std::env::args()
        .find_map(|a| a.strip_prefix("--profile=").map(ToString::to_string));

    let data_dir = SettingsStore::default_data_dir(profile.as_deref());
    let crash_log = data_dir.join("pit-crash.log");

    std::panic::set_hook(Box::new(move |info| {
        let _ = std::fs::create_dir_all(crash_log.parent().unwrap_or(std::path::Path::new(".")));
        let _ = std::fs::write(&crash_log, format!("[PANIC] {info}\n"));
    }));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .try_init();

    let rt = Arc::new(Runtime::new().expect("Falha ao criar runtime Tokio"));
    let session = RoomSession::new(data_dir);

    let win_w = 1280.0_f32;
    let win_h = 780.0_f32;
    let app_icon = ui::theme::load_window_icon();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(ui::theme::APP_NAME)
        .with_inner_size([win_w, win_h])
        .with_min_inner_size([860.0, 540.0]);

    // Centraliza no monitor principal logo ao abrir
    if let Some(primary) = session.list_screen_sources().first() {
        let cx = primary.left as f32 + ((primary.width as f32 - win_w) * 0.5).max(0.0);
        let cy = primary.top as f32 + ((primary.height as f32 - win_h) * 0.45).max(0.0);
        viewport = viewport.with_position(egui::pos2(cx, cy));
    }

    if let Some(ref icon) = app_icon {
        viewport = viewport.with_icon(icon.clone());
    }

    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        vsync: true,
        multisampling: 0,
        ..Default::default()
    };

    eframe::run_native(
        ui::theme::APP_NAME,
        options,
        Box::new(move |cc| {
            let (initial_theme, initial_custom) = {
                let rx = session.subscribe();
                let s = &rx.borrow().settings;
                (s.theme, s.custom_accent)
            };
            ui::theme::configure_theme(&cc.egui_ctx, initial_theme, initial_custom);
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
