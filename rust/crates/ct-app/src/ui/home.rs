use ct_core::settings::ThemeId;
use ct_media::p2p::is_iroh_ticket;
use ct_media::session::Phase;
use egui::{
    Align, Color32, Context, CornerRadius, Layout, Order, RichText, Stroke, pos2, vec2,
};

use super::CompartilharApp;
use super::theme::{
    ACCENT_LIVE, APP_NAME, BG_CARD, BG_CONTROL, BG_SURFACE, BG_VOID, BORDER_STRONG, BORDER_SUBTLE,
    CUSTOM_COLOR_PRESETS, INK_ON_WHITE, MARATHON_GREEN, RED_ACCENT, TEXT_MUTED, TEXT_SECONDARY,
    ThemePalette, WHITE_PURE, display_text, draw_logo_badge, floating_card_shadow, mono_text,
    paint_marathon_backdrop, primary_white_button, render_admin_warning, tactical_accent_button,
};

pub fn render_home(app: &mut CompartilharApp, ctx: &Context) {
    let pal = app.palette();

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(BG_VOID))
        .show(ctx, |ui| {
            let full_rect = ui.max_rect();
            paint_marathon_backdrop(ui.painter(), full_rect, pal);

            // Barra superior discreta com seletor de temas (Vermelho / Verde / Personalizado RGB)
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(16.0);
                    render_theme_switcher(app, ctx, ui);
                });
            });

            let avail = ui.available_size();
            let content_w = (avail.x - 48.0).clamp(560.0, 760.0);

            if avail.y < 470.0 {
                // Fallback de rolagem apenas em janelas extremamente baixas
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        render_home_body(app, ui, pal, content_w, avail.y);
                    });
                });
            } else {
                // Layout totalmente responsivo ao tamanho da janela — sem scroll!
                let estimated_h = estimate_home_height(avail.y, app.snap.error.is_some(), app.snap.elevated);
                let top_pad = ((avail.y - estimated_h) * 0.42).clamp(4.0, 64.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(top_pad);
                    render_home_body(app, ui, pal, content_w, avail.y);
                });
            }
        });

    if app.show_color_picker {
        render_custom_color_popover(app, ctx);
    }
}

fn estimate_home_height(avail_h: f32, has_err: bool, elevated: bool) -> f32 {
    let compact = avail_h < 650.0;
    let mut h = if compact { 468.0 } else { 548.0 };
    if has_err {
        h += 52.0;
    }
    if elevated {
        h += 48.0;
    }
    h
}

fn render_home_body(
    app: &mut CompartilharApp,
    ui: &mut egui::Ui,
    pal: ThemePalette,
    content_w: f32,
    avail_h: f32,
) {
    let compact = avail_h < 650.0;
    let logo_size = (avail_h * 0.095).clamp(44.0, 72.0);
    let title_size = if compact { 32.0 } else { 40.0 };
    let section_gap = if compact { 10.0 } else { 14.0 };
    let hero_gap = if compact { 14.0 } else { 24.0 };
    let card_margin = if compact { 14 } else { 18 };
    let step2_min_h = (avail_h * 0.27).clamp(174.0, 212.0);
    let btn_h = if compact { 38.0 } else { 42.0 };

    // Hero: Logo personalizado quadrado + marca PIT + tag tática
    ui.allocate_ui_with_layout(vec2(content_w, 0.0), Layout::top_down(Align::Center), |ui| {
        draw_logo_badge(ui, app.logo_texture.as_ref(), logo_size, true);
        ui.add_space(if compact { 8.0 } else { 12.0 });

        ui.horizontal(|ui| {
            let row_w = if compact { 174.0 } else { 190.0 };
            let pad = ((ui.available_width() - row_w) * 0.5).max(0.0);
            ui.add_space(pad);
            ui.label(display_text(APP_NAME, title_size).color(WHITE_PURE));
            ui.add_space(8.0);
            egui::Frame::NONE
                .fill(pal.accent)
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    ui.label(mono_text("P2P // DIRETO", 11.0).color(pal.accent_ink));
                });
        });

        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Compartilhe sua tela e o som do seu PC com até 10 amigos — sem configurar nada.",
            )
            .size(if compact { 13.5 } else { 14.5 })
            .color(TEXT_SECONDARY),
        );
    });

    ui.add_space(hero_gap);

    // Container principal centralizado
    ui.allocate_ui_with_layout(vec2(content_w, 0.0), Layout::top_down(Align::LEFT), |ui| {
        // Passo 1: Nome do usuário
        egui::Frame::NONE
            .fill(BG_SURFACE)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(16))
            .shadow(floating_card_shadow())
            .inner_margin(egui::Margin::same(card_margin))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    step_pill(ui, "01", pal.accent, pal.accent_ink);
                    ui.label(display_text("Seu apelido", 15.0).color(WHITE_PURE));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new("Como seus amigos vão te ver")
                                .size(12.0)
                                .color(TEXT_MUTED),
                        );
                    });
                });
                ui.add_space(if compact { 6.0 } else { 9.0 });
                let nick_resp = ui.add(
                    egui::TextEdit::singleline(&mut app.nickname_input)
                        .char_limit(32)
                        .hint_text("Ex.: Pedro, Ana, Joao...")
                        .desired_width(f32::INFINITY)
                        .margin(vec2(12.0, if compact { 8.0 } else { 10.0 })),
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

        ui.add_space(section_gap);
        render_admin_warning(ui, app.snap.elevated);

        if let Some(ref err) = app.snap.error {
            egui::Frame::NONE
                .fill(Color32::from_rgb(54, 17, 22))
                .stroke(Stroke::new(1.0_f32, RED_ACCENT))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.colored_label(Color32::from_rgb(255, 214, 218), err);
                });
            ui.add_space(section_gap);
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
                .inner_margin(egui::Margin::same(card_margin))
                .show(&mut cols[0], |ui| {
                    ui.set_min_height(step2_min_h);
                    accent_top_bar(ui, WHITE_PURE);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        step_pill(ui, "02", WHITE_PURE, INK_ON_WHITE);
                        ui.label(
                            display_text("Criar uma sala", if compact { 17.0 } else { 19.0 })
                                .color(WHITE_PURE),
                        );
                    });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(
                            "Abre uma sala nova na hora. Depois é só clicar em Copiar convite e mandar para seus amigos.",
                        )
                        .size(if compact { 12.5 } else { 13.5 })
                        .color(TEXT_SECONDARY),
                    );
                    ui.add_space(8.0);
                    egui::Frame::NONE
                        .fill(BG_CARD)
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(9, 5))
                        .show(ui, |ui| {
                            ui.label(
                                mono_text(
                                    "SEM ABRIR PORTAS  •  TROCA DE HOST AUTOMÁTICA",
                                    9.5,
                                )
                                .color(TEXT_MUTED),
                            );
                        });

                    ui.add_space(if compact { 14.0 } else { 22.0 });
                    let btn = ui.add_enabled(
                        !name.is_empty() && !connecting,
                        primary_white_button(if connecting {
                            "Criando sala…"
                        } else {
                            "Criar sala"
                        })
                        .min_size(vec2(ui.available_width(), btn_h)),
                    );
                    if btn.clicked() {
                        let session = app.session.clone();
                        let nick = name.clone();
                        app.rt.spawn(async move {
                            session.update_settings(|s| s.nickname = nick.clone()).await;
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
                .inner_margin(egui::Margin::same(card_margin))
                .show(&mut cols[1], |ui| {
                    ui.set_min_height(step2_min_h);
                    accent_top_bar(ui, pal.accent);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        step_pill(ui, "02", pal.accent, pal.accent_ink);
                        ui.label(
                            display_text("Entrar em sala", if compact { 17.0 } else { 19.0 })
                                .color(WHITE_PURE),
                        );
                    });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Cole o código de convite enviado pelo seu amigo:")
                            .size(if compact { 12.5 } else { 13.5 })
                            .color(TEXT_SECONDARY),
                    );
                    ui.add_space(8.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut app.code_input)
                            .hint_text("Cole o código (IROH-...) aqui")
                            .desired_width(f32::INFINITY)
                            .margin(vec2(10.0, if compact { 7.0 } else { 9.0 })),
                    );
                    let code_valid =
                        is_iroh_ticket(&app.code_input) && app.code_input.trim().len() > 16;

                    ui.add_space(5.0);
                    if code_valid {
                        ui.label(
                            RichText::new("Código válido — pronto para entrar!")
                                .size(11.5)
                                .color(ACCENT_LIVE),
                        );
                    } else if !app.code_input.trim().is_empty() {
                        ui.label(
                            RichText::new("Cole o código completo começando com IROH-")
                                .size(11.5)
                                .color(pal.accent_bright),
                        );
                    } else {
                        ui.label(
                            RichText::new("Dica: use Ctrl+V para colar o código")
                                .size(11.5)
                                .color(TEXT_MUTED),
                        );
                    }

                    ui.add_space(if compact { 10.0 } else { 14.0 });
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
                        .min_size(vec2(ui.available_width(), btn_h)),
                    );
                    if btn.clicked() {
                        let session = app.session.clone();
                        let code = app.code_input.clone();
                        let nick = name.clone();
                        app.rt.spawn(async move {
                            session.update_settings(|s| s.nickname = nick.clone()).await;
                            session.join_room(code, nick).await;
                        });
                    }
                });
        });

        if name.is_empty() {
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Digite seu apelido acima para liberar os botões.")
                        .size(12.0)
                        .color(TEXT_MUTED),
                );
            });
        }

        ui.add_space(if compact { 10.0 } else { 18.0 });
        ui.vertical_centered(|ui| {
            ui.label(
                mono_text("PIT // v0.2  •  CONEXÃO DIRETA CRIPTOGRAFADA", 10.0)
                    .color(BORDER_STRONG),
            );
        });
    });
}

fn render_theme_switcher(app: &mut CompartilharApp, ctx: &Context, ui: &mut egui::Ui) {
    let custom_color = Color32::from_rgb(
        app.custom_accent[0],
        app.custom_accent[1],
        app.custom_accent[2],
    );

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
                    app.show_color_picker = false;
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
                    app.show_color_picker = false;
                    app.set_theme(ctx, ThemeId::MarathonGreen);
                }

                let custom_resp = theme_chip(
                    ui,
                    "Personalizado",
                    custom_color,
                    app.active_theme == ThemeId::Custom,
                )
                .on_hover_text("Escolher sua própria cor de destaque (RGB)");
                if custom_resp.clicked() {
                    if app.active_theme != ThemeId::Custom {
                        app.set_theme(ctx, ThemeId::Custom);
                        app.show_color_picker = true;
                    } else {
                        app.show_color_picker = !app.show_color_picker;
                    }
                }
            });
        });
}

fn render_custom_color_popover(app: &mut CompartilharApp, ctx: &Context) {
    let mut open = app.show_color_picker;
    let [mut r, mut g, mut b] = app.custom_accent;
    let mut color = Color32::from_rgb(r, g, b);
    let mut changed = false;
    let mut persist_now = false;

    // Posição X fixa na tela (fixed_pos) + largura travada:
    // o card da caixa de cores nunca se move nem treme ao arrastar a bolinha!
    let popup_w = 276.0_f32;
    let screen_r = ctx.screen_rect();
    let fixed_x = (screen_r.right() - popup_w - 36.0).max(16.0);

    egui::Area::new(egui::Id::new("custom_rgb_picker"))
        .fixed_pos(pos2(fixed_x, 52.0))
        .order(Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, BORDER_STRONG))
                .corner_radius(CornerRadius::same(14))
                .shadow(floating_card_shadow())
                .inner_margin(egui::Margin::same(14))
                .show(ui, |ui| {
                    ui.set_min_width(popup_w);
                    ui.set_max_width(popup_w);

                    // Cabeçalho com código HEX em largura fixa e botão Fechar
                    ui.horizontal(|ui| {
                        ui.label(display_text("Cor personalizada", 14.0).color(WHITE_PURE));
                        egui::Frame::NONE
                            .fill(BG_CARD)
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(egui::Margin::symmetric(6, 2))
                            .show(ui, |ui| {
                                ui.set_min_width(58.0);
                                ui.label(
                                    mono_text(format!("#{r:02X}{g:02X}{b:02X}"), 11.0)
                                        .color(color),
                                );
                            });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("Fechar").clicked() {
                                open = false;
                                persist_now = true;
                            }
                        });
                    });

                    ui.add_space(8.0);
                    ui.label(RichText::new("Cores rápidas:").size(11.5).color(TEXT_MUTED));
                    ui.add_space(4.0);

                    // 8 presets rápidos em 1 clique
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for &(preset_name, rgb) in CUSTOM_COLOR_PRESETS {
                            let sw = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                            let selected = app.custom_accent == rgb;
                            let (rect, resp) =
                                ui.allocate_exact_size(vec2(26.0, 26.0), egui::Sense::click());
                            ui.painter().rect(
                                rect,
                                CornerRadius::same(7),
                                sw,
                                Stroke::new(
                                    if selected { 2.0_f32 } else { 1.0_f32 },
                                    if selected { WHITE_PURE } else { BORDER_SUBTLE },
                                ),
                                egui::StrokeKind::Inside,
                            );
                            if resp.on_hover_text(preset_name).clicked() {
                                r = rgb[0];
                                g = rgb[1];
                                b = rgb[2];
                                color = sw;
                                changed = true;
                                persist_now = true;
                            }
                        }
                    });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(8.0);

                    // Caixa visual com as cores (com slider_width travado para não tremer)
                    ui.spacing_mut().slider_width = 236.0;
                    if egui::color_picker::color_picker_color32(
                        ui,
                        &mut color,
                        egui::color_picker::Alpha::Opaque,
                    ) {
                        r = color.r();
                        g = color.g();
                        b = color.b();
                        changed = true;
                    }
                });
        });

    if changed {
        app.set_custom_accent_live(ctx, [r, g, b], persist_now);
    } else if persist_now {
        app.flush_theme_settings();
    }
    app.show_color_picker = open;
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
