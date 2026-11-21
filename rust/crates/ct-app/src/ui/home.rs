use ct_core::protocol::RoomMode;
use ct_core::room_code::format_room_code_input;
use ct_media::p2p::is_iroh_ticket;
use ct_media::session::Phase;
use egui::{Align, Color32, Context, Layout, RichText};

use super::{CompartilharApp, render_admin_warning};

pub fn render_home(app: &mut CompartilharApp, ctx: &Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(36.0);
            ui.heading(RichText::new("CompartilhadorDeTela").size(28.0).strong());
            ui.label(
                RichText::new("Transmita sua tela com áudio para até 10 amigos (Rust Native)")
                    .color(Color32::GRAY),
            );
            ui.add_space(24.0);

            ui.allocate_ui_with_layout(
                egui::vec2(580.0, 0.0),
                Layout::top_down(Align::LEFT),
                |ui| {
                    ui.label("Seu apelido");
                    let nick_resp = ui.add(
                        egui::TextEdit::singleline(&mut app.nickname_input)
                            .char_limit(32)
                            .hint_text("Como os amigos vão te ver")
                            .desired_width(f32::INFINITY),
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

                    ui.add_space(12.0);
                    render_admin_warning(ui, app.snap.elevated);

                    if let Some(ref err) = app.snap.error {
                        egui::Frame::NONE
                            .fill(Color32::from_rgb(100, 28, 32))
                            .corner_radius(6.0)
                            .inner_margin(10.0)
                            .show(ui, |ui| {
                                ui.colored_label(Color32::from_rgb(255, 200, 200), err);
                            });
                        ui.add_space(12.0);
                    }

                    let name = app.nickname_input.trim().to_string();
                    let connecting = app.snap.phase == Phase::Connecting;

                    ui.columns(2, |cols| {
                        // Criar sala
                        egui::Frame::NONE
                            .fill(Color32::from_rgb(24, 27, 34))
                            .corner_radius(8.0)
                            .inner_margin(16.0)
                            .show(&mut cols[0], |ui| {
                                ui.heading("Criar sala");
                                ui.add_space(6.0);
                                ui.radio_value(
                                    &mut app.create_mode,
                                    RoomMode::Iroh,
                                    "P2P (Iroh) — sem abrir portas / CGNAT",
                                );
                                ui.radio_value(
                                    &mut app.create_mode,
                                    RoomMode::Classic,
                                    "Clássico (LiveKit) — compatível Electron",
                                );
                                ui.add_space(12.0);
                                let btn = ui.add_enabled(
                                    !name.is_empty() && !connecting,
                                    egui::Button::new(if connecting {
                                        "Conectando…"
                                    } else {
                                        "Criar sala"
                                    })
                                    .min_size(egui::vec2(ui.available_width(), 34.0)),
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

                        // Entrar em sala
                        egui::Frame::NONE
                            .fill(Color32::from_rgb(24, 27, 34))
                            .corner_radius(8.0)
                            .inner_margin(16.0)
                            .show(&mut cols[1], |ui| {
                                ui.heading("Entrar em sala");
                                ui.add_space(6.0);
                                ui.small("Aceita ticket IROH-… (P2P) ou código clássico XXXX-XXXX-XXXX.");
                                ui.add_space(4.0);
                                let code_resp = ui.add(
                                    egui::TextEdit::singleline(&mut app.code_input)
                                        .hint_text("IROH-... ou XXXX-XXXX-XXXX")
                                        .desired_width(f32::INFINITY),
                                );
                                if code_resp.changed() && !is_iroh_ticket(&app.code_input) {
                                    // Só auto-formata com traços se não for um ticket Iroh
                                    let trimmed = app.code_input.trim();
                                    if !trimmed.to_ascii_uppercase().starts_with("IROH") {
                                        app.code_input = format_room_code_input(trimmed);
                                    }
                                }
                                let is_iroh = is_iroh_ticket(&app.code_input)
                                    && app.code_input.trim().len() > 16;
                                let raw_len = app.code_input.replace('-', "").len();
                                let code_complete = is_iroh || raw_len == 12 || raw_len == 32;
                                ui.add_space(12.0);
                                let btn = ui.add_enabled(
                                    !name.is_empty() && code_complete && !connecting,
                                    egui::Button::new(if connecting {
                                        "Conectando…"
                                    } else {
                                        "Entrar"
                                    })
                                    .min_size(egui::vec2(ui.available_width(), 34.0)),
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
