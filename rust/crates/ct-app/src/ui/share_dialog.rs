use ct_core::protocol::{
    ContentType, allowed_presets, clamp_quality, host_upload_mbps, preset_by_id,
};
use ct_media::session::ShareOptions;
use egui::{Context, Window};

use super::CompartilharApp;

pub fn render_share_dialog(app: &mut CompartilharApp, ctx: &Context) {
    let mut open = app.show_share_dialog;
    let max_q = app.snap.meta.as_ref().and_then(|m| m.max_quality);
    let allowed = allowed_presets(max_q);
    let mut quality = clamp_quality(app.snap.settings.quality, max_q);
    let mut content_type = app.snap.settings.content_type;
    let mut share_audio = app.snap.settings.share_audio;
    let mut blocked = app.snap.settings.blocked_apps.clone();
    let mut start_clicked = false;
    let mut cancel_clicked = false;
    let mut settings_dirty = false;

    Window::new("Compartilhar tela")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .min_width(440.0)
        .show(ctx, |ui| {
            ui.label("Tela");
            egui::ComboBox::from_id_salt("screen_picker")
                .selected_text(
                    app.screen_sources
                        .get(app.selected_source_idx)
                        .map(|s| s.name.as_str())
                        .unwrap_or("Nenhuma tela"),
                )
                .show_ui(ui, |ui| {
                    for (idx, src) in app.screen_sources.iter().enumerate() {
                        ui.selectable_value(&mut app.selected_source_idx, idx, &src.name);
                    }
                });

            ui.add_space(10.0);
            ui.label("Qualidade");
            egui::ComboBox::from_id_salt("quality_picker")
                .selected_text(preset_by_id(quality).label)
                .show_ui(ui, |ui| {
                    for p in allowed {
                        if ui.selectable_value(&mut quality, p.id, p.label).changed() {
                            settings_dirty = true;
                        }
                    }
                });

            let est_mbps = host_upload_mbps(preset_by_id(quality), app.snap.streams.len() + 1);
            ui.small(format!("Upload estimado do host (sala cheia): ~{est_mbps} Mbps"));

            ui.add_space(10.0);
            ui.label("Tipo de conteúdo");
            ui.horizontal(|ui| {
                if ui
                    .radio_value(&mut content_type, ContentType::Motion, "Jogo / vídeo (H.264 HW)")
                    .changed()
                {
                    settings_dirty = true;
                }
                if ui
                    .radio_value(&mut content_type, ContentType::Detail, "Texto / código (VP9)")
                    .changed()
                {
                    settings_dirty = true;
                }
            });

            ui.add_space(10.0);
            if ui
                .checkbox(&mut share_audio, "Transmitir áudio do PC (com bloqueio por app)")
                .changed()
            {
                settings_dirty = true;
            }

            if share_audio && !app.audio_apps.is_empty() {
                ui.add_space(6.0);
                ui.label("Apps bloqueados (não enviados):");
                for app_entry in &app.audio_apps {
                    let mut is_blocked = blocked.iter().any(|b| b == &app_entry.exe);
                    if ui
                        .checkbox(&mut is_blocked, format!("{} ({})", app_entry.name, app_entry.exe))
                        .changed()
                    {
                        if is_blocked {
                            blocked.push(app_entry.exe.clone());
                        } else {
                            blocked.retain(|b| b != &app_entry.exe);
                        }
                        settings_dirty = true;
                    }
                }
            }

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if ui.button("Iniciar transmissão").clicked() {
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
        app.rt.spawn(async move {
            session
                .update_settings(|s| {
                    s.quality = quality;
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
            quality,
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
