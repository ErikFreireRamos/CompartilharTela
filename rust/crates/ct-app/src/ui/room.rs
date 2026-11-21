use std::time::{Duration, Instant};

use ct_core::protocol::{QUALITY_PRESETS, RoomScope};
use ct_media::audio::AudioMode;
use ct_media::session::ReceiveQuality;
use egui::{Color32, Context, RichText};

use super::{CompartilharApp, render_admin_warning, share_dialog};

pub fn render_room(app: &mut CompartilharApp, ctx: &Context) {
    let snap = app.snap.clone();
    let copied = app
        .copied_until
        .is_some_and(|until| Instant::now() < until);

    // Top bar
    egui::TopBottomPanel::top("room_header").show(ctx, |ui| {
        ui.horizontal(|ui| {
            let code_label = if copied {
                format!("Código {} ✓ copiado", snap.code())
            } else {
                format!("Código {} ⧉", snap.code())
            };
            if ui.button(RichText::new(code_label).strong()).clicked() {
                ctx.copy_text(snap.code().to_string());
                app.copied_until = Some(Instant::now() + Duration::from_millis(1500));
            }

            let host_name = snap
                .meta
                .as_ref()
                .map(|m| m.host_name.as_str())
                .unwrap_or("…");
            ui.label(format!("Host: {host_name} 👑"));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(RichText::new("Sair").color(Color32::from_rgb(255, 120, 120)))
                    .clicked()
                {
                    let session = app.session.clone();
                    app.rt.spawn(async move {
                        session.leave_room().await;
                    });
                }

                if snap.streams.len() > 1 {
                    let label = if app.grid_mode { "Destaque" } else { "Grade" };
                    if ui.button(label).clicked() {
                        app.grid_mode = !app.grid_mode;
                    }
                }

                if snap.is_host() {
                    let current = snap.meta.as_ref().and_then(|m| m.max_quality);
                    let current_text = current
                        .map(|q| ct_core::protocol::preset_by_id(q).label)
                        .unwrap_or("Sem limite");
                    egui::ComboBox::from_id_salt("max_quality")
                        .selected_text(format!("Teto: {current_text}"))
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(current.is_none(), "Sem limite").clicked() {
                                let session = app.session.clone();
                                app.rt.spawn(async move { session.set_max_quality(None).await });
                            }
                            for p in QUALITY_PRESETS {
                                if ui
                                    .selectable_label(current == Some(p.id), p.label)
                                    .clicked()
                                {
                                    let session = app.session.clone();
                                    let id = p.id;
                                    app.rt
                                        .spawn(async move { session.set_max_quality(Some(id)).await });
                                }
                            }
                        });
                }
            });
        });
    });

    // Footer bar
    egui::TopBottomPanel::bottom("room_footer").show(ctx, |ui| {
        ui.horizontal(|ui| {
            if let Some(ref share) = snap.share {
                let mode_str = match share.audio_mode {
                    AudioMode::Native => " · áudio por app",
                    AudioMode::System => " · áudio do PC inteiro",
                    AudioMode::None => "",
                };
                ui.colored_label(
                    Color32::from_rgb(100, 230, 140),
                    format!("● Transmitindo · {}{mode_str}", share.preset.label),
                );

                if share.audio_mode != AudioMode::None {
                    let mut send_vol = snap.settings.send_volume;
                    ui.label(format!("Volume enviado {}%", (send_vol * 100.0).round()));
                    if ui
                        .add(egui::Slider::new(&mut send_vol, 0.0..=2.0).show_value(false))
                        .changed()
                    {
                        let session = app.session.clone();
                        app.rt
                            .spawn(async move { session.set_send_volume(send_vol).await });
                    }
                }

                if share.audio_mode == AudioMode::Native && ui.button("Apps bloqueados").clicked() {
                    app.show_audio_popover = !app.show_audio_popover;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(
                            RichText::new("Parar transmissão")
                                .color(Color32::from_rgb(255, 120, 120)),
                        )
                        .clicked()
                    {
                        let session = app.session.clone();
                        app.rt.spawn(async move { session.stop_share().await });
                    }
                });
            } else {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Compartilhar tela").clicked() {
                        app.open_share_dialog();
                    }
                });
            }
        });
    });

    // Participant sidebar
    egui::SidePanel::right("participants")
        .min_width(210.0)
        .show(ctx, |ui| {
            ui.heading(format!("Participantes ({})", snap.peers.len()));
            ui.separator();
            for p in &snap.peers {
                ui.horizontal(|ui| {
                    let suffix = if p.is_local { " (você)" } else { "" };
                    ui.label(format!("{}{suffix}", p.name));
                    if p.is_host {
                        ui.label("👑");
                    } else if p.can_host == Some(true) {
                        ui.colored_label(Color32::from_rgb(130, 200, 255), "reserva");
                    }
                    if p.sharing {
                        ui.colored_label(Color32::from_rgb(100, 230, 140), "●");
                    }
                });
            }
        });

    // Central video stage
    egui::CentralPanel::default().show(ctx, |ui| {
        if let Some(ref notice) = snap.notice {
            egui::Frame::NONE
                .fill(Color32::from_rgb(28, 52, 84))
                .corner_radius(6.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.label(notice);
                });
            ui.add_space(6.0);
        }
        render_admin_warning(ui, snap.elevated);

        if let Some(warn) = ports_warning(&snap) {
            egui::Frame::NONE
                .fill(Color32::from_rgb(90, 58, 18))
                .corner_radius(6.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.colored_label(Color32::from_rgb(255, 224, 150), warn);
                });
            ui.add_space(6.0);
        }

        if snap.streams.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.label("Ninguém está transmitindo ainda.");
                    ui.add_space(8.0);
                    if ui.button("Compartilhar minha tela").clicked() {
                        app.open_share_dialog();
                    }
                });
            });
        } else {
            let focused_identity = app
                .focused_id
                .clone()
                .filter(|id| snap.streams.iter().any(|s| &s.identity == id))
                .or_else(|| snap.streams.iter().find(|s| !s.is_local).map(|s| s.identity.clone()))
                .or_else(|| snap.streams.first().map(|s| s.identity.clone()));

            for stream in &snap.streams {
                let is_focused = app.grid_mode
                    || focused_identity.as_deref() == Some(stream.identity.as_str());
                egui::Frame::NONE
                    .fill(Color32::from_rgb(20, 22, 28))
                    .corner_radius(8.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let label = if stream.is_local {
                                "● Você".to_string()
                            } else {
                                format!("● {}", stream.name)
                            };
                            if ui.selectable_label(is_focused, RichText::new(label).strong()).clicked() {
                                app.focused_id = Some(stream.identity.clone());
                            }

                            if !stream.is_local {
                                let mut vol = snap.volume_of(&stream.name);
                                ui.label(format!("Vol: {}%", (vol * 100.0).round()));
                                if ui
                                    .add(egui::Slider::new(&mut vol, 0.0..=2.0).show_value(false))
                                    .changed()
                                {
                                    let session = app.session.clone();
                                    let name = stream.name.clone();
                                    app.rt.spawn(async move {
                                        session.set_peer_volume(name, vol).await;
                                    });
                                }

                                egui::ComboBox::from_id_salt(format!("recv-{}", stream.identity))
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
                            }
                        });

                        if stream.receive == ReceiveQuality::Off {
                            if ui.button("Transmissão desligada. Clique para ligar.").clicked() {
                                let session = app.session.clone();
                                let id = stream.identity.clone();
                                app.rt.spawn(async move {
                                    session
                                        .set_receive_quality(id, ReceiveQuality::Auto)
                                        .await;
                                });
                            }
                        } else if let Some((_, tex)) = app.textures.get(&stream.identity) {
                            let avail = ui.available_size();
                            let aspect = tex.aspect_ratio();
                            let w = avail.x.min(avail.y * aspect).max(160.0);
                            let h = (w / aspect).max(90.0);
                            ui.image((tex.id(), egui::vec2(w, h)));
                        } else {
                            ui.label("Aguardando vídeo…");
                        }
                    });
                ui.add_space(8.0);
            }
        }
    });

    if app.show_share_dialog {
        share_dialog::render_share_dialog(app, ctx);
    }
}

fn ports_warning(snap: &ct_media::session::SessionSnapshot) -> Option<&'static str> {
    if !snap.is_host() {
        return None;
    }
    let info = snap.info.as_ref()?;
    let via_ipv6 = info.endpoint.contains(':');
    if info.cgnat == Some(true) && via_ipv6 {
        Some(
            "Sua internet tem CGNAT, então esta sala foi criada pelo IPv6. Quem for entrar precisa ter IPv6 na internet (libere TCP 7880–7881 e UDP 7882 no firewall IPv6 do roteador).",
        )
    } else if info.cgnat == Some(true) {
        Some(
            "Sua internet usa CGNAT: amigos de fora não conseguem alcançar seu PC pelo IPv4. Peça para outro amigo criar a sala.",
        )
    } else if snap.meta.as_ref().map(|m| m.scope) == Some(RoomScope::Internet)
        && !via_ipv6
        && info.ports_open == Some(false)
    {
        Some(
            "Não consegui abrir as portas no roteador (UPnP). Abra manualmente TCP 7880–7881 e UDP 7882 se amigos de fora não conseguirem entrar.",
        )
    } else if snap.meta.as_ref().map(|m| m.scope) == Some(RoomScope::Lan) {
        Some("Sem IP público: esta sala só funciona para quem está na mesma rede local.")
    } else {
        None
    }
}
