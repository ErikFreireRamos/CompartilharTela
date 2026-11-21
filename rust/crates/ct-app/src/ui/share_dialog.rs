use ct_core::protocol::{
    AUDIO_QUALITIES, ContentType, FPS_OPTIONS, RECOMMENDED_AUDIO_QUALITY, RECOMMENDED_FPS,
    RECOMMENDED_RESOLUTION, RESOLUTIONS,
};
use ct_media::session::ShareOptions;
use egui::{Color32, Context, RichText, Stroke, Window};

use super::{
    ACCENT_GREEN, ACCENT_PRIMARY, BG_CARD, BG_CONTROL, CompartilharApp, TEXT_MUTED, primary_button,
};

pub fn render_share_dialog(app: &mut CompartilharApp, ctx: &Context) {
    let mut open = app.show_share_dialog;
    let mut res = app.selected_resolution;
    let mut fps = app.selected_fps;
    let mut audio_q = app.selected_audio_quality;
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
        .min_width(500.0)
        .show(ctx, |ui| {
            // Recomendação estável de qualidade com botão rápido
            egui::Frame::NONE
                .fill(BG_CARD)
                .stroke(Stroke::new(1.0_f32, ACCENT_GREEN))
                .corner_radius(8.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("Recomendado para maior estabilidade:")
                                    .strong()
                                    .color(ACCENT_GREEN),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{} · {} fps · {}",
                                    RECOMMENDED_RESOLUTION.label(),
                                    RECOMMENDED_FPS,
                                    RECOMMENDED_AUDIO_QUALITY.label()
                                ))
                                .size(12.5)
                                .color(TEXT_MUTED),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Usar recomendado").clicked() {
                                res = RECOMMENDED_RESOLUTION;
                                fps = RECOMMENDED_FPS;
                                audio_q = RECOMMENDED_AUDIO_QUALITY;
                                app.selected_resolution = res;
                                app.selected_fps = fps;
                                app.selected_audio_quality = audio_q;
                                settings_dirty = true;
                            }
                        });
                    });
                });

            ui.add_space(10.0);
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

            // Seleção de GPU quando houver mais de 1 GPU disponível no PC
            if app.gpu_adapters.len() > 2 {
                ui.add_space(10.0);
                ui.label(RichText::new("GPU de captura").strong());
                egui::ComboBox::from_id_salt("gpu_picker")
                    .width(ui.available_width())
                    .selected_text(
                        app.gpu_adapters
                            .get(app.selected_gpu_idx)
                            .map(|g| g.name.as_str())
                            .unwrap_or("Automática"),
                    )
                    .show_ui(ui, |ui| {
                        for (idx, gpu) in app.gpu_adapters.iter().enumerate() {
                            if ui
                                .selectable_value(&mut app.selected_gpu_idx, idx, &gpu.name)
                                .changed()
                            {
                                settings_dirty = true;
                            }
                        }
                    });
            }

            ui.add_space(10.0);
            ui.label(RichText::new("Resolução").strong());
            ui.horizontal(|ui| {
                for &opt in RESOLUTIONS {
                    let selected = res == opt;
                    let label = if opt == RECOMMENDED_RESOLUTION {
                        format!("{} ★", opt.label())
                    } else {
                        opt.label().to_string()
                    };
                    let btn = egui::Button::new(
                        RichText::new(label)
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
                    let label = if opt == RECOMMENDED_FPS {
                        format!("{opt} fps ★")
                    } else {
                        format!("{opt} fps")
                    };
                    let btn = egui::Button::new(
                        RichText::new(label)
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

            ui.add_space(10.0);
            ui.label(RichText::new("Otimização de imagem").strong());
            ui.horizontal(|ui| {
                if ui
                    .radio_value(
                        &mut content_type,
                        ContentType::Motion,
                        "Jogo / vídeo (mais fluidez)",
                    )
                    .changed()
                {
                    settings_dirty = true;
                }
                if ui
                    .radio_value(
                        &mut content_type,
                        ContentType::Detail,
                        "Texto / código (mais nitidez)",
                    )
                    .changed()
                {
                    settings_dirty = true;
                }
            });

            ui.add_space(10.0);
            if ui
                .checkbox(
                    &mut share_audio,
                    "Transmitir áudio do PC (com bloqueio por aplicativo)",
                )
                .changed()
            {
                settings_dirty = true;
            }

            if share_audio {
                ui.add_space(6.0);
                ui.label(RichText::new("Qualidade do áudio").strong());
                ui.horizontal(|ui| {
                    for &opt in AUDIO_QUALITIES {
                        let selected = audio_q == opt;
                        let btn = egui::Button::new(
                            RichText::new(opt.label())
                                .color(if selected { Color32::WHITE } else { TEXT_MUTED })
                                .size(12.5),
                        )
                        .fill(if selected { ACCENT_PRIMARY } else { BG_CONTROL });
                        if ui.add(btn).clicked() {
                            audio_q = opt;
                            app.selected_audio_quality = opt;
                            settings_dirty = true;
                        }
                    }
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Apps bloqueados:").strong());
                    if ui.small_button("Atualizar lista").clicked() {
                        app.refresh_audio_apps_async(ctx);
                    }
                });
                if apps_snapshot.is_empty() {
                    ui.label(
                        RichText::new(
                            "Discord/Vesktop já ficam bloqueados por padrão. Apps tocando som aparecerão aqui.",
                        )
                        .size(12.0)
                        .color(TEXT_MUTED),
                    );
                } else {
                    egui::ScrollArea::vertical().max_height(130.0).show(ui, |ui| {
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

            ui.add_space(14.0);
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

    let chosen_gpu = app
        .gpu_adapters
        .get(app.selected_gpu_idx)
        .and_then(|g| g.index);

    if settings_dirty {
        let session = app.session.clone();
        let blocked_clone = blocked.clone();
        let qid = res.to_quality_id(fps);
        app.rt.spawn(async move {
            session
                .update_settings(|s| {
                    s.quality = qid;
                    s.fps = fps;
                    s.audio_quality = audio_q;
                    s.gpu_adapter_index = chosen_gpu;
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
            audio_quality: audio_q,
            gpu_adapter_index: chosen_gpu,
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
