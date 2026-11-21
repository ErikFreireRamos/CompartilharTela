use std::time::{Duration, Instant};

use ct_core::protocol::{FPS_OPTIONS, RESOLUTIONS, RoomMode, RoomScope};
use ct_media::audio::AudioMode;
use ct_media::session::{ReceiveQuality, SessionSnapshot, StreamView};
use egui::{Color32, Context, RichText, Stroke, ViewportBuilder, ViewportId};

use super::{
    ACCENT_GREEN, ACCENT_PRIMARY, ACCENT_WARN, BG_CARD, BG_CONTROL, BG_PANEL, BORDER_SUBTLE,
    CompartilharApp, TEXT_MUTED, danger_button, primary_button, render_admin_warning, share_dialog,
};

pub fn render_room(app: &mut CompartilharApp, ctx: &Context) {
    let snap = app.snap.clone();
    let copied = app
        .copied_until
        .is_some_and(|until| Instant::now() < until);

    // =========================================================================
    // Barra Superior (sem o antigo botão "Teto de qualidade")
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
                // Badge do modo da sala
                let (mode_label, mode_color) = match snap.mode() {
                    Some(RoomMode::Iroh) => ("P2P Iroh", ACCENT_PRIMARY),
                    Some(RoomMode::Classic) => ("Clássico", Color32::from_rgb(130, 145, 175)),
                    None => ("Sala", TEXT_MUTED),
                };
                egui::Frame::NONE
                    .fill(mode_color)
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| {
                        ui.label(RichText::new(mode_label).size(12.0).strong().color(Color32::WHITE));
                    });

                // Código copiável
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

                    // Alternador de layout: Destaque vs Grade
                    if !snap.streams.is_empty() {
                        ui.add_space(6.0);
                        let grid_btn = egui::Button::new(
                            RichText::new("Grade")
                                .color(if app.grid_mode { Color32::WHITE } else { TEXT_MUTED }),
                        )
                        .fill(if app.grid_mode { ACCENT_PRIMARY } else { BG_CONTROL });
                        if ui.add(grid_btn).clicked() {
                            app.grid_mode = true;
                        }

                        let focus_btn = egui::Button::new(
                            RichText::new("Destaque")
                                .color(if !app.grid_mode { Color32::WHITE } else { TEXT_MUTED }),
                        )
                        .fill(if !app.grid_mode { ACCENT_PRIMARY } else { BG_CONTROL });
                        if ui.add(focus_btn).clicked() {
                            app.grid_mode = false;
                        }
                    }
                });
            });
        });

    // =========================================================================
    // Barra Inferior: controles ao vivo de transmissão (Resolução, FPS, Volume, Apps)
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

                    // Controle individual ao vivo de RESOLUÇÃO
                    let cur_res = share.resolution;
                    let cur_fps = share.fps;
                    egui::ComboBox::from_id_salt("live_res")
                        .width(82.0)
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

                    // Controle individual ao vivo de FPS
                    egui::ComboBox::from_id_salt("live_fps")
                        .width(78.0)
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

                    // Controles de áudio ao vivo
                    if share.audio_mode != AudioMode::None {
                        ui.separator();
                        let mut send_vol = snap.settings.send_volume;
                        ui.label(
                            RichText::new(format!("Áudio {}%", (send_vol * 100.0).round()))
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
    // Barra Lateral de Participantes
    // =========================================================================
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
            ui.label(
                RichText::new(format!("Participantes ({})", snap.peers.len()))
                    .size(15.0)
                    .strong(),
            );
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
                                ui.label(RichText::new(format!("{}{suffix}", p.name)).strong());
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

        if let Some(warn) = ports_warning(&snap) {
            egui::Frame::NONE
                .fill(Color32::from_rgb(96, 68, 24))
                .corner_radius(8.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.colored_label(Color32::from_rgb(255, 232, 170), warn);
                });
            ui.add_space(6.0);
        }
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

        // Streams exibidos na janela principal (os destacados em janela separada mostram aviso)
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
                        .add(primary_button("Compartilhar minha tela").min_size(egui::vec2(210.0, 40.0)))
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
            // MODO GRADE: colunas proporcionais lado a lado
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
                                render_stream_tile(app, ui, &snap, stream, true, false);
                            });
                        }
                    });
                    ui.add_space(spacing);
                }
            });
        } else {
            // MODO DESTAQUE: 1 transmissão grande + barra inferior de miniaturas clicáveis
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
            let hero_h = (avail.y - thumb_strip_h - if others.is_empty() { 0.0 } else { 10.0 }).max(220.0);

            ui.allocate_ui(egui::vec2(avail.x, hero_h), |ui| {
                render_stream_tile(app, ui, &snap, focused_stream, true, false);
            });

            if !others.is_empty() {
                ui.add_space(8.0);
                egui::ScrollArea::horizontal()
                    .max_height(thumb_strip_h)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for stream in others {
                                ui.allocate_ui(egui::vec2(250.0, thumb_strip_h - 6.0), |ui| {
                                    render_stream_tile(app, ui, &snap, stream, false, false);
                                });
                            }
                        });
                    });
            }
        }
    });

    // =========================================================================
    // Janelas Separadas (Pop-out Viewports nativos do SO)
    // =========================================================================
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

        ctx.show_viewport_immediate(vp_id, builder, |ctx, _class| {
            if ctx.input(|i| i.viewport().close_requested()) {
                to_dock_back.push(id.clone());
            }
            egui::CentralPanel::default().show(ctx, |ui| {
                if render_stream_tile(app, ui, &snap, stream, true, true) {
                    to_dock_back.push(id.clone());
                }
            });
        });
    }
    for id in to_dock_back {
        app.popped_out.remove(&id);
    }

    // =========================================================================
    // Janela de Apps Bloqueados (funciona ao vivo enquanto transmite!)
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

/// Renderiza um card de vídeo. Retorna `true` se o usuário clicou em "Voltar à sala" num pop-out.
fn render_stream_tile(
    app: &mut CompartilharApp,
    ui: &mut egui::Ui,
    snap: &SessionSnapshot,
    stream: &StreamView,
    large: bool,
    in_popout: bool,
) -> bool {
    let mut dock_back_clicked = false;
    let is_focused = app.focused_id.as_deref() == Some(stream.identity.as_str());
    let border_color = if !large && is_focused {
        ACCENT_PRIMARY
    } else {
        BORDER_SUBTLE
    };

    egui::Frame::NONE
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0_f32, border_color))
        .corner_radius(10.0)
        .inner_margin(8.0)
        .show(ui, |ui| {
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
                    if in_popout {
                        if ui.small_button("Voltar à sala").clicked() {
                            dock_back_clicked = true;
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
                        // Qualidade de recebimento (Auto / Baixa / Desligar)
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

                        // Volume individual do participante (0% - 200%)
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

            // Área do vídeo
            let avail = ui.available_size();
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
            } else if let Some((_, tex)) = app.textures.get(&stream.identity) {
                let aspect = tex.aspect_ratio().max(0.1);
                let w = avail.x.min(avail.y * aspect).max(120.0);
                let h = (w / aspect).max(68.0);
                ui.centered_and_justified(|ui| {
                    let img_resp = ui.add(
                        egui::Image::new((tex.id(), egui::vec2(w, h)))
                            .sense(egui::Sense::click()),
                    );
                    if !large && img_resp.clicked() {
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

    dock_back_clicked
}

fn draw_live_dot(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 4.5, ACCENT_GREEN);
}

fn ports_warning(snap: &SessionSnapshot) -> Option<&'static str> {
    if !snap.is_host() || snap.mode() == Some(RoomMode::Iroh) {
        return None;
    }
    let info = snap.info.as_ref()?;
    let via_ipv6 = info.endpoint.contains(':');
    if info.cgnat == Some(true) && via_ipv6 {
        Some(
            "Sua internet tem CGNAT, então esta sala Clássica foi criada pelo IPv6. Para evitar problemas de rede, prefira criar a sala em P2P (Iroh).",
        )
    } else if info.cgnat == Some(true) {
        Some(
            "Sua internet usa CGNAT: no modo Clássico amigos de fora não conseguem entrar pelo IPv4. Use o modo P2P (Iroh).",
        )
    } else if snap.meta.as_ref().map(|m| m.scope) == Some(RoomScope::Internet)
        && !via_ipv6
        && info.ports_open == Some(false)
    {
        Some(
            "UPnP não abriu as portas automaticamente. Abra TCP 7880–7881 e UDP 7882 no roteador ou use o modo P2P (Iroh).",
        )
    } else if snap.meta.as_ref().map(|m| m.scope) == Some(RoomScope::Lan) {
        Some("Sem IP público: esta sala Clássica só funciona na mesma rede local.")
    } else {
        None
    }
}
