use ct_core::protocol::{ContentType, FPS_OPTIONS, RESOLUTIONS};
use ct_media::session::ShareOptions;
use egui::{Color32, Context, RichText, Window};

use super::{ACCENT_PRIMARY, BG_CONTROL, CompartilharApp, TEXT_MUTED, primary_button};

pub fn render_share_dialog(app: &mut CompartilharApp, ctx: &Context) {
    let mut open = app.show_share_dialog;
    let mut res = app.selected_resolution;
    let mut fps = app.selected_fps;
    let mut content_type = app.snap.settings.content_type;
    let mut share_audio = app.snap.settings.share_audio;
    let mut blocked = app.snap.settings.blocked_apps.clone();
    let apps_snapshot = app.audio_apps.lock().map(|g| g.clone()).unwrap_or_default();

    let mut start_clicked = false;
    let mut cancel_clicked = false;
    let mut settings_dirty = false;

    Window::new("Configurar transmissão")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .min_width(480.0)
        .show(ctx, |ui| {
            ui.label(RichText::new("Monitor").strong());
            egui::ComboBox::from_id_salt("screen_picker")
                .width(ui.available_width())
                .selected_text(
                    app.screen_sources
                        .get(app.selected_source_idx)
                        .map(|s| s.name.as_str())
                        .unwrap_or("Nenhuma tela detectada"),
                )
                .show_ui(ui, |ui| {
                    for (idx, src) in app.screen_sources.iter().enumerate() {
                        ui.selectable_value(&mut app.selected_source_idx, idx, &src.name);
                    }
                });

            ui.add_space(12.0);
            ui.label(RichText::new("Resolução").strong());
            ui.horizontal(|ui| {
                for &opt in RESOLUTIONS {
                    let selected = res == opt;
                    let btn = egui::Button::new(
                        RichText::new(opt.label())
                            .color(if selected { Color32::WHITE } else { TEXT_MUTED })
                            .strong(),
                    )
                    .fill(if selected { ACCENT_PRIMARY } else { BG_CONTROL });
                    if ui.add(btn).clicked() {
                        res = opt;
                        app.selected_resolution = opt;
                        settings_dirty = true;
                    }
                }
            });

            ui.add_space(10.0);
            ui.label(RichText::new("Taxa de quadros (FPS)").strong());
            ui.horizontal(|ui| {
                for &opt in FPS_OPTIONS {
                    let selected = fps == opt;
                    let btn = egui::Button::new(
                        RichText::new(format!("{opt} fps"))
                            .color(if selected { Color32::WHITE } else { TEXT_MUTED })
                            .strong(),
                    )
                    .fill(if selected { ACCENT_PRIMARY } else { BG_CONTROL });
                    if ui.add(btn).clicked() {
                        fps = opt;
                        app.selected_fps = opt;
                        settings_dirty = true;
                    }
                }
            });
            ui.label(
                RichText::new("Você poderá alterar resolução, FPS e apps bloqueados a qualquer momento durante a transmissão.")
                    .size(12.0)
                    .color(TEXT_MUTED),
            );

            ui.add_space(12.0);
            ui.label(RichText::new("Otimização de imagem").strong());
            ui.horizontal(|ui| {
                if ui
                    .radio_value(&mut content_type, ContentType::Motion, "Jogo / vídeo (mais fluidez)")
                    .changed()
                {
                    settings_dirty = true;
                }
                if ui
                    .radio_value(&mut content_type, ContentType::Detail, "Texto / código (mais nitidez)")
                    .changed()
                {
                    settings_dirty = true;
                }
            });

            ui.add_space(12.0);
            if ui
                .checkbox(&mut share_audio, "Transmitir áudio do PC (com bloqueio por aplicativo)")
                .changed()
            {
                settings_dirty = true;
            }

            if share_audio {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Apps bloqueados:").strong());
                    if ui.small_button("Atualizar lista").clicked() {
                        app.refresh_audio_apps_async(ctx);
                    }
                });
                if apps_snapshot.is_empty() {
                    ui.label(
                        RichText::new("Discord/Vesktop já ficam bloqueados por padrão. Apps tocando som aparecerão aqui.")
                            .size(12.0)
                            .color(TEXT_MUTED),
                    );
                } else {
                    egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                        for app_entry in &apps_snapshot {
                            let mut is_blocked = blocked.iter().any(|b| b == &app_entry.exe);
                            if ui
                                .checkbox(
                                    &mut is_blocked,
                                    format!("{} ({})", app_entry.name, app_entry.exe),
                                )
                                .changed()
                            {
                                if is_blocked {
                                    if !blocked.iter().any(|b| b == &app_entry.exe) {
                                        blocked.push(app_entry.exe.clone());
                                    }
                                } else {
                                    blocked.retain(|b| b != &app_entry.exe);
                                }
                                settings_dirty = true;
                            }
                        }
                    });
                }
            }

            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(primary_button("Iniciar transmissão").min_size(egui::vec2(160.0, 34.0)))
                    .clicked()
                {
                    start_clicked = true;
                }
                if ui.button("Cancelar").clicked() {
                    cancel_clicked = true;
                }
            });
        });

    if cancel_clicked {
        open = false;
    }

    if settings_dirty {
        let session = app.session.clone();
        let blocked_clone = blocked.clone();
        let qid = res.to_quality_id(fps);
        app.rt.spawn(async move {
            session
                .update_settings(|s| {
                    s.quality = qid;
                    s.content_type = content_type;
                    s.share_audio = share_audio;
                    s.blocked_apps = blocked_clone;
                })
                .await;
        });
    }

    if start_clicked {
        let source_id = app
            .screen_sources
            .get(app.selected_source_idx)
            .map(|s| s.id.clone())
            .unwrap_or_else(|| "screen:0".to_string());
        let opts = ShareOptions {
            source_id,
            resolution: res,
            fps,
            content_type,
            audio: share_audio,
            blocked,
            send_volume: app.snap.settings.send_volume,
        };
        let session = app.session.clone();
        app.rt.spawn(async move {
            let _ = session.start_share(opts).await;
        });
        open = false;
    }

    app.show_share_dialog = open;
}
