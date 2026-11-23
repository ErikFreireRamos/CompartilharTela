use ct_core::settings::ThemeId;
use ct_media::p2p::is_iroh_ticket;
use ct_media::session::Phase;
use egui::{Align, Color32, Context, CornerRadius, Layout, RichText, Stroke, pos2, vec2};

use super::CompartilharApp;
use super::theme::{
    ACCENT_LIVE, APP_NAME, BG_CARD, BG_CONTROL, BG_SURFACE, BG_VOID, BORDER_STRONG, BORDER_SUBTLE,
    INK_ON_WHITE, MARATHON_GREEN, RED_ACCENT, TEXT_MUTED, TEXT_SECONDARY, WHITE_PURE,
    display_text, draw_logo_badge, floating_card_shadow, mono_text, paint_marathon_backdrop,
    primary_white_button, render_admin_warning, tactical_accent_button,
};

pub fn render_home(app: &mut CompartilharApp, ctx: &Context) {
    let pal = app.palette();

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(BG_VOID))
        .show(ctx, |ui| {
            let full_rect = ui.max_rect();
            paint_marathon_backdrop(ui.painter(), full_rect, pal);

            // Barra superior discreta com seletor dos 2 temas (Vermelho / Verde Marathon)
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(16.0);
                    render_theme_switcher(app, ctx, ui);
                });
            });

            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(16.0);

                    // Hero: Logo personalizado quadrado + marca PIT + tag tática
                    ui.allocate_ui_with_layout(
                        vec2(680.0, 0.0),
                        Layout::top_down(Align::Center),
                        |ui| {
                            draw_logo_badge(ui, app.logo_texture.as_ref(), 72.0, true);
                            ui.add_space(14.0);

                            ui.horizontal(|ui| {
                                let row_w = 190.0;
                                let pad = ((ui.available_width() - row_w) * 0.5).max(0.0);
                                ui.add_space(pad);
                                ui.label(display_text(APP_NAME, 40.0).color(WHITE_PURE));
                                ui.add_space(8.0);
                                egui::Frame::NONE
                                    .fill(pal.accent)
                                    .corner_radius(CornerRadius::same(6))
                                    .inner_margin(egui::Margin::symmetric(8, 3))
                                    .show(ui, |ui| {
                                        ui.label(
                                            mono_text("P2P // DIRETO", 11.0).color(pal.accent_ink),
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

                    ui.add_space(26.0);

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
                                        step_pill(ui, "01", pal.accent, pal.accent_ink);
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
                                    .fill(Color32::from_rgb(54, 17, 22))
                                    .stroke(Stroke::new(1.0_f32, RED_ACCENT))
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
                                // Card esquerdo — Criar sala (acento branco puro)
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
                                            step_pill(ui, "02", WHITE_PURE, INK_ON_WHITE);
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

                                // Card direito — Entrar em uma sala (cor de acento do tema)
                                egui::Frame::NONE
                                    .fill(BG_SURFACE)
                                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                                    .corner_radius(CornerRadius::same(16))
                                    .shadow(floating_card_shadow())
                                    .inner_margin(egui::Margin::same(20))
                                    .show(&mut cols[1], |ui| {
                                        ui.set_min_height(216.0);
                                        accent_top_bar(ui, pal.accent);
                                        ui.add_space(10.0);
                                        ui.horizontal(|ui| {
                                            step_pill(ui, "02", pal.accent, pal.accent_ink);
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
                                                .color(pal.accent_bright),
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
                                            tactical_accent_button(
                                                if connecting {
                                                    "Entrando…"
                                                } else {
                                                    "Entrar na sala"
                                                },
                                                pal,
                                            )
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

                            ui.add_space(22.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    mono_text("PIT // v0.2  •  CONEXÃO DIRETA CRIPTOGRAFADA", 10.5)
                                        .color(BORDER_STRONG),
                                );
                            });
                        },
                    );
                    ui.add_space(24.0);
                });
            });
        });
}

fn render_theme_switcher(app: &mut CompartilharApp, ctx: &Context, ui: &mut egui::Ui) {
    egui::Frame::NONE
        .fill(BG_SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(mono_text("TEMA", 10.0).color(TEXT_MUTED));

                if theme_chip(
                    ui,
                    "Vermelho",
                    RED_ACCENT,
                    app.active_theme == ThemeId::Red,
                )
                .clicked()
                {
                    app.set_theme(ctx, ThemeId::Red);
                }

                if theme_chip(
                    ui,
                    "Verde",
                    MARATHON_GREEN,
                    app.active_theme == ThemeId::MarathonGreen,
                )
                .clicked()
                {
                    app.set_theme(ctx, ThemeId::MarathonGreen);
                }
            });
        });
}

fn theme_chip(ui: &mut egui::Ui, label: &str, swatch: Color32, active: bool) -> egui::Response {
    let font_id = egui::FontId::new(
        12.0,
        if active {
            super::theme::display_family()
        } else {
            egui::FontFamily::Proportional
        },
    );
    let text_color = if active { WHITE_PURE } else { TEXT_SECONDARY };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font_id, text_color);
    let total_w = 10.0 + 10.0 + 6.0 + galley.size().x + 10.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(total_w, 26.0), egui::Sense::click());

    let p = ui.painter();
    let bg = if active {
        BG_CONTROL
    } else if resp.hovered() {
        BG_CARD
    } else {
        Color32::TRANSPARENT
    };
    let border = if active {
        swatch
    } else if resp.hovered() {
        BORDER_STRONG
    } else {
        Color32::TRANSPARENT
    };
    p.rect(
        rect,
        CornerRadius::same(8),
        bg,
        Stroke::new(1.0_f32, border),
        egui::StrokeKind::Inside,
    );

    let dot_center = pos2(rect.left() + 14.0, rect.center().y);
    p.circle_filled(dot_center, 4.5, swatch);
    p.galley(
        pos2(dot_center.x + 10.0, rect.center().y - galley.size().y * 0.5),
        galley,
        text_color,
    );
    resp
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
