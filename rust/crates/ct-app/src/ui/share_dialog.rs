use ct_core::protocol::{
    AUDIO_QUALITIES, ContentType, FPS_OPTIONS, RECOMMENDED_AUDIO_QUALITY, RECOMMENDED_FPS,
    RECOMMENDED_RESOLUTION, RESOLUTIONS,
};
use ct_media::session::ShareOptions;
use egui::{Align2, Context, CornerRadius, RichText, Stroke, Window, vec2};

use super::CompartilharApp;
use super::theme::{
    ACCENT_LIVE, BG_CARD, BG_CONTROL, BG_SURFACE, BORDER_STRONG, BORDER_SUBTLE, INK_ON_WHITE,
    TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY, ThemePalette, WHITE_PURE, display_text,
    draw_logo_badge, mono_text, primary_white_button,
};

pub fn render_share_dialog(app: &mut CompartilharApp, ctx: &Context) {
    let pal = app.palette();
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

    // Altura máxima do corpo adaptativa para nunca invadir a barra de tarefas do Windows
    let screen_h = ctx.screen_rect().height();
    let body_max_h = (screen_h - 190.0).clamp(220.0, 440.0);

    Window::new("Configurar transmissão")
        .open(&mut open)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .constrain(true)
        .collapsible(false)
        .resizable(false)
        .fixed_size(vec2(540.0, 0.0))
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, BORDER_STRONG))
                .corner_radius(CornerRadius::same(16))
                .inner_margin(egui::Margin::same(16)),
        )
        .show(ctx, |ui| {
            // Cabeçalho compacto com logo PIT
            ui.horizontal(|ui| {
                draw_logo_badge(ui, app.logo_texture.as_ref(), 24.0, false);
                ui.vertical(|ui| {
                    ui.label(display_text("Preparar compartilhamento", 16.0).color(WHITE_PURE));
                    ui.label(
                        RichText::new("Você pode ajustar qualidade e apps ao vivo depois.")
                            .size(11.5)
                            .color(TEXT_MUTED),
                    );
                });
            });
            ui.add_space(8.0);

            // Corpo rolável com altura limitada para sempre caber dentro da janela
            egui::ScrollArea::vertical()
                .max_height(body_max_h)
                .show(ui, |ui| {
                    // Recomendação estável em 1 clique
                    egui::Frame::NONE
                        .fill(BG_CARD)
                        .stroke(Stroke::new(1.0_f32, ACCENT_LIVE.linear_multiply(0.65)))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(
                                        mono_text("PERFIL RECOMENDADO // ESTÁVEL", 10.5)
                                            .color(ACCENT_LIVE),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "{} · {} fps · {}",
                                            RECOMMENDED_RESOLUTION.label(),
                                            RECOMMENDED_FPS,
                                            RECOMMENDED_AUDIO_QUALITY.label()
                                        ))
                                        .size(12.5)
                                        .color(TEXT_PRIMARY),
                                    );
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add(
                                                primary_white_button("Usar recomendado")
                                                    .min_size(vec2(132.0, 28.0)),
                                            )
                                            .clicked()
                                        {
                                            res = RECOMMENDED_RESOLUTION;
                                            fps = RECOMMENDED_FPS;
                                            audio_q = RECOMMENDED_AUDIO_QUALITY;
                                            app.selected_resolution = res;
                                            app.selected_fps = fps;
                                            app.selected_audio_quality = audio_q;
                                            settings_dirty = true;
                                        }
                                    },
                                );
                            });
                        });

                    ui.add_space(10.0);
                    section_heading(ui, "01", "Tela e placa de vídeo", pal);
                    ui.add_space(5.0);

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
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("GPU:")
                                    .size(12.5)
                                    .color(TEXT_SECONDARY),
                            );
                            egui::ComboBox::from_id_salt("gpu_picker")
                                .width((ui.available_width() - 4.0).max(160.0))
                                .selected_text(
                                    app.gpu_adapters
                                        .get(app.selected_gpu_idx)
                                        .map(|g| g.name.as_str())
                                        .unwrap_or("Automática"),
                                )
                                .show_ui(ui, |ui| {
                                    for (idx, gpu) in app.gpu_adapters.iter().enumerate() {
                                        if ui
                                            .selectable_value(
                                                &mut app.selected_gpu_idx,
                                                idx,
                                                &gpu.name,
                                            )
                                            .changed()
                                        {
                                            settings_dirty = true;
                                        }
                                    }
                                });
                        });
                    }

                    ui.add_space(10.0);
                    section_heading(ui, "02", "Qualidade da imagem", pal);
                    ui.add_space(5.0);

                    // Resolução + FPS lado a lado para economizar altura
                    ui.columns(2, |cols| {
                        cols[0].label(
                            RichText::new("Resolução").size(11.5).color(TEXT_MUTED),
                        );
                        cols[0].horizontal_wrapped(|ui| {
                            for &opt in RESOLUTIONS {
                                let selected = res == opt;
                                let label = if opt == RECOMMENDED_RESOLUTION {
                                    format!("{} ★", opt.label())
                                } else {
                                    opt.label().to_string()
                                };
                                if select_pill(ui, &label, selected).clicked() {
                                    res = opt;
                                    app.selected_resolution = opt;
                                    settings_dirty = true;
                                }
                            }
                        });

                        cols[1].label(
                            RichText::new("Taxa de quadros").size(11.5).color(TEXT_MUTED),
                        );
                        cols[1].horizontal_wrapped(|ui| {
                            for &opt in FPS_OPTIONS {
                                let selected = fps == opt;
                                let label = if opt == RECOMMENDED_FPS {
                                    format!("{opt} fps ★")
                                } else {
                                    format!("{opt} fps")
                                };
                                if select_pill(ui, &label, selected).clicked() {
                                    fps = opt;
                                    app.selected_fps = opt;
                                    settings_dirty = true;
                                }
                            }
                        });
                    });

                    ui.add_space(6.0);
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
                    section_heading(ui, "03", "Áudio do PC", pal);
                    ui.add_space(5.0);

                    if ui
                        .checkbox(
                            &mut share_audio,
                            "Transmitir som do PC (com bloqueio de apps para evitar eco)",
                        )
                        .changed()
                    {
                        settings_dirty = true;
                    }

                    if share_audio {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            for &opt in AUDIO_QUALITIES {
                                let selected = audio_q == opt;
                                if select_pill(ui, opt.short_label(), selected)
                                    .on_hover_text(opt.label())
                                    .clicked()
                                {
                                    audio_q = opt;
                                    app.selected_audio_quality = opt;
                                    settings_dirty = true;
                                }
                            }
                        });

                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Silenciar apps específicos:")
                                    .size(12.5)
                                    .color(TEXT_SECONDARY),
                            );
                            if ui.small_button("Atualizar lista").clicked() {
                                app.refresh_audio_apps_async(ctx);
                            }
                        });
                        if apps_snapshot.is_empty() {
                            ui.label(
                                RichText::new(
                                    "Discord/Vesktop já ficam silenciados por padrão. Apps tocando som aparecem aqui.",
                                )
                                .size(11.5)
                                .color(TEXT_MUTED),
                            );
                        } else {
                            egui::Frame::NONE
                                .fill(BG_CARD)
                                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                                .corner_radius(CornerRadius::same(10))
                                .inner_margin(8.0)
                                .show(ui, |ui| {
                                    egui::ScrollArea::vertical()
                                        .id_salt("share_dialog_apps_scroll")
                                        .max_height(96.0)
                                        .show(ui, |ui| {
                                            for app_entry in &apps_snapshot {
                                                let mut is_blocked = blocked
                                                    .iter()
                                                    .any(|b| b == &app_entry.exe);
                                                if ui
                                                    .checkbox(
                                                        &mut is_blocked,
                                                        format!(
                                                            "{} ({})",
                                                            app_entry.name, app_entry.exe
                                                        ),
                                                    )
                                                    .changed()
                                                {
                                                    if is_blocked {
                                                        if !blocked
                                                            .iter()
                                                            .any(|b| b == &app_entry.exe)
                                                        {
                                                            blocked.push(app_entry.exe.clone());
                                                        }
                                                    } else {
                                                        blocked.retain(|b| b != &app_entry.exe);
                                                    }
                                                    settings_dirty = true;
                                                }
                                            }
                                        });
                                });
                        }
                    }
                });

            // Rodapé sempre fixo e visível na base do modal
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        primary_white_button("Iniciar transmissão").min_size(vec2(170.0, 34.0)),
                    )
                    .clicked()
                {
                    start_clicked = true;
                }
                if ui
                    .add(egui::Button::new("Cancelar").min_size(vec2(90.0, 34.0)))
                    .clicked()
                {
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

fn section_heading(ui: &mut egui::Ui, num: &str, title: &str, pal: ThemePalette) {
    ui.horizontal(|ui| {
        egui::Frame::NONE
            .fill(pal.accent)
            .corner_radius(CornerRadius::same(4))
            .inner_margin(egui::Margin::symmetric(5, 1))
            .show(ui, |ui| {
                ui.label(mono_text(num, 10.0).color(pal.accent_ink));
            });
        ui.label(display_text(title, 13.5).color(WHITE_PURE));
    });
}

fn select_pill(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let btn = egui::Button::new(if selected {
        display_text(label, 12.0).color(INK_ON_WHITE)
    } else {
        RichText::new(label).size(12.0).color(TEXT_SECONDARY)
    })
    .fill(if selected { WHITE_PURE } else { BG_CONTROL })
    .stroke(Stroke::new(
        1.0_f32,
        if selected { WHITE_PURE } else { BORDER_SUBTLE },
    ))
    .corner_radius(CornerRadius::same(8))
    .min_size(vec2(0.0, 27.0));
    ui.add(btn)
}
