use std::time::{Duration, Instant};

use ct_core::protocol::{AUDIO_QUALITIES, FPS_OPTIONS, RESOLUTIONS};
use ct_media::audio::AudioMode;
use ct_media::session::{ReceiveQuality, SessionSnapshot, StreamView};
use egui::{
    Align, Align2, Color32, Context, CornerRadius, Key, Layout, Order, RichText, Stroke,
    ViewportBuilder, ViewportCommand, ViewportId, pos2, vec2,
};

use super::theme::{
    ACCENT_LIVE, ACCENT_RED, ACCENT_RED_BRIGHT, ACCENT_RED_DIM, ACCENT_WARN, APP_NAME, BG_CARD,
    BG_CONTROL, BG_OVERLAY, BG_SURFACE, BG_VOID, BORDER_STRONG, BORDER_SUBTLE, IconKind,
    PillVariant, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY, WHITE_PURE, display_text,
    draw_logo_badge, draw_status_dot, floating_card_shadow, icon_pill_button, mono_text,
    paint_icon, primary_white_button, render_admin_warning,
};
use super::{CompartilharApp, share_dialog};

pub fn render_room(app: &mut CompartilharApp, ctx: &Context) {
    let snap = app.snap.clone();
    let copied = app
        .copied_until
        .is_some_and(|until| Instant::now() < until);

    // Atalho ESC sai da tela cheia na janela principal
    if app.fullscreen_stream_id.is_some() && ctx.input(|i| i.key_pressed(Key::Escape)) {
        app.exit_main_fullscreen(ctx);
    }

    // =========================================================================
    // Modo Tela Cheia na janela principal (vídeo 100% + pílula flutuante no topo)
    // =========================================================================
    if let Some(fs_id) = app.fullscreen_stream_id.clone()
        && let Some(stream) = snap.streams.iter().find(|s| s.identity == fs_id)
    {
        let mut exit_fs = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::BLACK))
            .show(ctx, |ui| {
                let avail = ui.available_size();
                let tex_info = app
                    .textures
                    .get(&stream.identity)
                    .map(|(_, tex)| (tex.id(), tex.aspect_ratio().max(0.1)));
                if let Some((tex_id, aspect)) = tex_info {
                    let w = avail.x.min(avail.y * aspect).max(160.0);
                    let h = (w / aspect).max(90.0);
                    ui.centered_and_justified(|ui| {
                        let resp = ui.add(
                            egui::Image::new((tex_id, vec2(w, h))).sense(egui::Sense::click()),
                        );
                        if resp.double_clicked() {
                            exit_fs = true;
                        }
                    });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("Aguardando vídeo…").color(TEXT_MUTED));
                    });
                }
            });

        // Pílula flutuante translúcida no topo da tela cheia
        egui::Area::new(egui::Id::new("main_fs_overlay"))
            .anchor(Align2::CENTER_TOP, vec2(0.0, 14.0))
            .order(Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(BG_OVERLAY)
                    .stroke(Stroke::new(1.0_f32, BORDER_STRONG))
                    .corner_radius(CornerRadius::same(14))
                    .shadow(floating_card_shadow())
                    .inner_margin(egui::Margin::symmetric(12, 7))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            draw_logo_badge(ui, app.logo_texture.as_ref(), 18.0, false);
                            draw_status_dot(ui, ACCENT_RED, 4.0);
                            let title = if stream.is_local {
                                "Você".to_string()
                            } else {
                                stream.name.clone()
                            };
                            ui.label(display_text(title, 13.5).color(WHITE_PURE));

                            if !stream.is_local {
                                ui.separator();
                                let mut vol = snap.volume_of(&stream.name);
                                let (icon_r, _) =
                                    ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                                paint_icon(
                                    ui.painter(),
                                    icon_r,
                                    if vol <= 0.01 {
                                        IconKind::VolumeMute
                                    } else {
                                        IconKind::Volume
                                    },
                                    TEXT_SECONDARY,
                                );
                                if ui
                                    .add(
                                        egui::Slider::new(&mut vol, 0.0..=2.0)
                                            .show_value(false)
                                            .fixed_decimals(2),
                                    )
                                    .changed()
                                {
                                    let session = app.session.clone();
                                    let id = stream.identity.clone();
                                    let name = stream.name.clone();
                                    app.rt.spawn(async move {
                                        session.set_peer_volume(id, name, vol).await;
                                    });
                                }
                                ui.label(
                                    mono_text(format!("{}%", (vol * 100.0).round()), 11.5)
                                        .color(TEXT_SECONDARY),
                                );
                            }

                            ui.add_space(6.0);
                            if icon_pill_button(
                                ui,
                                IconKind::ExitFullscreen,
                                Some("Sair (Esc)"),
                                PillVariant::ActiveWhite,
                                28.0,
                            )
                            .clicked()
                            {
                                exit_fs = true;
                            }
                        });
                    });
            });

        if exit_fs {
            app.exit_main_fullscreen(ctx);
        }
        render_popout_windows(app, ctx, &snap);
        return;
    }

    // =========================================================================
    // Barra Superior minimalista (logo PIT + convite central + botões flutuantes à direita)
    // =========================================================================
    egui::TopBottomPanel::top("room_header")
        .frame(
            egui::Frame::NONE
                .fill(BG_VOID)
                .inner_margin(egui::Margin {
                    left: 14,
                    right: 14,
                    top: 10,
                    bottom: 6,
                }),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Esquerda: Marca PIT + Host
                draw_logo_badge(ui, app.logo_texture.as_ref(), 26.0, false);
                ui.add_space(2.0);
                ui.label(display_text(APP_NAME, 19.0).color(WHITE_PURE));

                // Pequena barra vertical vermelha tática
                let (bar_r, _) = ui.allocate_exact_size(vec2(2.0, 18.0), egui::Sense::hover());
                ui.painter().rect_filled(bar_r, 1.0, ACCENT_RED);

                let host_name = snap
                    .meta
                    .as_ref()
                    .map(|m| m.host_name.as_str())
                    .unwrap_or("…");
                egui::Frame::NONE
                    .fill(BG_SURFACE)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(9, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            ui.label(mono_text("HOST", 10.0).color(ACCENT_WARN));
                            ui.label(RichText::new(host_name).size(12.5).color(TEXT_PRIMARY));
                        });
                    });

                ui.add_space(8.0);

                // Pílula de convite clicável (super intuitiva para usuários leigos)
                let full_code = snap.code();
                let short_code = if full_code.len() > 16 {
                    format!("{}…", &full_code[..14])
                } else {
                    full_code.to_string()
                };
                let invite_label = if copied {
                    "Convite copiado!".to_string()
                } else {
                    format!("Copiar convite  ·  {short_code}")
                };
                let invite_resp = icon_pill_button(
                    ui,
                    if copied {
                        IconKind::Check
                    } else {
                        IconKind::Copy
                    },
                    Some(&invite_label),
                    if copied {
                        PillVariant::ActiveWhite
                    } else {
                        PillVariant::Dark
                    },
                    32.0,
                )
                .on_hover_text("Clique para copiar o código da sala e enviar para seus amigos");
                if invite_resp.clicked() {
                    ctx.copy_text(full_code.to_string());
                    app.copied_until = Some(Instant::now() + Duration::from_millis(2000));
                }

                // Direita: Botões flutuantes estilo print de referência + avatar PIT
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Avatar/logo redondo no canto superior direito
                    draw_logo_badge(ui, app.logo_texture.as_ref(), 24.0, true);

                    // Botão Participantes (mostra ou minimiza a aba lateral direita)
                    let part_label = format!("Participantes ({})", snap.peers.len());
                    if icon_pill_button(
                        ui,
                        IconKind::People,
                        Some(&part_label),
                        if app.show_participants {
                            PillVariant::ActiveWhite
                        } else {
                            PillVariant::Dark
                        },
                        32.0,
                    )
                    .on_hover_text("Mostrar ou ocultar o painel de participantes")
                    .clicked()
                    {
                        app.show_participants = !app.show_participants;
                    }

                    // Atalho rápido de Apps Bloqueados quando está transmitindo áudio nativo
                    if snap
                        .share
                        .as_ref()
                        .is_some_and(|s| s.audio_mode == AudioMode::Native)
                        && icon_pill_button(
                            ui,
                            IconKind::Shield,
                            Some("Apps silenciados"),
                            if app.show_audio_popover {
                                PillVariant::ActiveWhite
                            } else {
                                PillVariant::Dark
                            },
                            32.0,
                        )
                        .on_hover_text("Escolher quais apps não devem ter o som transmitido")
                        .clicked()
                    {
                        app.refresh_audio_apps_async(ctx);
                        app.show_audio_popover = !app.show_audio_popover;
                    }
                });
            });
        });

    // =========================================================================
    // Dock Flutuante Inferior (controles ao vivo + marca PIT + Destaque/Grade + Parar/Sair)
    // =========================================================================
    egui::TopBottomPanel::bottom("room_floating_dock")
        .frame(
            egui::Frame::NONE
                .fill(BG_VOID)
                .inner_margin(egui::Margin {
                    left: 14,
                    right: 14,
                    top: 6,
                    bottom: 12,
                }),
        )
        .show(ctx, |ui| {
            let dock_resp = egui::Frame::NONE
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, BORDER_STRONG))
                .corner_radius(CornerRadius::same(16))
                .shadow(floating_card_shadow())
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        // Lado esquerdo: Controles de transmissão (ou botão grande Compartilhar tela)
                        if let Some(ref share) = snap.share {
                            // Badge AO VIVO em vermelho tático
                            egui::Frame::NONE
                                .fill(ACCENT_RED_DIM)
                                .stroke(Stroke::new(1.0_f32, ACCENT_RED))
                                .corner_radius(CornerRadius::same(8))
                                .inner_margin(egui::Margin::symmetric(9, 5))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        draw_status_dot(ui, ACCENT_RED_BRIGHT, 3.8);
                                        ui.label(
                                            mono_text("AO VIVO", 11.0).color(WHITE_PURE),
                                        );
                                    });
                                });

                            // Resolução ao vivo
                            let cur_res = share.resolution;
                            let cur_fps = share.fps;
                            egui::ComboBox::from_id_salt("live_res")
                                .width(76.0)
                                .selected_text(cur_res.label())
                                .show_ui(ui, |ui| {
                                    for &res in RESOLUTIONS {
                                        if ui
                                            .selectable_label(cur_res == res, res.label())
                                            .clicked()
                                        {
                                            app.selected_resolution = res;
                                            let session = app.session.clone();
                                            app.rt.spawn(async move {
                                                session.update_share_quality(res, cur_fps).await;
                                            });
                                        }
                                    }
                                });

                            // FPS ao vivo
                            egui::ComboBox::from_id_salt("live_fps")
                                .width(74.0)
                                .selected_text(format!("{cur_fps} fps"))
                                .show_ui(ui, |ui| {
                                    for &fps in FPS_OPTIONS {
                                        if ui
                                            .selectable_label(cur_fps == fps, format!("{fps} fps"))
                                            .clicked()
                                        {
                                            app.selected_fps = fps;
                                            let session = app.session.clone();
                                            app.rt.spawn(async move {
                                                session.update_share_quality(cur_res, fps).await;
                                            });
                                        }
                                    }
                                });

                            // Qualidade de áudio + volume ao vivo
                            if share.audio_mode != AudioMode::None {
                                let cur_aq = share.audio_quality;
                                egui::ComboBox::from_id_salt("live_audio_q")
                                    .width(114.0)
                                    .selected_text(cur_aq.short_label())
                                    .show_ui(ui, |ui| {
                                        for &aq in AUDIO_QUALITIES {
                                            if ui
                                                .selectable_label(cur_aq == aq, aq.label())
                                                .clicked()
                                            {
                                                app.selected_audio_quality = aq;
                                                let session = app.session.clone();
                                                app.rt.spawn(async move {
                                                    session.set_audio_quality(aq).await;
                                                });
                                            }
                                        }
                                    });

                                let mut send_vol = snap.settings.send_volume;
                                let (icon_r, _) = ui
                                    .allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                                paint_icon(
                                    ui.painter(),
                                    icon_r,
                                    if send_vol <= 0.01 {
                                        IconKind::VolumeMute
                                    } else {
                                        IconKind::Volume
                                    },
                                    TEXT_SECONDARY,
                                );
                                if ui
                                    .add(
                                        egui::Slider::new(&mut send_vol, 0.0..=2.0)
                                            .show_value(false)
                                            .fixed_decimals(2),
                                    )
                                    .changed()
                                {
                                    let session = app.session.clone();
                                    app.rt.spawn(async move {
                                        session.set_send_volume(send_vol).await;
                                    });
                                }
                                ui.label(
                                    mono_text(format!("{}%", (send_vol * 100.0).round()), 11.5)
                                        .color(TEXT_SECONDARY),
                                );
                            }
                        } else {
                            // Não está transmitindo: botão branco WSTR de alto contraste
                            if icon_pill_button(
                                ui,
                                IconKind::Screen,
                                Some("Compartilhar tela"),
                                PillVariant::ActiveWhite,
                                36.0,
                            )
                            .clicked()
                            {
                                app.open_share_dialog(ctx);
                            }

                            ui.label(
                                RichText::new("Escolha monitor, qualidade e som dos apps")
                                    .size(12.5)
                                    .color(TEXT_MUTED),
                            );
                        }

                        // Lado direito: Destaque/Grade + Parar + Sair
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if icon_pill_button(
                                ui,
                                IconKind::Leave,
                                Some("Sair"),
                                PillVariant::GhostDanger,
                                34.0,
                            )
                            .on_hover_text("Sair da sala (se você for o host, outra pessoa assume)")
                            .clicked()
                            {
                                let session = app.session.clone();
                                app.rt.spawn(async move {
                                    session.leave_room().await;
                                });
                            }

                            if snap.share.is_some()
                                && icon_pill_button(
                                    ui,
                                    IconKind::Stop,
                                    Some("Parar"),
                                    PillVariant::TacticalRed,
                                    34.0,
                                )
                                .on_hover_text("Encerrar sua transmissão de tela")
                                .clicked()
                            {
                                let session = app.session.clone();
                                app.rt.spawn(async move { session.stop_share().await });
                            }

                            // Alternador segmentado Destaque / Grade
                            if !snap.streams.is_empty() {
                                if icon_pill_button(
                                    ui,
                                    IconKind::Grid,
                                    Some("Grade"),
                                    if app.grid_mode {
                                        PillVariant::ActiveWhite
                                    } else {
                                        PillVariant::Dark
                                    },
                                    34.0,
                                )
                                .on_hover_text("Ver todas as telas lado a lado")
                                .clicked()
                                {
                                    app.grid_mode = true;
                                }

                                if icon_pill_button(
                                    ui,
                                    IconKind::Spotlight,
                                    Some("Destaque"),
                                    if !app.grid_mode {
                                        PillVariant::ActiveWhite
                                    } else {
                                        PillVariant::Dark
                                    },
                                    34.0,
                                )
                                .on_hover_text("Focar uma tela grande com miniaturas embaixo")
                                .clicked()
                                {
                                    app.grid_mode = false;
                                }
                            }
                        });
                    });
                });

            // Marca PIT centralizada no dock flutuante (estilo print de referência) quando há folga horizontal
            let dock_rect = dock_resp.response.rect;
            let show_center_brand = (snap.share.is_none() && dock_rect.width() >= 960.0)
                || dock_rect.width() >= 1240.0;
            if show_center_brand {
                let brand_rect = egui::Rect::from_center_size(dock_rect.center(), vec2(88.0, 26.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(brand_rect), |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        draw_logo_badge(ui, app.logo_texture.as_ref(), 20.0, false);
                        ui.label(
                            display_text(APP_NAME, 15.0)
                                .color(WHITE_PURE.linear_multiply(0.82)),
                        );
                    });
                });
            }
        });

    // =========================================================================
    // Painel Lateral Direito Flutuante (Participantes — minimizável)
    // =========================================================================
    if app.show_participants {
        egui::SidePanel::right("participants")
            .min_width(252.0)
            .default_width(268.0)
            .resizable(false)
            .frame(
                egui::Frame::NONE.fill(BG_VOID).inner_margin(egui::Margin {
                    left: 4,
                    right: 14,
                    top: 6,
                    bottom: 6,
                }),
            )
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(BG_SURFACE)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .corner_radius(CornerRadius::same(16))
                    .shadow(floating_card_shadow())
                    .inner_margin(egui::Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_min_height(ui.available_height());

                        // Cabeçalho do card lateral
                        ui.horizontal(|ui| {
                            let (bar_r, _) =
                                ui.allocate_exact_size(vec2(3.0, 16.0), egui::Sense::hover());
                            ui.painter().rect_filled(bar_r, 1.5, ACCENT_RED);

                            ui.label(display_text("Participantes", 15.0).color(WHITE_PURE));
                            egui::Frame::NONE
                                .fill(BG_CONTROL)
                                .corner_radius(CornerRadius::same(6))
                                .inner_margin(egui::Margin::symmetric(6, 1))
                                .show(ui, |ui| {
                                    ui.label(
                                        mono_text(format!("{}", snap.peers.len()), 11.0)
                                            .color(TEXT_SECONDARY),
                                    );
                                });

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if icon_pill_button(
                                    ui,
                                    IconKind::ChevronRight,
                                    None,
                                    PillVariant::Dark,
                                    26.0,
                                )
                                .on_hover_text("Minimizar painel de participantes")
                                .clicked()
                                {
                                    app.show_participants = false;
                                }
                            });
                        });

                        ui.add_space(10.0);

                        egui::ScrollArea::vertical()
                            .id_salt("participants_scroll")
                            .show(ui, |ui| {
                                for p in &snap.peers {
                                    egui::Frame::NONE
                                        .fill(BG_CARD)
                                        .stroke(Stroke::new(
                                            1.0_f32,
                                            if p.sharing {
                                                ACCENT_RED.linear_multiply(0.55)
                                            } else {
                                                BORDER_SUBTLE
                                            },
                                        ))
                                        .corner_radius(CornerRadius::same(12))
                                        .inner_margin(egui::Margin::symmetric(10, 9))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                draw_peer_avatar(ui, &p.name, p.sharing);
                                                ui.vertical(|ui| {
                                                    ui.horizontal(|ui| {
                                                        let suffix =
                                                            if p.is_local { " (você)" } else { "" };
                                                        ui.label(
                                                            display_text(
                                                                format!("{}{suffix}", p.name),
                                                                13.5,
                                                            )
                                                            .color(WHITE_PURE),
                                                        );
                                                    });
                                                    ui.horizontal(|ui| {
                                                        if p.is_host {
                                                            role_pill(ui, "HOST", ACCENT_WARN);
                                                        } else if p.can_host == Some(true) {
                                                            role_pill(ui, "RESERVA", TEXT_MUTED);
                                                        }
                                                        if p.sharing {
                                                            role_pill(
                                                                ui,
                                                                "AO VIVO",
                                                                ACCENT_RED_BRIGHT,
                                                            );
                                                        }
                                                    });
                                                });

                                                if p.sharing {
                                                    ui.with_layout(
                                                        Layout::right_to_left(Align::Center),
                                                        |ui| {
                                                            let btn = egui::Button::new(
                                                                display_text("Ver", 11.5)
                                                                    .color(Color32::BLACK),
                                                            )
                                                            .fill(WHITE_PURE)
                                                            .stroke(Stroke::NONE)
                                                            .corner_radius(CornerRadius::same(7))
                                                            .min_size(vec2(42.0, 24.0));
                                                            if ui.add(btn).clicked() {
                                                                app.focused_id =
                                                                    Some(p.identity.clone());
                                                                app.grid_mode = false;
                                                            }
                                                        },
                                                    );
                                                }
                                            });
                                        });
                                    ui.add_space(7.0);
                                }
                            });
                    });
            });
    }

    // =========================================================================
    // Palco Principal de Vídeo (Destaque ou Grade com cantos arredondados e overlays)
    // =========================================================================
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(BG_VOID).inner_margin(egui::Margin {
            left: 14,
            right: if app.show_participants { 8 } else { 14 },
            top: 6,
            bottom: 6,
        }))
        .show(ctx, |ui| {
            if let Some(ref notice) = snap.notice {
                egui::Frame::NONE
                    .fill(BG_SURFACE)
                    .stroke(Stroke::new(1.0_f32, WHITE_PURE.linear_multiply(0.45)))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.label(RichText::new(notice).color(WHITE_PURE));
                    });
                ui.add_space(8.0);
            }
            render_admin_warning(ui, snap.elevated);

            if let Some(ref warn) = snap.share.as_ref().and_then(|s| s.warning.as_ref()) {
                egui::Frame::NONE
                    .fill(Color32::from_rgb(56, 42, 16))
                    .stroke(Stroke::new(1.0_f32, ACCENT_WARN))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.colored_label(Color32::from_rgb(255, 232, 175), *warn);
                    });
                ui.add_space(8.0);
            }

            let docked_streams: Vec<&StreamView> = snap
                .streams
                .iter()
                .filter(|s| !app.popped_out.contains(&s.identity))
                .collect();

            if snap.streams.is_empty() {
                // Estado vazio amigável com logo PIT em destaque
                ui.centered_and_justified(|ui| {
                    egui::Frame::NONE
                        .fill(BG_SURFACE)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .corner_radius(CornerRadius::same(18))
                        .shadow(floating_card_shadow())
                        .inner_margin(egui::Margin::same(32))
                        .show(ui, |ui| {
                            ui.set_max_width(430.0);
                            ui.vertical_centered(|ui| {
                                draw_logo_badge(ui, app.logo_texture.as_ref(), 62.0, true);
                                ui.add_space(14.0);
                                ui.label(
                                    display_text("Sala pronta para transmitir", 21.0)
                                        .color(WHITE_PURE),
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(
                                        "Ninguém está compartilhando a tela ainda. Inicie sua transmissão ou envie o convite para seus amigos.",
                                    )
                                    .size(13.5)
                                    .color(TEXT_SECONDARY),
                                );
                                ui.add_space(20.0);
                                ui.horizontal(|ui| {
                                    let pad = ((ui.available_width() - 340.0) * 0.5).max(0.0);
                                    ui.add_space(pad);
                                    if ui
                                        .add(
                                            primary_white_button("Compartilhar minha tela")
                                                .min_size(vec2(196.0, 40.0)),
                                        )
                                        .clicked()
                                    {
                                        app.open_share_dialog(ctx);
                                    }
                                    if icon_pill_button(
                                        ui,
                                        if copied {
                                            IconKind::Check
                                        } else {
                                            IconKind::Copy
                                        },
                                        Some(if copied { "Copiado!" } else { "Convite" }),
                                        PillVariant::Dark,
                                        40.0,
                                    )
                                    .clicked()
                                    {
                                        ctx.copy_text(snap.code().to_string());
                                        app.copied_until =
                                            Some(Instant::now() + Duration::from_millis(2000));
                                    }
                                });
                            });
                        });
                });
            } else if docked_streams.is_empty() {
                ui.centered_and_justified(|ui| {
                    egui::Frame::NONE
                        .fill(BG_SURFACE)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .corner_radius(CornerRadius::same(16))
                        .inner_margin(24.0)
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    display_text(
                                        "Todas as transmissões estão em janelas separadas",
                                        16.5,
                                    )
                                    .color(WHITE_PURE),
                                );
                                ui.add_space(12.0);
                                if ui
                                    .add(
                                        primary_white_button("Trazer todas de volta")
                                            .min_size(vec2(190.0, 36.0)),
                                    )
                                    .clicked()
                                {
                                    app.popped_out.clear();
                                }
                            });
                        });
                });
            } else if app.grid_mode {
                // Layout Grade (inspirado diretamente na print 2x2!)
                let count = docked_streams.len();
                let cols = if count <= 1 {
                    1
                } else if count <= 4 {
                    2
                } else {
                    3
                };
                let rows = count.div_ceil(cols);
                let avail = ui.available_size();
                let spacing = 12.0;
                let cell_w =
                    ((avail.x - spacing * (cols as f32 - 1.0)) / cols as f32).max(260.0);
                let cell_h =
                    ((avail.y - spacing * (rows as f32 - 1.0)) / rows as f32).max(200.0);

                egui::ScrollArea::both().show(ui, |ui| {
                    for row_chunk in docked_streams.chunks(cols) {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = spacing;
                            for stream in row_chunk {
                                ui.allocate_ui(vec2(cell_w, cell_h), |ui| {
                                    render_stream_tile(
                                        app, ctx, ui, &snap, stream, true, false, false,
                                    );
                                });
                            }
                        });
                        ui.add_space(spacing);
                    }
                });
            } else {
                // Layout Destaque (tela principal grande + faixa horizontal de miniaturas clicáveis)
                let focused_id = app
                    .focused_id
                    .clone()
                    .filter(|id| docked_streams.iter().any(|s| &s.identity == id))
                    .or_else(|| {
                        docked_streams
                            .iter()
                            .find(|s| !s.is_local)
                            .map(|s| s.identity.clone())
                    })
                    .or_else(|| docked_streams.first().map(|s| s.identity.clone()));

                let focused_stream = focused_id
                    .as_deref()
                    .and_then(|id| docked_streams.iter().find(|s| s.identity == id).copied())
                    .unwrap_or(docked_streams[0]);

                let others: Vec<&StreamView> = docked_streams
                    .iter()
                    .copied()
                    .filter(|s| s.identity != focused_stream.identity)
                    .collect();

                let avail = ui.available_size();
                let thumb_strip_h = if others.is_empty() { 0.0 } else { 156.0 };
                let hero_h =
                    (avail.y - thumb_strip_h - if others.is_empty() { 0.0 } else { 10.0 })
                        .max(220.0);

                ui.allocate_ui(vec2(avail.x, hero_h), |ui| {
                    render_stream_tile(app, ctx, ui, &snap, focused_stream, true, false, false);
                });

                if !others.is_empty() {
                    ui.add_space(10.0);
                    egui::ScrollArea::horizontal()
                        .max_height(thumb_strip_h)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                for stream in others {
                                    ui.allocate_ui(vec2(258.0, thumb_strip_h - 6.0), |ui| {
                                        render_stream_tile(
                                            app, ctx, ui, &snap, stream, false, false, false,
                                        );
                                    });
                                }
                            });
                        });
                }
            }
        });

    // =========================================================================
    // Janelas Separadas (Pop-out nativos com ícone PIT e Tela Cheia!)
    // =========================================================================
    render_popout_windows(app, ctx, &snap);

    // =========================================================================
    // Janela flutuante de Apps Silenciados (ao vivo)
    // =========================================================================
    if app.show_audio_popover {
        let mut open = app.show_audio_popover;
        let mut blocked = snap.settings.blocked_apps.clone();
        let mut changed = false;
        let apps_snapshot = app.audio_apps.lock().map(|g| g.clone()).unwrap_or_default();

        egui::Window::new("Apps silenciados (ao vivo)")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .min_width(370.0)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(BG_SURFACE)
                    .stroke(Stroke::new(1.0_f32, BORDER_STRONG))
                    .corner_radius(CornerRadius::same(16))
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Marque os apps cujo som NÃO deve sair na live:")
                            .color(TEXT_SECONDARY)
                            .size(12.5),
                    );
                    if ui.small_button("Atualizar").clicked() {
                        app.refresh_audio_apps_async(ctx);
                    }
                });
                ui.add_space(8.0);

                if apps_snapshot.is_empty() {
                    ui.label(
                        RichText::new("Nenhum app com áudio ativo encontrado no momento.")
                            .color(TEXT_MUTED),
                    );
                } else {
                    egui::Frame::NONE
                        .fill(BG_CARD)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                                for entry in &apps_snapshot {
                                    let mut is_blocked =
                                        blocked.iter().any(|b| b == &entry.exe);
                                    let label = if entry.active {
                                        format!("{} ({}) — tocando", entry.name, entry.exe)
                                    } else {
                                        format!("{} ({})", entry.name, entry.exe)
                                    };
                                    if ui.checkbox(&mut is_blocked, label).changed() {
                                        if is_blocked {
                                            if !blocked.iter().any(|b| b == &entry.exe) {
                                                blocked.push(entry.exe.clone());
                                            }
                                        } else {
                                            blocked.retain(|b| b != &entry.exe);
                                        }
                                        changed = true;
                                    }
                                }
                            });
                        });
                }
            });

        if changed {
            let session = app.session.clone();
            app.rt.spawn(async move {
                session.set_blocked_apps(blocked).await;
            });
        }
        app.show_audio_popover = open;
    }

    if app.show_share_dialog {
        share_dialog::render_share_dialog(app, ctx);
    }
}

fn render_popout_windows(app: &mut CompartilharApp, ctx: &Context, snap: &SessionSnapshot) {
    let popped_ids: Vec<String> = app.popped_out.iter().cloned().collect();
    let mut to_dock_back = Vec::new();

    for id in popped_ids {
        let Some(stream) = snap.streams.iter().find(|s| s.identity == id) else {
            continue;
        };
        let title = if stream.is_local {
            format!("{APP_NAME} — Sua transmissão")
        } else {
            format!("{APP_NAME} — {}", stream.name)
        };
        let vp_id = ViewportId::from_hash_of(format!("popout-{id}"));
        let mut builder = ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([960.0, 580.0])
            .with_min_inner_size([480.0, 300.0]);
        if let Some(ref icon) = app.app_icon {
            builder = builder.with_icon(icon.clone());
        }

        let is_fs = app.popout_fullscreen.contains(&id);
        let mut toggle_fs = false;

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx.input(|i| i.viewport().close_requested()) {
                to_dock_back.push(id.clone());
            }
            if is_fs && vp_ctx.input(|i| i.key_pressed(Key::Escape)) {
                toggle_fs = true;
            }
            egui::CentralPanel::default()
                .frame(if is_fs {
                    egui::Frame::NONE.fill(Color32::BLACK)
                } else {
                    egui::Frame::NONE.fill(BG_VOID).inner_margin(8.0)
                })
                .show(vp_ctx, |ui| {
                    let action =
                        render_stream_tile(app, vp_ctx, ui, snap, stream, true, true, is_fs);
                    if action.dock_back {
                        to_dock_back.push(id.clone());
                    }
                    if action.toggle_fullscreen {
                        toggle_fs = true;
                    }
                });

            if toggle_fs {
                vp_ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!is_fs));
            }
        });

        if toggle_fs {
            if is_fs {
                app.popout_fullscreen.remove(&id);
            } else {
                app.popout_fullscreen.insert(id.clone());
            }
        }
    }

    for id in to_dock_back {
        app.popped_out.remove(&id);
        app.popout_fullscreen.remove(&id);
    }
}

struct TileAction {
    dock_back: bool,
    toggle_fullscreen: bool,
}

#[allow(clippy::too_many_arguments)]
fn render_stream_tile(
    app: &mut CompartilharApp,
    ctx: &Context,
    ui: &mut egui::Ui,
    snap: &SessionSnapshot,
    stream: &StreamView,
    large: bool,
    in_popout: bool,
    popout_is_fs: bool,
) -> TileAction {
    let mut action = TileAction {
        dock_back: false,
        toggle_fullscreen: false,
    };
    let is_focused = app.focused_id.as_deref() == Some(stream.identity.as_str());
    let border_color = if !large && is_focused {
        WHITE_PURE
    } else if large {
        BORDER_SUBTLE
    } else {
        BORDER_STRONG
    };

    let frame = if popout_is_fs {
        egui::Frame::NONE.fill(Color32::BLACK)
    } else {
        egui::Frame::NONE
            .fill(Color32::from_rgb(12, 13, 16))
            .stroke(Stroke::new(1.0_f32, border_color))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(egui::Margin::same(6))
    };

    frame.show(ui, |ui| {
        let tile_rect = ui.available_rect_before_wrap();

        // 1. Vídeo ocupando todo o espaço do tile (limpo, igual à print de referência!)
        let avail = ui.available_size();
        let tex_info = app
            .textures
            .get(&stream.identity)
            .map(|(_, tex)| (tex.id(), tex.aspect_ratio().max(0.1)));

        if stream.receive == ReceiveQuality::Off {
            ui.allocate_ui(avail, |ui| {
                ui.centered_and_justified(|ui| {
                    if ui
                        .add(
                            primary_white_button("Transmissão pausada — Clique para voltar")
                                .min_size(vec2(260.0, 38.0)),
                        )
                        .clicked()
                    {
                        let session = app.session.clone();
                        let id = stream.identity.clone();
                        app.rt.spawn(async move {
                            session
                                .set_receive_quality(id, ReceiveQuality::Auto)
                                .await;
                        });
                    }
                });
            });
        } else if let Some((tex_id, aspect)) = tex_info {
            let w = avail.x.min(avail.y * aspect).max(120.0);
            let h = (w / aspect).max(68.0);
            ui.centered_and_justified(|ui| {
                let img_resp = ui.add(
                    egui::Image::new((tex_id, vec2(w, h)))
                        .corner_radius(CornerRadius::same(if popout_is_fs { 0 } else { 10 }))
                        .sense(egui::Sense::click()),
                );
                if img_resp.double_clicked() && large {
                    if in_popout {
                        action.toggle_fullscreen = true;
                    } else {
                        app.enter_main_fullscreen(ctx, stream.identity.clone());
                    }
                } else if !large && img_resp.clicked() {
                    app.focused_id = Some(stream.identity.clone());
                    app.grid_mode = false;
                }
            });
        } else {
            ui.allocate_ui(avail, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Conectando ao vídeo…").color(TEXT_MUTED));
                });
            });
        }

        // 2. Pílula flutuante superior esquerda (Logo PIT + ponto ao vivo + nome)
        let top_left_rect = egui::Rect::from_min_size(
            pos2(tile_rect.left() + 10.0, tile_rect.top() + 10.0),
            vec2((tile_rect.width() * 0.45).max(110.0), 32.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(top_left_rect), |ui| {
            egui::Frame::NONE
                .fill(BG_OVERLAY)
                .stroke(Stroke::new(1.0_f32, BORDER_STRONG))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        draw_logo_badge(ui, app.logo_texture.as_ref(), 16.0, false);
                        draw_status_dot(
                            ui,
                            if stream.is_local {
                                ACCENT_LIVE
                            } else {
                                ACCENT_RED_BRIGHT
                            },
                            3.5,
                        );
                        let title = if stream.is_local {
                            "Você".to_string()
                        } else {
                            stream.name.clone()
                        };
                        ui.label(display_text(title, 12.5).color(WHITE_PURE));
                    });
                });
        });

        // 3. Barra de controles flutuante no canto superior direito do vídeo
        let controls_w = (tile_rect.width() - 24.0).max(120.0);
        let top_right_rect = egui::Rect::from_min_size(
            pos2(tile_rect.right() - controls_w - 10.0, tile_rect.top() + 10.0),
            vec2(controls_w, 34.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(top_right_rect), |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                // Botão Tela cheia
                if large {
                    let fs_icon = if popout_is_fs {
                        IconKind::ExitFullscreen
                    } else {
                        IconKind::Fullscreen
                    };
                    let fs_label = if popout_is_fs {
                        Some("Sair (Esc)")
                    } else {
                        None
                    };
                    if icon_pill_button(
                        ui,
                        fs_icon,
                        fs_label,
                        if popout_is_fs {
                            PillVariant::ActiveWhite
                        } else {
                            PillVariant::Overlay
                        },
                        28.0,
                    )
                    .on_hover_text(if popout_is_fs {
                        "Sair da tela cheia (Esc)"
                    } else {
                        "Tela cheia (ou dê duplo clique no vídeo)"
                    })
                    .clicked()
                    {
                        if in_popout {
                            action.toggle_fullscreen = true;
                        } else {
                            app.enter_main_fullscreen(ctx, stream.identity.clone());
                        }
                    }
                }

                // Botão Janela separada / Voltar à sala
                if in_popout {
                    if !popout_is_fs
                        && icon_pill_button(
                            ui,
                            IconKind::DockBack,
                            Some("Voltar"),
                            PillVariant::Overlay,
                            28.0,
                        )
                        .on_hover_text("Trazer transmissão de volta para a sala")
                        .clicked()
                    {
                        action.dock_back = true;
                    }
                } else if icon_pill_button(ui, IconKind::PopOut, None, PillVariant::Overlay, 28.0)
                    .on_hover_text("Abrir em janela separada")
                    .clicked()
                {
                    app.popped_out.insert(stream.identity.clone());
                }

                // Botão Destacar em miniaturas
                if !large
                    && icon_pill_button(
                        ui,
                        IconKind::Spotlight,
                        Some("Destacar"),
                        PillVariant::ActiveWhite,
                        26.0,
                    )
                    .clicked()
                {
                    app.focused_id = Some(stream.identity.clone());
                    app.grid_mode = false;
                }

                // Controles de recepção & volume para transmissões remotas (dentro de uma pílula flutuante)
                if large && !stream.is_local && tile_rect.width() >= 440.0 {
                    egui::Frame::NONE
                        .fill(BG_OVERLAY)
                        .stroke(Stroke::new(1.0_f32, BORDER_STRONG))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(8, 3))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                let mut vol = snap.volume_of(&stream.name);
                                let (icon_r, _) = ui
                                    .allocate_exact_size(vec2(13.0, 13.0), egui::Sense::hover());
                                paint_icon(
                                    ui.painter(),
                                    icon_r,
                                    if vol <= 0.01 {
                                        IconKind::VolumeMute
                                    } else {
                                        IconKind::Volume
                                    },
                                    WHITE_PURE,
                                );
                                if ui
                                    .add(
                                        egui::Slider::new(&mut vol, 0.0..=2.0)
                                            .show_value(false)
                                            .fixed_decimals(2),
                                    )
                                    .changed()
                                {
                                    let session = app.session.clone();
                                    let id = stream.identity.clone();
                                    let name = stream.name.clone();
                                    app.rt.spawn(async move {
                                        session.set_peer_volume(id, name, vol).await;
                                    });
                                }
                                ui.label(
                                    mono_text(format!("{}%", (vol * 100.0).round()), 11.0)
                                        .color(TEXT_SECONDARY),
                                );

                                egui::ComboBox::from_id_salt(format!(
                                    "recv-{}-{in_popout}",
                                    stream.identity
                                ))
                                .width(70.0)
                                .selected_text(match stream.receive {
                                    ReceiveQuality::Auto => "Auto",
                                    ReceiveQuality::Low => "Baixa",
                                    ReceiveQuality::Off => "Pausar",
                                })
                                .show_ui(ui, |ui| {
                                    for (mode, txt) in [
                                        (ReceiveQuality::Auto, "Auto"),
                                        (ReceiveQuality::Low, "Baixa"),
                                        (ReceiveQuality::Off, "Pausar"),
                                    ] {
                                        if ui
                                            .selectable_label(stream.receive == mode, txt)
                                            .clicked()
                                        {
                                            let session = app.session.clone();
                                            let id = stream.identity.clone();
                                            app.rt.spawn(async move {
                                                session.set_receive_quality(id, mode).await;
                                            });
                                        }
                                    }
                                });
                            });
                        });
                }
            });
        });
    });

    action
}

fn draw_peer_avatar(ui: &mut egui::Ui, name: &str, sharing: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(32.0, 32.0), egui::Sense::hover());
    let p = ui.painter();
    let initial = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());
    p.circle_filled(
        rect.center(),
        15.0,
        if sharing { ACCENT_RED_DIM } else { BG_CONTROL },
    );
    p.circle_stroke(
        rect.center(),
        15.0,
        Stroke::new(
            1.4_f32,
            if sharing {
                ACCENT_RED_BRIGHT
            } else {
                BORDER_STRONG
            },
        ),
    );
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        initial,
        egui::FontId::new(13.5, super::theme::display_family()),
        WHITE_PURE,
    );
}

fn role_pill(ui: &mut egui::Ui, label: &str, color: Color32) {
    egui::Frame::NONE
        .fill(color.linear_multiply(0.16))
        .stroke(Stroke::new(1.0_f32, color.linear_multiply(0.55)))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(mono_text(label, 9.5).color(color));
        });
}
