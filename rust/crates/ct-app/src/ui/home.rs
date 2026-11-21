use ct_media::p2p::is_iroh_ticket;
use ct_media::session::Phase;
use egui::{Align, Color32, Context, Layout, RichText, Stroke};

use super::{
    ACCENT_DANGER, ACCENT_GREEN, BG_CARD, BORDER_SUBTLE, CompartilharApp, TEXT_MUTED,
    primary_button, render_admin_warning,
};

pub fn render_home(app: &mut CompartilharApp, ctx: &Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(46.0);
            ui.label(
                RichText::new("CompartilhadorDeTela")
                    .size(32.0)
                    .strong()
                    .color(Color32::WHITE),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Transmissão P2P direta de tela e áudio por app para até 10 amigos — sem abrir portas",
                )
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
                        // Card: Criar sala P2P
                        egui::Frame::NONE
                            .fill(BG_CARD)
                            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                            .corner_radius(12.0)
                            .inner_margin(18.0)
                            .show(&mut cols[0], |ui| {
                                ui.label(RichText::new("Criar nova sala").size(18.0).strong());
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(
                                        "Cria uma sala P2P instantânea via Iroh (QUIC). Funciona com CGNAT, sem precisar abrir portas no roteador e com troca automática de host.",
                                    )
                                    .color(TEXT_MUTED)
                                    .size(13.0),
                                );
                                ui.add_space(28.0);
                                let btn = ui.add_enabled(
                                    !name.is_empty() && !connecting,
                                    primary_button(if connecting {
                                        "Criando sala…"
                                    } else {
                                        "Criar sala"
                                    })
                                    .min_size(egui::vec2(ui.available_width(), 40.0)),
                                );
                                if btn.clicked() {
                                    let session = app.session.clone();
                                    let nick = name.clone();
                                    app.rt.spawn(async move {
                                        session
                                            .update_settings(|s| s.nickname = nick.clone())
                                            .await;
                                        session.create_room(nick).await;
                                    });
                                }
                            });

                        // Card: Entrar em sala P2P
                        egui::Frame::NONE
                            .fill(BG_CARD)
                            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                            .corner_radius(12.0)
                            .inner_margin(18.0)
                            .show(&mut cols[1], |ui| {
                                ui.label(RichText::new("Entrar em sala").size(18.0).strong());
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new("Cole o código P2P (IROH-...) enviado pelo seu amigo.")
                                        .color(TEXT_MUTED)
                                        .size(13.0),
                                );
                                ui.add_space(10.0);
                                ui.add(
                                    egui::TextEdit::singleline(&mut app.code_input)
                                        .hint_text("IROH-...")
                                        .desired_width(f32::INFINITY)
                                        .margin(egui::vec2(10.0, 8.0)),
                                );
                                let code_valid = is_iroh_ticket(&app.code_input)
                                    && app.code_input.trim().len() > 16;

                                ui.add_space(6.0);
                                if code_valid {
                                    ui.label(
                                        RichText::new("Código P2P válido")
                                            .size(12.0)
                                            .color(ACCENT_GREEN),
                                    );
                                } else {
                                    ui.label(
                                        RichText::new("Cole o código completo começando com IROH-")
                                            .size(12.0)
                                            .color(TEXT_MUTED),
                                    );
                                }

                                ui.add_space(16.0);
                                let btn = ui.add_enabled(
                                    !name.is_empty() && code_valid && !connecting,
                                    primary_button(if connecting {
                                        "Entrando…"
                                    } else {
                                        "Entrar na sala"
                                    })
                                    .min_size(egui::vec2(ui.available_width(), 40.0)),
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
