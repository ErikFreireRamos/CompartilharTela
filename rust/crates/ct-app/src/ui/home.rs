use ct_media::p2p::is_iroh_ticket;
use ct_media::session::Phase;
use egui::{Align, Color32, Context, CornerRadius, Layout, RichText, Stroke, pos2, vec2};

use super::CompartilharApp;
use super::theme::{
    ACCENT_LIVE, ACCENT_RED, ACCENT_RED_BRIGHT, ACCENT_RED_DIM, APP_NAME, BG_CARD, BG_SURFACE,
    BG_VOID, BORDER_STRONG, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY, WHITE_PURE,
    display_text, draw_logo_badge, floating_card_shadow, mono_text, paint_marathon_backdrop,
    primary_white_button, render_admin_warning, tactical_red_button,
};

pub fn render_home(app: &mut CompartilharApp, ctx: &Context) {
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(BG_VOID))
        .show(ctx, |ui| {
            let full_rect = ui.max_rect();
            paint_marathon_backdrop(ui.painter(), full_rect);

            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(44.0);

                    // Hero: Logo personalizado + marca PIT + tag tática Marathon
                    ui.allocate_ui_with_layout(
                        vec2(680.0, 0.0),
                        Layout::top_down(Align::Center),
                        |ui| {
                            draw_logo_badge(ui, app.logo_texture.as_ref(), 74.0, true);
                            ui.add_space(14.0);

                            ui.horizontal(|ui| {
                                let row_w = 190.0;
                                let pad = ((ui.available_width() - row_w) * 0.5).max(0.0);
                                ui.add_space(pad);
                                ui.label(display_text(APP_NAME, 40.0).color(WHITE_PURE));
                                ui.add_space(8.0);
                                egui::Frame::NONE
                                    .fill(ACCENT_RED)
                                    .corner_radius(CornerRadius::same(6))
                                    .inner_margin(egui::Margin::symmetric(8, 3))
                                    .show(ui, |ui| {
                                        ui.label(
                                            mono_text("P2P // DIRETO", 11.0).color(WHITE_PURE),
                                        );
                                    });
                            });

                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(
                                    "Compartilhe sua tela e o som do seu PC com até 10 amigos — sem configurar nada.",
                                )
                                .size(15.0)
                                .color(TEXT_SECONDARY),
                            );
                        },
                    );

                    ui.add_space(28.0);

                    // Container principal centralizado
                    ui.allocate_ui_with_layout(
                        vec2(680.0, 0.0),
                        Layout::top_down(Align::LEFT),
                        |ui| {
                            // Passo 1: Nome do usuário
                            egui::Frame::NONE
                                .fill(BG_SURFACE)
                                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                                .corner_radius(CornerRadius::same(16))
                                .shadow(floating_card_shadow())
                                .inner_margin(egui::Margin::same(18))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        step_pill(ui, "01", ACCENT_RED, WHITE_PURE);
                                        ui.label(
                                            display_text("Seu apelido", 15.0).color(WHITE_PURE),
                                        );
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new("Como seus amigos vão te ver")
                                                        .size(12.0)
                                                        .color(TEXT_MUTED),
                                                );
                                            },
                                        );
                                    });
                                    ui.add_space(10.0);
                                    let nick_resp = ui.add(
                                        egui::TextEdit::singleline(&mut app.nickname_input)
                                            .char_limit(32)
                                            .hint_text("Ex.: Pedro, Ana, Joao...")
                                            .desired_width(f32::INFINITY)
                                            .margin(vec2(12.0, 10.0)),
                                    );
                                    if nick_resp.lost_focus() {
                                        let name = app.nickname_input.trim().to_string();
                                        if name != app.snap.settings.nickname {
                                            let session = app.session.clone();
                                            app.rt.spawn(async move {
                                                session
                                                    .update_settings(|s| s.nickname = name)
                                                    .await;
                                            });
                                        }
                                    }
                                });

                            ui.add_space(14.0);
                            render_admin_warning(ui, app.snap.elevated);

                            if let Some(ref err) = app.snap.error {
                                egui::Frame::NONE
                                    .fill(ACCENT_RED_DIM)
                                    .stroke(Stroke::new(1.0_f32, ACCENT_RED))
                                    .corner_radius(CornerRadius::same(12))
                                    .inner_margin(12.0)
                                    .show(ui, |ui| {
                                        ui.colored_label(
                                            Color32::from_rgb(255, 214, 218),
                                            err,
                                        );
                                    });
                                ui.add_space(14.0);
                            }

                            let name = app.nickname_input.trim().to_string();
                            let connecting = app.snap.phase == Phase::Connecting;

                            // Passo 2: Criar ou Entrar em uma sala (cards flutuantes lado a lado)
                            ui.columns(2, |cols| {
                                // Card esquerdo — Criar sala (acento branco WSTR)
                                egui::Frame::NONE
                                    .fill(BG_SURFACE)
                                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                                    .corner_radius(CornerRadius::same(16))
                                    .shadow(floating_card_shadow())
                                    .inner_margin(egui::Margin::same(20))
                                    .show(&mut cols[0], |ui| {
                                        ui.set_min_height(216.0);
                                        accent_top_bar(ui, WHITE_PURE);
                                        ui.add_space(10.0);
                                        ui.horizontal(|ui| {
                                            step_pill(ui, "02", WHITE_PURE, Color32::BLACK);
                                            ui.label(
                                                display_text("Criar uma sala", 19.0)
                                                    .color(WHITE_PURE),
                                            );
                                        });
                                        ui.add_space(10.0);
                                        ui.label(
                                            RichText::new(
                                                "Abre uma sala nova na hora. Depois é só clicar em Copiar convite e mandar para seus amigos.",
                                            )
                                            .size(13.5)
                                            .color(TEXT_SECONDARY),
                                        );
                                        ui.add_space(10.0);
                                        egui::Frame::NONE
                                            .fill(BG_CARD)
                                            .corner_radius(CornerRadius::same(8))
                                            .inner_margin(egui::Margin::symmetric(10, 6))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    mono_text(
                                                        "SEM ABRIR PORTAS  •  TROCA DE HOST AUTOMÁTICA",
                                                        10.0,
                                                    )
                                                    .color(TEXT_MUTED),
                                                );
                                            });

                                        ui.add_space(28.0);
                                        let btn = ui.add_enabled(
                                            !name.is_empty() && !connecting,
                                            primary_white_button(if connecting {
                                                "Criando sala…"
                                            } else {
                                                "Criar sala"
                                            })
                                            .min_size(vec2(ui.available_width(), 44.0)),
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

                                // Card direito — Entrar em uma sala (acento vermelho tático WSTR)
                                egui::Frame::NONE
                                    .fill(BG_SURFACE)
                                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                                    .corner_radius(CornerRadius::same(16))
                                    .shadow(floating_card_shadow())
                                    .inner_margin(egui::Margin::same(20))
                                    .show(&mut cols[1], |ui| {
                                        ui.set_min_height(216.0);
                                        accent_top_bar(ui, ACCENT_RED);
                                        ui.add_space(10.0);
                                        ui.horizontal(|ui| {
                                            step_pill(ui, "02", ACCENT_RED, WHITE_PURE);
                                            ui.label(
                                                display_text("Entrar em sala", 19.0)
                                                    .color(WHITE_PURE),
                                            );
                                        });
                                        ui.add_space(10.0);
                                        ui.label(
                                            RichText::new(
                                                "Cole o código de convite enviado pelo seu amigo:",
                                            )
                                            .size(13.5)
                                            .color(TEXT_SECONDARY),
                                        );
                                        ui.add_space(10.0);
                                        ui.add(
                                            egui::TextEdit::singleline(&mut app.code_input)
                                                .hint_text("Cole o código (IROH-...) aqui")
                                                .desired_width(f32::INFINITY)
                                                .margin(vec2(12.0, 9.0)),
                                        );
                                        let code_valid = is_iroh_ticket(&app.code_input)
                                            && app.code_input.trim().len() > 16;

                                        ui.add_space(6.0);
                                        if code_valid {
                                            ui.label(
                                                RichText::new("Código válido — pronto para entrar!")
                                                    .size(12.0)
                                                    .color(ACCENT_LIVE),
                                            );
                                        } else if !app.code_input.trim().is_empty() {
                                            ui.label(
                                                RichText::new(
                                                    "Cole o código completo começando com IROH-",
                                                )
                                                .size(12.0)
                                                .color(ACCENT_RED_BRIGHT),
                                            );
                                        } else {
                                            ui.label(
                                                RichText::new("Dica: use Ctrl+V para colar o código")
                                                    .size(12.0)
                                                    .color(TEXT_MUTED),
                                            );
                                        }

                                        ui.add_space(16.0);
                                        let btn = ui.add_enabled(
                                            !name.is_empty() && code_valid && !connecting,
                                            tactical_red_button(if connecting {
                                                "Entrando…"
                                            } else {
                                                "Entrar na sala"
                                            })
                                            .min_size(vec2(ui.available_width(), 44.0)),
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

                            if name.is_empty() {
                                ui.add_space(12.0);
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new(
                                            "Digite seu apelido acima para liberar os botões.",
                                        )
                                        .size(12.5)
                                        .color(TEXT_MUTED),
                                    );
                                });
                            }

                            ui.add_space(24.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    mono_text("PIT // v0.2  •  CONEXÃO DIRETA CRIPTOGRAFADA", 10.5)
                                        .color(BORDER_STRONG),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        mono_text("WSTR EDITION", 10.5).color(TEXT_PRIMARY.linear_multiply(0.35)),
                                    );
                                });
                            });
                        },
                    );
                    ui.add_space(28.0);
                });
            });
        });
}

fn step_pill(ui: &mut egui::Ui, label: &str, bg: Color32, fg: Color32) {
    egui::Frame::NONE
        .fill(bg)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(mono_text(label, 11.0).color(fg));
        });
}

fn accent_top_bar(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 3.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 1.5, Color32::from_rgb(32, 36, 44));
    let bar = egui::Rect::from_min_size(pos2(rect.left(), rect.top()), vec2(44.0, 3.0));
    p.rect_filled(bar, 1.5, color);
}
