use std::time::{Duration, Instant};

use ct_core::protocol::{AUDIO_QUALITIES, FPS_OPTIONS, RESOLUTIONS};
use ct_media::audio::AudioMode;
use ct_media::session::{ReceiveQuality, SessionSnapshot, StreamView};
use egui::{Color32, Context, Key, RichText, Stroke, ViewportBuilder, ViewportCommand, ViewportId};

use super::{
    ACCENT_GREEN, ACCENT_PRIMARY, ACCENT_WARN, BG_CARD, BG_CONTROL, BG_PANEL, BORDER_SUBTLE,
    CompartilharApp, TEXT_MUTED, danger_button, primary_button, render_admin_warning, share_dialog,
};

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
    // Modo Tela Cheia na janela principal (oculta barras/menus e exibe só o vídeo)
    // =========================================================================
    if let Some(fs_id) = app.fullscreen_stream_id.clone()
        && let Some(stream) = snap.streams.iter().find(|s| s.identity == fs_id)
    {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::BLACK))
            .show(ctx, |ui| {
                // Barra superior discreta no modo tela cheia
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    draw_live_dot(ui);
                    let title = if stream.is_local {
                        "Você".to_string()
                    } else {
                        stream.name.clone()
                    };
                    ui.label(RichText::new(title).strong().color(Color32::WHITE));

                    if !stream.is_local {
                        ui.add_space(12.0);
                        let mut vol = snap.volume_of(&stream.name);
                        ui.label(
                            RichText::new(format!("Vol {}%", (vol * 100.0).round()))
                                .color(TEXT_MUTED),
                        );
                        if ui
                            .add(egui::Slider::new(&mut vol, 0.0..=2.0).show_value(false))
                            .changed()
                        {
                            let session = app.session.clone();
                            let id = stream.identity.clone();
                            let name = stream.name.clone();
                            app.rt.spawn(async move {
                                session.set_peer_volume(id, name, vol).await;
                            });
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(10.0);
                        if ui.button("Sair da tela cheia (Esc)").clicked() {
                            app.exit_main_fullscreen(ctx);
                        }
                    });
                });

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
                            egui::Image::new((tex_id, egui::vec2(w, h)))
                                .sense(egui::Sense::click()),
                        );
                        if resp.double_clicked() {
                            app.exit_main_fullscreen(ctx);
                        }
                    });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("Aguardando vídeo…").color(TEXT_MUTED));
                    });
                }
            });
        render_popout_windows(app, ctx, &snap);
        return;
    }

    // =========================================================================
    // Barra Superior
    // =========================================================================
    egui::TopBottomPanel::top("room_header")
        .frame(
            egui::Frame::NONE
                .fill(BG_PANEL)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .inner_margin(egui::Margin::symmetric(14, 10)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                egui::Frame::NONE
                    .fill(ACCENT_PRIMARY)
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("P2P Iroh")
                                .size(12.0)
                                .strong()
                                .color(Color32::WHITE),
                        );
                    });

                let full_code = snap.code();
                let display_code = if full_code.len() > 22 {
                    format!("{}…", &full_code[..18])
                } else {
                    full_code.to_string()
                };
                let code_btn_txt = if copied {
                    format!("Código {display_code}  —  Copiado!")
                } else {
                    format!("Código: {display_code}  [Copiar]")
                };
                if ui.button(RichText::new(code_btn_txt).strong()).clicked() {
                    ctx.copy_text(full_code.to_string());
                    app.copied_until = Some(Instant::now() + Duration::from_millis(1800));
                }

                let host_name = snap
                    .meta
                    .as_ref()
                    .map(|m| m.host_name.as_str())
                    .unwrap_or("…");
                ui.label(RichText::new(format!("Host: {host_name}")).color(TEXT_MUTED));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(danger_button("Sair")).clicked() {
                        let session = app.session.clone();
                        app.rt.spawn(async move {
                            session.leave_room().await;
                        });
                    }

                    // Botão para mostrar/minimizar a aba lateral de participantes
                    let part_btn = egui::Button::new(
                        RichText::new(format!("Participantes ({})", snap.peers.len())).color(
                            if app.show_participants {
                                Color32::WHITE
                            } else {
                                TEXT_MUTED
                            },
                        ),
                    )
                    .fill(if app.show_participants {
                        BG_CARD
                    } else {
                        BG_CONTROL
                    });
                    if ui
                        .add(part_btn)
                        .on_hover_text("Mostrar ou minimizar a lista de participantes")
                        .clicked()
                    {
                        app.show_participants = !app.show_participants;
                    }

                    // Alternador de layout: Destaque vs Grade
                    if !snap.streams.is_empty() {
                        let grid_btn = egui::Button::new(
                            RichText::new("Grade")
                                .color(if app.grid_mode { Color32::WHITE } else { TEXT_MUTED }),
                        )
                        .fill(if app.grid_mode {
                            ACCENT_PRIMARY
                        } else {
                            BG_CONTROL
                        });
                        if ui.add(grid_btn).clicked() {
                            app.grid_mode = true;
                        }

                        let focus_btn = egui::Button::new(
                            RichText::new("Destaque").color(if !app.grid_mode {
                                Color32::WHITE
                            } else {
                                TEXT_MUTED
                            }),
                        )
                        .fill(if !app.grid_mode {
                            ACCENT_PRIMARY
                        } else {
                            BG_CONTROL
                        });
                        if ui.add(focus_btn).clicked() {
                            app.grid_mode = false;
                        }
                    }
                });
            });
        });

    // =========================================================================
    // Barra Inferior: controles ao vivo (Resolução, FPS, Qualidade de Áudio, Volume, Apps)
    // =========================================================================
    egui::TopBottomPanel::bottom("room_footer")
        .frame(
            egui::Frame::NONE
                .fill(BG_PANEL)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .inner_margin(egui::Margin::symmetric(14, 10)),
        )
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if let Some(ref share) = snap.share {
                    draw_live_dot(ui);
                    ui.label(RichText::new("Ao vivo").strong().color(ACCENT_GREEN));
                    ui.separator();

                    // Resolução ao vivo
                    let cur_res = share.resolution;
                    let cur_fps = share.fps;
                    egui::ComboBox::from_id_salt("live_res")
                        .width(78.0)
                        .selected_text(cur_res.label())
                        .show_ui(ui, |ui| {
                            for &res in RESOLUTIONS {
                                if ui.selectable_label(cur_res == res, res.label()).clicked() {
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
                        ui.separator();
                        let cur_aq = share.audio_quality;
                        egui::ComboBox::from_id_salt("live_audio_q")
                            .width(126.0)
                            .selected_text(cur_aq.short_label())
                            .show_ui(ui, |ui| {
                                for &aq in AUDIO_QUALITIES {
                                    if ui.selectable_label(cur_aq == aq, aq.label()).clicked() {
                                        app.selected_audio_quality = aq;
                                        let session = app.session.clone();
                                        app.rt.spawn(async move {
                                            session.set_audio_quality(aq).await;
                                        });
                                    }
                                }
                            });

                        let mut send_vol = snap.settings.send_volume;
                        ui.label(
                            RichText::new(format!("{}%", (send_vol * 100.0).round()))
                                .color(TEXT_MUTED),
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
                            app.rt
                                .spawn(async move { session.set_send_volume(send_vol).await });
                        }
                    }

                    if share.audio_mode == AudioMode::Native
                        && ui.button("Apps bloqueados").clicked()
                    {
                        app.refresh_audio_apps_async(ctx);
                        app.show_audio_popover = !app.show_audio_popover;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(danger_button("Parar transmissão")).clicked() {
                            let session = app.session.clone();
                            app.rt.spawn(async move { session.stop_share().await });
                        }
                    });
                } else {
                    ui.label(
                        RichText::new("Pronto para transmitir sua tela com áudio")
                            .color(TEXT_MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(primary_button("Compartilhar tela")).clicked() {
                            app.open_share_dialog(ctx);
                        }
                    });
                }
            });
        });

    // =========================================================================
    // Barra Lateral de Participantes (minimizável)
    // =========================================================================
    if app.show_participants {
        egui::SidePanel::right("participants")
            .min_width(220.0)
            .default_width(240.0)
            .frame(
                egui::Frame::NONE
                    .fill(BG_PANEL)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .inner_margin(12.0),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Participantes ({})", snap.peers.len()))
                            .size(15.0)
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(">")
                            .on_hover_text("Minimizar aba de participantes")
                            .clicked()
                        {
                            app.show_participants = false;
                        }
                    });
                });
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for p in &snap.peers {
                        egui::Frame::NONE
                            .fill(BG_CARD)
                            .corner_radius(8.0)
                            .inner_margin(egui::Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if p.sharing {
                                        draw_live_dot(ui);
                                    }
                                    let suffix = if p.is_local { " (você)" } else { "" };
                                    ui.label(
                                        RichText::new(format!("{}{suffix}", p.name)).strong(),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if p.is_host {
                                                ui.colored_label(ACCENT_WARN, "Host");
                                            } else if p.can_host == Some(true) {
                                                ui.colored_label(ACCENT_PRIMARY, "Reserva");
                                            }
                                        },
                                    );
                                });
                                if p.sharing {
                                    ui.add_space(2.0);
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("Transmitindo")
                                                .size(11.0)
                                                .color(ACCENT_GREEN),
                                        );
                                        if ui.small_button("Ver").clicked() {
                                            app.focused_id = Some(p.identity.clone());
                                            app.grid_mode = false;
                                        }
                                    });
                                }
                            });
                        ui.add_space(6.0);
                    }
                });
            });
    }

    // =========================================================================
    // Palco Principal (Destaque com miniaturas clicáveis OU Grade multi-coluna)
    // =========================================================================
    egui::CentralPanel::default().show(ctx, |ui| {
        if let Some(ref notice) = snap.notice {
            egui::Frame::NONE
                .fill(Color32::from_rgb(38, 66, 108))
                .corner_radius(8.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.label(notice);
                });
            ui.add_space(6.0);
        }
        render_admin_warning(ui, snap.elevated);

        if let Some(ref warn) = snap.share.as_ref().and_then(|s| s.warning.as_ref()) {
            egui::Frame::NONE
                .fill(Color32::from_rgb(96, 68, 24))
                .corner_radius(8.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.colored_label(Color32::from_rgb(255, 232, 170), *warn);
                });
            ui.add_space(6.0);
        }

        let docked_streams: Vec<&StreamView> = snap
            .streams
            .iter()
            .filter(|s| !app.popped_out.contains(&s.identity))
            .collect();

        if snap.streams.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("Ninguém está transmitindo ainda")
                            .size(18.0)
                            .color(TEXT_MUTED),
                    );
                    ui.add_space(12.0);
                    if ui
                        .add(
                            primary_button("Compartilhar minha tela")
                                .min_size(egui::vec2(210.0, 40.0)),
                        )
                        .clicked()
                    {
                        app.open_share_dialog(ctx);
                    }
                });
            });
        } else if docked_streams.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("Todas as transmissões estão em janelas separadas.")
                            .size(16.0)
                            .color(TEXT_MUTED),
                    );
                    ui.add_space(10.0);
                    if ui.button("Trazer todas de volta").clicked() {
                        app.popped_out.clear();
                    }
                });
            });
        } else if app.grid_mode {
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
            let spacing = 10.0;
            let cell_w = ((avail.x - spacing * (cols as f32 - 1.0)) / cols as f32).max(260.0);
            let cell_h = ((avail.y - spacing * (rows as f32 - 1.0)) / rows as f32).max(200.0);

            egui::ScrollArea::both().show(ui, |ui| {
                for row_chunk in docked_streams.chunks(cols) {
                    ui.horizontal(|ui| {
                        for stream in row_chunk {
                            ui.allocate_ui(egui::vec2(cell_w, cell_h), |ui| {
                                render_stream_tile(app, ctx, ui, &snap, stream, true, false, false);
                            });
                        }
                    });
                    ui.add_space(spacing);
                }
            });
        } else {
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
            let thumb_strip_h = if others.is_empty() { 0.0 } else { 155.0 };
            let hero_h =
                (avail.y - thumb_strip_h - if others.is_empty() { 0.0 } else { 10.0 }).max(220.0);

            ui.allocate_ui(egui::vec2(avail.x, hero_h), |ui| {
                render_stream_tile(app, ctx, ui, &snap, focused_stream, true, false, false);
            });

            if !others.is_empty() {
                ui.add_space(8.0);
                egui::ScrollArea::horizontal()
                    .max_height(thumb_strip_h)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for stream in others {
                                ui.allocate_ui(egui::vec2(250.0, thumb_strip_h - 6.0), |ui| {
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
    // Janelas Separadas (Pop-out Viewports nativos com suporte a Tela Cheia!)
    // =========================================================================
    render_popout_windows(app, ctx, &snap);

    // =========================================================================
    // Janela de Apps Bloqueados (ao vivo)
    // =========================================================================
    if app.show_audio_popover {
        let mut open = app.show_audio_popover;
        let mut blocked = snap.settings.blocked_apps.clone();
        let mut changed = false;
        let apps_snapshot = app.audio_apps.lock().map(|g| g.clone()).unwrap_or_default();

        egui::Window::new("Apps bloqueados (ao vivo)")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .min_width(360.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Marque os apps cujo áudio NÃO deve ser transmitido:")
                            .color(TEXT_MUTED)
                            .size(12.5),
                    );
                    if ui.small_button("Atualizar").clicked() {
                        app.refresh_audio_apps_async(ctx);
                    }
                });
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                if apps_snapshot.is_empty() {
                    ui.label(
                        RichText::new("Nenhum app com áudio ativo encontrado no momento.")
                            .color(TEXT_MUTED),
                    );
                } else {
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for entry in &apps_snapshot {
                            let mut is_blocked = blocked.iter().any(|b| b == &entry.exe);
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
            "Sua transmissão — CompartilhadorDeTela".to_string()
        } else {
            format!("Transmissão de {} — CompartilhadorDeTela", stream.name)
        };
        let vp_id = ViewportId::from_hash_of(format!("popout-{id}"));
        let builder = ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([960.0, 580.0])
            .with_min_inner_size([480.0, 300.0]);

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
                    egui::Frame::central_panel(&vp_ctx.style())
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
        ACCENT_PRIMARY
    } else {
        BORDER_SUBTLE
    };

    let frame = if popout_is_fs {
        egui::Frame::NONE.fill(Color32::BLACK).inner_margin(4.0)
    } else {
        egui::Frame::NONE
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0_f32, border_color))
            .corner_radius(10.0)
            .inner_margin(8.0)
    };

    frame.show(ui, |ui| {
        // Cabeçalho do card
        ui.horizontal(|ui| {
            draw_live_dot(ui);
            let title = if stream.is_local {
                "Você".to_string()
            } else {
                stream.name.clone()
            };
            ui.label(RichText::new(title).strong());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Botão Tela Cheia (tanto na janela principal quanto na janela separada!)
                if large {
                    let fs_txt = if popout_is_fs {
                        "Sair da tela cheia (Esc)"
                    } else {
                        "Tela cheia"
                    };
                    if ui.small_button(fs_txt).clicked() {
                        if in_popout {
                            action.toggle_fullscreen = true;
                        } else {
                            app.enter_main_fullscreen(ctx, stream.identity.clone());
                        }
                    }
                }

                if in_popout {
                    if !popout_is_fs && ui.small_button("Voltar à sala").clicked() {
                        action.dock_back = true;
                    }
                } else if ui
                    .small_button("Janela separada")
                    .on_hover_text("Abrir esta transmissão em uma janela independente")
                    .clicked()
                {
                    app.popped_out.insert(stream.identity.clone());
                }

                if !large && ui.small_button("Destacar").clicked() {
                    app.focused_id = Some(stream.identity.clone());
                    app.grid_mode = false;
                }

                if large && !stream.is_local {
                    egui::ComboBox::from_id_salt(format!("recv-{}-{in_popout}", stream.identity))
                        .width(82.0)
                        .selected_text(match stream.receive {
                            ReceiveQuality::Auto => "Auto",
                            ReceiveQuality::Low => "Baixa",
                            ReceiveQuality::Off => "Desligar",
                        })
                        .show_ui(ui, |ui| {
                            for (mode, txt) in [
                                (ReceiveQuality::Auto, "Auto"),
                                (ReceiveQuality::Low, "Baixa"),
                                (ReceiveQuality::Off, "Desligar"),
                            ] {
                                if ui.selectable_label(stream.receive == mode, txt).clicked() {
                                    let session = app.session.clone();
                                    let id = stream.identity.clone();
                                    app.rt.spawn(async move {
                                        session.set_receive_quality(id, mode).await;
                                    });
                                }
                            }
                        });

                    let mut vol = snap.volume_of(&stream.name);
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
                        RichText::new(format!("Vol {}%", (vol * 100.0).round()))
                            .size(12.0)
                            .color(TEXT_MUTED),
                    );
                }
            });
        });

        ui.add_space(4.0);

        let avail = ui.available_size();
        let tex_info = app
            .textures
            .get(&stream.identity)
            .map(|(_, tex)| (tex.id(), tex.aspect_ratio().max(0.1)));
        if stream.receive == ReceiveQuality::Off {
            ui.allocate_ui(avail, |ui| {
                ui.centered_and_justified(|ui| {
                    if ui.button("Transmissão desligada — Clique para ligar").clicked() {
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
                    egui::Image::new((tex_id, egui::vec2(w, h))).sense(egui::Sense::click()),
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
                    ui.label(RichText::new("Aguardando vídeo…").color(TEXT_MUTED));
                });
            });
        }
    });

    action
}

fn draw_live_dot(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 4.5, ACCENT_GREEN);
}
