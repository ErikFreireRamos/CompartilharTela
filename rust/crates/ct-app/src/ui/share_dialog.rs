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
    let mut draft = ShareDraft {
        res: app.selected_resolution,
        fps: app.selected_fps,
        audio_q: app.selected_audio_quality,
        content_type: app.snap.settings.content_type,
        share_audio: app.snap.settings.share_audio,
        blocked: app.snap.settings.blocked_apps.clone(),
        settings_dirty: false,
    };
    let apps_snapshot = app.audio_apps.lock().map(|g| g.clone()).unwrap_or_default();

    let mut start_clicked = false;
    let mut cancel_clicked = false;

    // Adapta largura e layout ao tamanho atual da janela para evitar scroll sempre que possível
    let screen = ctx.screen_rect();
    let two_cols = screen.width() >= 760.0;
    let dialog_w = if two_cols {
        (screen.width() - 64.0).clamp(680.0, 820.0)
    } else {
        (screen.width() - 40.0).clamp(440.0, 560.0)
    };
    let needs_outer_scroll = !two_cols || screen.height() < 490.0;
    let apps_list_h = if two_cols {
        (screen.height() - 360.0).clamp(96.0, 196.0)
    } else {
        92.0
    };

    Window::new("Configurar transmissão")
        .open(&mut open)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .constrain(true)
        .collapsible(false)
        .resizable(false)
        .fixed_size(vec2(dialog_w, 0.0))
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, BORDER_STRONG))
                .corner_radius(CornerRadius::same(16))
                .inner_margin(egui::Margin::same(16)),
        )
        .show(ctx, |ui| {
            // Cabeçalho + banner recomendado compacto na mesma linha em telas largas
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

                if two_cols {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        egui::Frame::NONE
                            .fill(BG_CARD)
                            .stroke(Stroke::new(1.0_f32, ACCENT_LIVE.linear_multiply(0.6)))
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            mono_text("RECOMENDADO", 9.5).color(ACCENT_LIVE),
                                        );
                                        ui.label(
                                            RichText::new(format!(
                                                "{} · {} fps · {}",
                                                RECOMMENDED_RESOLUTION.label(),
                                                RECOMMENDED_FPS,
                                                RECOMMENDED_AUDIO_QUALITY.short_label()
                                            ))
                                            .size(11.5)
                                            .color(TEXT_PRIMARY),
                                        );
                                    });
                                    if ui
                                        .add(
                                            primary_white_button("Aplicar")
                                                .min_size(vec2(72.0, 26.0)),
                                        )
                                        .clicked()
                                    {
                                        draft.res = RECOMMENDED_RESOLUTION;
                                        draft.fps = RECOMMENDED_FPS;
                                        draft.audio_q = RECOMMENDED_AUDIO_QUALITY;
                                        app.selected_resolution = draft.res;
                                        app.selected_fps = draft.fps;
                                        app.selected_audio_quality = draft.audio_q;
                                        draft.settings_dirty = true;
                                    }
                                });
                            });
                    });
                }
            });

            if !two_cols {
                ui.add_space(8.0);
                egui::Frame::NONE
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0_f32, ACCENT_LIVE.linear_multiply(0.6)))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(12, 7))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                mono_text(
                                    format!(
                                        "RECOMENDADO: {} · {} fps · {}",
                                        RECOMMENDED_RESOLUTION.label(),
                                        RECOMMENDED_FPS,
                                        RECOMMENDED_AUDIO_QUALITY.short_label()
                                    ),
                                    10.5,
                                )
                                .color(ACCENT_LIVE),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(
                                            primary_white_button("Aplicar")
                                                .min_size(vec2(72.0, 26.0)),
                                        )
                                        .clicked()
                                    {
                                        draft.res = RECOMMENDED_RESOLUTION;
                                        draft.fps = RECOMMENDED_FPS;
                                        draft.audio_q = RECOMMENDED_AUDIO_QUALITY;
                                        app.selected_resolution = draft.res;
                                        app.selected_fps = draft.fps;
                                        app.selected_audio_quality = draft.audio_q;
                                        draft.settings_dirty = true;
                                    }
                                },
                            );
                        });
                    });
            }

            ui.add_space(10.0);

            if needs_outer_scroll {
                let body_max_h = (screen.height() - 180.0).clamp(200.0, 420.0);
                egui::ScrollArea::vertical()
                    .max_height(body_max_h)
                    .show(ui, |ui| {
                        render_video_col(app, ui, pal, &mut draft);
                        ui.add_space(10.0);
                        render_audio_col(app, ctx, ui, pal, &apps_snapshot, apps_list_h, &mut draft);
                    });
            } else {
                // Layout horizontal de 2 colunas: cabe inteiro na janela sem scroll!
                ui.columns(2, |cols| {
                    egui::Frame::NONE
                        .fill(BG_CARD)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(12.0)
                        .show(&mut cols[0], |ui| {
                            render_video_col(app, ui, pal, &mut draft);
                        });

                    egui::Frame::NONE
                        .fill(BG_CARD)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(12.0)
                        .show(&mut cols[1], |ui| {
                            render_audio_col(
                                app,
                                ctx,
                                ui,
                                pal,
                                &apps_snapshot,
                                apps_list_h,
                                &mut draft,
                            );
                        });
                });
            }

            // Rodapé sempre fixo e visível
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(primary_white_button("Iniciar transmissão").min_size(vec2(170.0, 34.0)))
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

    let ShareDraft {
        res,
        fps,
        audio_q,
        content_type,
        share_audio,
        blocked,
        settings_dirty,
    } = draft;

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

struct ShareDraft {
    res: ct_core::protocol::ResolutionId,
    fps: u32,
    audio_q: ct_core::protocol::AudioQualityId,
    content_type: ContentType,
    share_audio: bool,
    blocked: Vec<String>,
    settings_dirty: bool,
}

fn render_video_col(
    app: &mut CompartilharApp,
    ui: &mut egui::Ui,
    pal: ThemePalette,
    d: &mut ShareDraft,
) {
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

    if app.gpu_adapters.len() > 2 {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("GPU:").size(12.0).color(TEXT_SECONDARY));
            egui::ComboBox::from_id_salt("gpu_picker")
                .width((ui.available_width() - 4.0).max(150.0))
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
                            d.settings_dirty = true;
                        }
                    }
                });
        });
    }

    ui.add_space(12.0);
    section_heading(ui, "02", "Qualidade da imagem", pal);
    ui.add_space(5.0);

    ui.label(RichText::new("Resolução").size(11.5).color(TEXT_MUTED));
    ui.horizontal_wrapped(|ui| {
        for &opt in RESOLUTIONS {
            let selected = d.res == opt;
            let label = if opt == RECOMMENDED_RESOLUTION {
                format!("{} ★", opt.label())
            } else {
                opt.label().to_string()
            };
            if select_pill(ui, &label, selected).clicked() {
                d.res = opt;
                app.selected_resolution = opt;
                d.settings_dirty = true;
            }
        }
    });

    ui.add_space(6.0);
    ui.label(RichText::new("Taxa de quadros (FPS)").size(11.5).color(TEXT_MUTED));
    ui.horizontal_wrapped(|ui| {
        for &opt in FPS_OPTIONS {
            let selected = d.fps == opt;
            let label = if opt == RECOMMENDED_FPS {
                format!("{opt} fps ★")
            } else {
                format!("{opt} fps")
            };
            if select_pill(ui, &label, selected).clicked() {
                d.fps = opt;
                app.selected_fps = opt;
                d.settings_dirty = true;
            }
        }
    });

    ui.add_space(8.0);
    ui.label(RichText::new("Otimização").size(11.5).color(TEXT_MUTED));
    ui.horizontal_wrapped(|ui| {
        if ui
            .radio_value(&mut d.content_type, ContentType::Motion, "Jogo / vídeo")
            .changed()
        {
            d.settings_dirty = true;
        }
        if ui
            .radio_value(&mut d.content_type, ContentType::Detail, "Texto / código")
            .changed()
        {
            d.settings_dirty = true;
        }
    });
}

fn render_audio_col(
    app: &mut CompartilharApp,
    ctx: &Context,
    ui: &mut egui::Ui,
    pal: ThemePalette,
    apps_snapshot: &[ct_media::audio::AudioApp],
    apps_list_h: f32,
    d: &mut ShareDraft,
) {
    section_heading(ui, "03", "Áudio do PC", pal);
    ui.add_space(5.0);

    if ui
        .checkbox(&mut d.share_audio, "Transmitir som do PC")
        .changed()
    {
        d.settings_dirty = true;
    }

    if d.share_audio {
        ui.add_space(6.0);
        ui.label(RichText::new("Qualidade do áudio").size(11.5).color(TEXT_MUTED));
        ui.horizontal_wrapped(|ui| {
            for &opt in AUDIO_QUALITIES {
                let selected = d.audio_q == opt;
                if select_pill(ui, opt.short_label(), selected)
                    .on_hover_text(opt.label())
                    .clicked()
                {
                    d.audio_q = opt;
                    app.selected_audio_quality = opt;
                    d.settings_dirty = true;
                }
            }
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Silenciar apps:").size(12.0).color(TEXT_SECONDARY));
            if ui.small_button("Atualizar").clicked() {
                app.refresh_audio_apps_async(ctx);
            }
        });
        if apps_snapshot.is_empty() {
            egui::Frame::NONE
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(
                            "Discord e Vesktop já ficam silenciados por padrão. Apps tocando som aparecem aqui.",
                        )
                        .size(11.5)
                        .color(TEXT_MUTED),
                    );
                });
        } else {
            egui::Frame::NONE
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(8.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("share_dialog_apps_scroll")
                        .max_height(apps_list_h)
                        .show(ui, |ui| {
                            for app_entry in apps_snapshot {
                                let mut is_blocked =
                                    d.blocked.iter().any(|b| b == &app_entry.exe);
                                if ui
                                    .checkbox(
                                        &mut is_blocked,
                                        format!("{} ({})", app_entry.name, app_entry.exe),
                                    )
                                    .changed()
                                {
                                    if is_blocked {
                                        if !d.blocked.iter().any(|b| b == &app_entry.exe) {
                                            d.blocked.push(app_entry.exe.clone());
                                        }
                                    } else {
                                        d.blocked.retain(|b| b != &app_entry.exe);
                                    }
                                    d.settings_dirty = true;
                                }
                            }
                        });
                });
        }
    } else {
        ui.add_space(6.0);
        ui.label(
            RichText::new("Apenas vídeo será transmitido (sem som do PC).")
                .size(12.0)
                .color(TEXT_MUTED),
        );
    }
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
