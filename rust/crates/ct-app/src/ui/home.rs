use ct_core::protocol::RoomMode;
use ct_core::room_code::format_room_code_input;
use ct_media::p2p::is_iroh_ticket;
use ct_media::session::Phase;
use egui::{Align, Color32, Context, Layout, RichText, Stroke};

use super::{
    ACCENT_DANGER, ACCENT_PRIMARY, BG_CARD, BORDER_SUBTLE, CompartilharApp, TEXT_MUTED,
    primary_button, render_admin_warning,
};

pub fn render_home(app: &mut CompartilharApp, ctx: &Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(42.0);
            ui.label(
                RichText::new("CompartilhadorDeTela")
                    .size(32.0)
                    .strong()
                    .color(Color32::WHITE),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("Transmita sua tela com áudio por aplicativo para até 10 amigos")
                    .size(15.0)
                    .color(TEXT_MUTED),
            );
            ui.add_space(28.0);

            ui.allocate_ui_with_layout(
                egui::vec2(640.0, 0.0),
                Layout::top_down(Align::LEFT),
                |ui| {
                    // Card do apelido
                    egui::Frame::NONE
                        .fill(BG_CARD)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .corner_radius(12.0)
                        .inner_margin(16.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Seu apelido").strong());
                            ui.add_space(4.0);
                            let nick_resp = ui.add(
                                egui::TextEdit::singleline(&mut app.nickname_input)
                                    .char_limit(32)
                                    .hint_text("Digite como seus amigos vão te ver...")
                                    .desired_width(f32::INFINITY)
                                    .margin(egui::vec2(10.0, 8.0)),
                            );
                            if nick_resp.lost_focus() {
                                let name = app.nickname_input.trim().to_string();
                                if name != app.snap.settings.nickname {
                                    let session = app.session.clone();
                                    app.rt.spawn(async move {
                                        session.update_settings(|s| s.nickname = name).await;
                                    });
                                }
                            }
                        });

                    ui.add_space(12.0);
                    render_admin_warning(ui, app.snap.elevated);

                    if let Some(ref err) = app.snap.error {
                        egui::Frame::NONE
                            .fill(Color32::from_rgb(96, 36, 40))
                            .stroke(Stroke::new(1.0_f32, ACCENT_DANGER))
                            .corner_radius(8.0)
                            .inner_margin(12.0)
                            .show(ui, |ui| {
                                ui.colored_label(Color32::from_rgb(255, 215, 215), err);
                            });
                        ui.add_space(12.0);
                    }

                    let name = app.nickname_input.trim().to_string();
                    let connecting = app.snap.phase == Phase::Connecting;

                    ui.columns(2, |cols| {
                        // Card: Criar sala
                        egui::Frame::NONE
                            .fill(BG_CARD)
                            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                            .corner_radius(12.0)
                            .inner_margin(18.0)
                            .show(&mut cols[0], |ui| {
                                ui.label(RichText::new("Criar nova sala").size(18.0).strong());
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("Escolha o tipo de conexão da sala:")
                                        .color(TEXT_MUTED)
                                        .size(13.0),
                                );
                                ui.add_space(10.0);

                                let iroh_selected = app.create_mode == RoomMode::Iroh;
                                ui.selectable_label(
                                    iroh_selected,
                                    RichText::new("P2P Direto (Iroh) — Recomendado").strong(),
                                )
                                .clicked()
                                .then(|| app.create_mode = RoomMode::Iroh);
                                ui.label(
                                    RichText::new("  Funciona com CGNAT, sem abrir portas.")
                                        .size(12.0)
                                        .color(TEXT_MUTED),
                                );

                                ui.add_space(6.0);

                                let classic_selected = app.create_mode == RoomMode::Classic;
                                ui.selectable_label(
                                    classic_selected,
                                    RichText::new("Clássico (LiveKit)").strong(),
                                )
                                .clicked()
                                .then(|| app.create_mode = RoomMode::Classic);
                                ui.label(
                                    RichText::new("  Compatível com o app Electron antigo.")
                                        .size(12.0)
                                        .color(TEXT_MUTED),
                                );

                                ui.add_space(16.0);
                                let btn = ui.add_enabled(
                                    !name.is_empty() && !connecting,
                                    primary_button(if connecting {
                                        "Criando sala…"
                                    } else {
                                        "Criar sala"
                                    })
                                    .min_size(egui::vec2(ui.available_width(), 38.0)),
                                );
                                if btn.clicked() {
                                    let session = app.session.clone();
                                    let nick = name.clone();
                                    let mode = app.create_mode;
                                    app.rt.spawn(async move {
                                        session
                                            .update_settings(|s| s.nickname = nick.clone())
                                            .await;
                                        session.create_room(nick, mode).await;
                                    });
                                }
                            });

                        // Card: Entrar em sala
                        egui::Frame::NONE
                            .fill(BG_CARD)
                            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                            .corner_radius(12.0)
                            .inner_margin(18.0)
                            .show(&mut cols[1], |ui| {
                                ui.label(RichText::new("Entrar em sala").size(18.0).strong());
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("Cole o código recebido do seu amigo (Iroh P2P ou Clássico).")
                                        .color(TEXT_MUTED)
                                        .size(13.0),
                                );
                                ui.add_space(12.0);
                                let code_resp = ui.add(
                                    egui::TextEdit::singleline(&mut app.code_input)
                                        .hint_text("IROH-... ou XXXX-XXXX-XXXX")
                                        .desired_width(f32::INFINITY)
                                        .margin(egui::vec2(10.0, 8.0)),
                                );
                                if code_resp.changed() && !is_iroh_ticket(&app.code_input) {
                                    let trimmed = app.code_input.trim();
                                    if !trimmed.to_ascii_uppercase().starts_with("IROH") {
                                        app.code_input = format_room_code_input(trimmed);
                                    }
                                }
                                let is_iroh = is_iroh_ticket(&app.code_input)
                                    && app.code_input.trim().len() > 16;
                                let raw_len = app.code_input.replace('-', "").len();
                                let code_complete = is_iroh || raw_len == 12 || raw_len == 32;

                                ui.add_space(8.0);
                                let hint = if is_iroh {
                                    ("Ticket P2P (Iroh) detectado", ACCENT_PRIMARY)
                                } else if raw_len == 12 || raw_len == 32 {
                                    ("Código Clássico detectado", ACCENT_PRIMARY)
                                } else {
                                    ("Aguardando código completo...", TEXT_MUTED)
                                };
                                ui.label(RichText::new(hint.0).size(12.0).color(hint.1));

                                ui.add_space(28.0);
                                let btn = ui.add_enabled(
                                    !name.is_empty() && code_complete && !connecting,
                                    primary_button(if connecting {
                                        "Entrando…"
                                    } else {
                                        "Entrar na sala"
                                    })
                                    .min_size(egui::vec2(ui.available_width(), 38.0)),
                                );
                                if btn.clicked() {
                                    let session = app.session.clone();
                                    let code = app.code_input.clone();
                                    let nick = name.clone();
                                    app.rt.spawn(async move {
                                        session
                                            .update_settings(|s| s.nickname = nick.clone())
                                            .await;
                                        session.join_room(code, nick).await;
                                    });
                                }
                            });
                    });
                },
            );
        });
    });
}
