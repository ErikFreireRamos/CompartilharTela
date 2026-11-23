use std::sync::Arc;

use ct_core::settings::ThemeId;
use egui::{
    Align2, Color32, ColorImage, Context, CornerRadius, FontData, FontDefinitions, FontFamily,
    FontId, IconData, Rect, Response, RichText, Sense, Shadow, Stroke, TextureHandle,
    TextureOptions, Ui, pos2, vec2,
};

// =============================================================================
// Base cromática PIT — Preto carbono fosco + Branco puro + Cinza titânio
// =============================================================================
pub const BG_VOID: Color32 = Color32::from_rgb(8, 9, 11);
pub const BG_SURFACE: Color32 = Color32::from_rgb(17, 19, 23);
pub const BG_CARD: Color32 = Color32::from_rgb(24, 27, 33);
pub const BG_CONTROL: Color32 = Color32::from_rgb(33, 37, 46);
pub const BG_CONTROL_HOVER: Color32 = Color32::from_rgb(46, 51, 63);
pub const BG_OVERLAY: Color32 = Color32::from_rgba_premultiplied(10, 11, 14, 218);

pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(42, 47, 58);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(72, 79, 94);

pub const WHITE_PURE: Color32 = Color32::from_rgb(255, 255, 255);
pub const INK_ON_WHITE: Color32 = Color32::from_rgb(10, 11, 14);
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(244, 245, 248);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(166, 173, 188);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(112, 120, 136);

// Cores de acento dos dois temas selecionáveis
pub const RED_ACCENT: Color32 = Color32::from_rgb(232, 35, 46);
pub const RED_BRIGHT: Color32 = Color32::from_rgb(255, 59, 71);
pub const RED_DIM: Color32 = Color32::from_rgb(54, 17, 22);

pub const MARATHON_GREEN: Color32 = Color32::from_rgb(194, 254, 12); // #C2FE0C
pub const MARATHON_GREEN_BRIGHT: Color32 = Color32::from_rgb(214, 255, 77);
pub const MARATHON_GREEN_DIM: Color32 = Color32::from_rgb(34, 46, 10);

pub const ACCENT_LIVE: Color32 = Color32::from_rgb(34, 229, 138);
pub const ACCENT_WARN: Color32 = Color32::from_rgb(245, 184, 65);

pub const APP_NAME: &str = "PIT";

#[derive(Clone, Copy, Debug)]
pub struct ThemePalette {
    pub accent: Color32,
    pub accent_bright: Color32,
    pub accent_dim: Color32,
    /// Cor do texto sobre fundo `accent` (branco no vermelho, preto no verde Marathon).
    pub accent_ink: Color32,
}

impl ThemePalette {
    pub fn for_theme(theme: ThemeId) -> Self {
        match theme {
            ThemeId::Red => Self {
                accent: RED_ACCENT,
                accent_bright: RED_BRIGHT,
                accent_dim: RED_DIM,
                accent_ink: WHITE_PURE,
            },
            ThemeId::MarathonGreen => Self {
                accent: MARATHON_GREEN,
                accent_bright: MARATHON_GREEN_BRIGHT,
                accent_dim: MARATHON_GREEN_DIM,
                accent_ink: INK_ON_WHITE,
            },
        }
    }
}

static LOGO_PNG_BYTES: &[u8] = include_bytes!("../../assets/logo.png");
static FONT_GROTESK_MEDIUM: &[u8] = include_bytes!("../../assets/fonts/SpaceGrotesk-Medium.ttf");
static FONT_GROTESK_BOLD: &[u8] = include_bytes!("../../assets/fonts/SpaceGrotesk-Bold.ttf");
static FONT_MONO_MEDIUM: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Medium.ttf");

pub fn display_family() -> FontFamily {
    FontFamily::Name("display".into())
}

pub fn display_text(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text.into()).font(FontId::new(size, display_family()))
}

pub fn mono_text(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text.into()).font(FontId::new(size, FontFamily::Monospace))
}

pub fn configure_theme(ctx: &Context, theme: ThemeId) {
    let pal = ThemePalette::for_theme(theme);

    // 1. Fontes embutidas (Space Grotesk + JetBrains Mono)
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "space_grotesk_medium".to_owned(),
        Arc::new(FontData::from_static(FONT_GROTESK_MEDIUM)),
    );
    fonts.font_data.insert(
        "space_grotesk_bold".to_owned(),
        Arc::new(FontData::from_static(FONT_GROTESK_BOLD)),
    );
    fonts.font_data.insert(
        "jetbrains_mono_medium".to_owned(),
        Arc::new(FontData::from_static(FONT_MONO_MEDIUM)),
    );

    if let Some(prop) = fonts.families.get_mut(&FontFamily::Proportional) {
        prop.insert(0, "space_grotesk_medium".to_owned());
    }
    if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
        mono.insert(0, "jetbrains_mono_medium".to_owned());
    }
    let mut display_fallbacks = vec!["space_grotesk_bold".to_owned()];
    if let Some(prop) = fonts.families.get(&FontFamily::Proportional) {
        display_fallbacks.extend(prop.iter().cloned());
    }
    fonts.families.insert(display_family(), display_fallbacks);
    ctx.set_fonts(fonts);

    // 2. Estilo visual PIT
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = vec2(10.0, 8.0);
    style.spacing.button_padding = vec2(14.0, 7.0);
    style.spacing.window_margin = egui::Margin::same(16);
    style.spacing.slider_width = 96.0;

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG_VOID;
    visuals.window_fill = BG_SURFACE;
    visuals.extreme_bg_color = Color32::from_rgb(12, 14, 18);
    visuals.faint_bg_color = BG_CARD;
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.selection.bg_fill = pal.accent;
    visuals.selection.stroke = Stroke::new(1.0_f32, pal.accent_ink);

    visuals.widgets.noninteractive.bg_fill = BG_SURFACE;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(10);

    visuals.widgets.inactive.bg_fill = BG_CONTROL;
    visuals.widgets.inactive.weak_bg_fill = BG_CONTROL;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(10);

    visuals.widgets.hovered.bg_fill = BG_CONTROL_HOVER;
    visuals.widgets.hovered.weak_bg_fill = BG_CONTROL_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, WHITE_PURE);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(10);

    visuals.widgets.active.bg_fill = WHITE_PURE;
    visuals.widgets.active.weak_bg_fill = WHITE_PURE;
    visuals.widgets.active.fg_stroke = Stroke::new(1.5_f32, INK_ON_WHITE);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, WHITE_PURE);
    visuals.widgets.active.corner_radius = CornerRadius::same(10);

    visuals.window_corner_radius = CornerRadius::same(16);
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER_STRONG);
    visuals.window_shadow = Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 0,
        color: Color32::from_black_alpha(175),
    };
    visuals.popup_shadow = Shadow {
        offset: [0, 8],
        blur: 20,
        spread: 0,
        color: Color32::from_black_alpha(150),
    };

    style.visuals = visuals;
    ctx.set_style(style);
}

pub fn apply_theme_accent(ctx: &Context, theme: ThemeId) {
    let pal = ThemePalette::for_theme(theme);
    ctx.style_mut(|style| {
        style.visuals.selection.bg_fill = pal.accent;
        style.visuals.selection.stroke = Stroke::new(1.0_f32, pal.accent_ink);
    });
}

pub fn load_window_icon() -> Option<IconData> {
    let img = image::load_from_memory(LOGO_PNG_BYTES).ok()?;
    let resized = img.resize_exact(128, 128, image::imageops::FilterType::Lanczos3);
    let rgba = resized.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some(IconData {
        rgba: rgba.into_raw(),
        width,
        height,
    })
}

pub fn load_logo_texture(ctx: &Context) -> Option<TextureHandle> {
    let img = image::load_from_memory(LOGO_PNG_BYTES).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let color_img = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
    Some(ctx.load_texture("pit-logo", color_img, TextureOptions::LINEAR))
}

pub fn floating_card_shadow() -> Shadow {
    Shadow {
        offset: [0, 10],
        blur: 26,
        spread: 0,
        color: Color32::from_black_alpha(165),
    }
}

pub fn primary_white_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(display_text(text, 14.0).color(INK_ON_WHITE))
        .fill(WHITE_PURE)
        .stroke(Stroke::new(1.0_f32, WHITE_PURE))
        .corner_radius(CornerRadius::same(10))
}

pub fn tactical_accent_button(text: impl Into<String>, pal: ThemePalette) -> egui::Button<'static> {
    egui::Button::new(display_text(text, 14.0).color(pal.accent_ink))
        .fill(pal.accent)
        .stroke(Stroke::new(1.0_f32, pal.accent_bright))
        .corner_radius(CornerRadius::same(10))
}

/// Desenha o logo personalizado PIT em tamanho quadrado exato (sem esticar em containers centralizados).
pub fn draw_logo_badge(ui: &mut Ui, logo: Option<&TextureHandle>, size: f32, framed: bool) {
    let pad = if framed { (size * 0.14).round().max(4.0) } else { 0.0 };
    let outer = size + pad * 2.0;
    let (rect, _) = ui.allocate_exact_size(vec2(outer, outer), Sense::hover());
    let p = ui.painter();

    if framed {
        let radius = CornerRadius::same((outer * 0.26).round() as u8);
        p.rect(
            rect,
            radius,
            BG_CARD,
            Stroke::new(1.0_f32, BORDER_STRONG),
            egui::StrokeKind::Inside,
        );
    }

    let img_rect = rect.shrink(pad);
    if let Some(tex) = logo {
        p.image(
            tex.id(),
            img_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        p.rect_filled(img_rect, CornerRadius::same(6), RED_ACCENT);
        p.text(
            img_rect.center(),
            Align2::CENTER_CENTER,
            "P",
            FontId::new(size * 0.62, display_family()),
            WHITE_PURE,
        );
    }
}

/// Pinta o ícone grande em baixa opacidade centralizado no fundo da tela de sala vazia.
pub fn paint_logo_watermark(
    painter: &egui::Painter,
    stage_rect: Rect,
    logo: Option<&TextureHandle>,
    alpha: u8,
) {
    let Some(tex) = logo else {
        return;
    };
    let side = (stage_rect.width().min(stage_rect.height()) * 0.68).clamp(220.0, 460.0);
    let img_rect = Rect::from_center_size(stage_rect.center(), vec2(side, side));
    painter.image(
        tex.id(),
        img_rect,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::from_rgba_unmultiplied(255, 255, 255, alpha),
    );
}

pub fn draw_status_dot(ui: &mut Ui, color: Color32, radius: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(radius * 2.4, radius * 2.4), Sense::hover());
    let p = ui.painter();
    p.circle_filled(rect.center(), radius + 2.0, color.linear_multiply(0.28));
    p.circle_filled(rect.center(), radius, color);
}

pub fn paint_marathon_backdrop(painter: &egui::Painter, rect: Rect, pal: ThemePalette) {
    let grid_stroke = Stroke::new(1.0_f32, Color32::from_rgb(20, 23, 29));
    let step = 72.0;
    let mut x = rect.left() + step;
    while x < rect.right() {
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], grid_stroke);
        x += step;
    }
    let mut y = rect.top() + step;
    while y < rect.bottom() {
        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], grid_stroke);
        y += step;
    }

    // Faixa tática na cor de acento no topo esquerdo + detalhe neutro no rodapé direito
    let accent_bar = Rect::from_min_size(pos2(rect.left() + 28.0, rect.top() + 24.0), vec2(56.0, 3.0));
    painter.rect_filled(accent_bar, 1.5, pal.accent);

    let white_bar = Rect::from_min_size(
        pos2(rect.right() - 84.0, rect.bottom() - 28.0),
        vec2(56.0, 3.0),
    );
    painter.rect_filled(white_bar, 1.5, Color32::from_rgb(52, 58, 70));

    // Cantoneiras técnicas discretas
    let corner_stroke = Stroke::new(1.5_f32, Color32::from_rgb(46, 51, 63));
    let m = 18.0;
    let len = 14.0;
    let corners = [
        (pos2(rect.left() + m, rect.top() + m), vec2(len, 0.0), vec2(0.0, len)),
        (pos2(rect.right() - m, rect.top() + m), vec2(-len, 0.0), vec2(0.0, len)),
        (pos2(rect.left() + m, rect.bottom() - m), vec2(len, 0.0), vec2(0.0, -len)),
        (pos2(rect.right() - m, rect.bottom() - m), vec2(-len, 0.0), vec2(0.0, -len)),
    ];
    for (o, dx, dy) in corners {
        painter.line_segment([o, o + dx], corner_stroke);
        painter.line_segment([o, o + dy], corner_stroke);
    }
}

// =============================================================================
// Ícones vetoriais limpos (renderizados via egui::Painter)
// =============================================================================
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    Screen,
    Stop,
    People,
    Grid,
    Spotlight,
    Fullscreen,
    ExitFullscreen,
    PopOut,
    DockBack,
    Copy,
    Check,
    Volume,
    VolumeMute,
    Shield,
    ChevronRight,
    Leave,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PillVariant {
    Dark,
    ActiveWhite,
    TacticalAccent,
    GhostAccent,
    Overlay,
}

pub fn icon_pill_button(
    ui: &mut Ui,
    icon: IconKind,
    label: Option<&str>,
    variant: PillVariant,
    pal: ThemePalette,
    height: f32,
) -> Response {
    let icon_size = 15.0;
    let pad_x = if label.is_some() { 12.0 } else { 9.0 };
    let gap = if label.is_some() { 7.0 } else { 0.0 };

    let font_id = match variant {
        PillVariant::ActiveWhite | PillVariant::TacticalAccent => {
            FontId::new(13.0, display_family())
        }
        _ => FontId::new(13.0, FontFamily::Proportional),
    };

    let galley = label.map(|txt| {
        ui.painter()
            .layout_no_wrap(txt.to_string(), font_id.clone(), WHITE_PURE)
    });
    let text_w = galley.as_ref().map(|g| g.size().x).unwrap_or(0.0);
    let total_w = pad_x * 2.0 + icon_size + gap + text_w;

    let (rect, resp) = ui.allocate_exact_size(vec2(total_w, height), Sense::click());
    let hovered = resp.hovered();
    let pressed = resp.is_pointer_button_down_on();

    let (bg, fg, border) = match variant {
        PillVariant::Dark => (
            if pressed {
                BG_CARD
            } else if hovered {
                BG_CONTROL_HOVER
            } else {
                BG_CONTROL
            },
            if hovered { WHITE_PURE } else { TEXT_PRIMARY },
            if hovered { BORDER_STRONG } else { BORDER_SUBTLE },
        ),
        PillVariant::ActiveWhite => (
            if pressed {
                Color32::from_rgb(225, 228, 234)
            } else {
                WHITE_PURE
            },
            INK_ON_WHITE,
            WHITE_PURE,
        ),
        PillVariant::TacticalAccent => (
            if pressed {
                pal.accent.linear_multiply(0.85)
            } else if hovered {
                pal.accent_bright
            } else {
                pal.accent
            },
            pal.accent_ink,
            pal.accent_bright,
        ),
        PillVariant::GhostAccent => (
            if hovered { pal.accent } else { pal.accent_dim },
            if hovered {
                pal.accent_ink
            } else {
                pal.accent_bright
            },
            pal.accent,
        ),
        PillVariant::Overlay => (
            if hovered {
                Color32::from_rgba_premultiplied(36, 40, 50, 238)
            } else {
                BG_OVERLAY
            },
            WHITE_PURE,
            if hovered { WHITE_PURE } else { BORDER_STRONG },
        ),
    };

    let p = ui.painter();
    p.rect(
        rect,
        CornerRadius::same(10),
        bg,
        Stroke::new(1.0_f32, border),
        egui::StrokeKind::Inside,
    );

    let icon_rect = Rect::from_center_size(
        pos2(rect.left() + pad_x + icon_size * 0.5, rect.center().y),
        vec2(icon_size, icon_size),
    );
    paint_icon(p, icon_rect, icon, fg);

    if let Some(txt) = label {
        let galley = p.layout_no_wrap(txt.to_string(), font_id, fg);
        let text_pos = pos2(
            icon_rect.right() + gap,
            rect.center().y - galley.size().y * 0.5,
        );
        p.galley(text_pos, galley, fg);
    }

    resp
}

pub fn paint_icon(p: &egui::Painter, r: Rect, icon: IconKind, color: Color32) {
    let s = Stroke::new(1.6_f32, color);
    let c = r.center();
    let w = r.width();
    let h = r.height();

    match icon {
        IconKind::Screen => {
            let monitor = Rect::from_min_max(
                pos2(r.left() + 1.0, r.top() + 1.5),
                pos2(r.right() - 1.0, r.bottom() - 4.0),
            );
            p.rect_stroke(monitor, 2.0, s, egui::StrokeKind::Inside);
            p.line_segment(
                [pos2(c.x, monitor.bottom()), pos2(c.x, r.bottom() - 1.0)],
                s,
            );
            p.line_segment(
                [
                    pos2(c.x - w * 0.25, r.bottom() - 1.0),
                    pos2(c.x + w * 0.25, r.bottom() - 1.0),
                ],
                s,
            );
        }
        IconKind::Stop => {
            let sq = Rect::from_center_size(c, vec2(w * 0.56, h * 0.56));
            p.rect_filled(sq, 2.0, color);
        }
        IconKind::People => {
            p.circle_stroke(pos2(c.x - 2.0, r.top() + 4.5), 2.7, s);
            p.line_segment(
                [pos2(r.left() + 1.5, r.bottom() - 2.0), pos2(r.left() + 3.0, c.y + 1.5)],
                s,
            );
            p.line_segment(
                [pos2(r.left() + 3.0, c.y + 1.5), pos2(c.x + 2.5, c.y + 1.5)],
                s,
            );
            p.line_segment(
                [pos2(c.x + 2.5, c.y + 1.5), pos2(c.x + 4.0, r.bottom() - 2.0)],
                s,
            );
            p.circle_stroke(pos2(c.x + 4.0, r.top() + 5.0), 2.0, s);
        }
        IconKind::Grid => {
            let gap = 2.0;
            let cell = (w - gap - 2.0) * 0.5;
            for gx in 0..2 {
                for gy in 0..2 {
                    let min = pos2(
                        r.left() + 1.0 + gx as f32 * (cell + gap),
                        r.top() + 1.0 + gy as f32 * (cell + gap),
                    );
                    p.rect_stroke(
                        Rect::from_min_size(min, vec2(cell, cell)),
                        1.5,
                        s,
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
        IconKind::Spotlight => {
            let main = Rect::from_min_max(
                pos2(r.left() + 1.0, r.top() + 1.0),
                pos2(r.right() - 1.0, r.bottom() - 5.0),
            );
            p.rect_stroke(main, 1.5, s, egui::StrokeKind::Inside);
            p.line_segment(
                [pos2(r.left() + 1.5, r.bottom() - 2.0), pos2(c.x - 1.0, r.bottom() - 2.0)],
                s,
            );
            p.line_segment(
                [pos2(c.x + 1.5, r.bottom() - 2.0), pos2(r.right() - 1.5, r.bottom() - 2.0)],
                s,
            );
        }
        IconKind::Fullscreen => {
            let l = 4.2;
            let corners = [
                (pos2(r.left() + 1.5, r.top() + 1.5), vec2(l, 0.0), vec2(0.0, l)),
                (pos2(r.right() - 1.5, r.top() + 1.5), vec2(-l, 0.0), vec2(0.0, l)),
                (pos2(r.left() + 1.5, r.bottom() - 1.5), vec2(l, 0.0), vec2(0.0, -l)),
                (pos2(r.right() - 1.5, r.bottom() - 1.5), vec2(-l, 0.0), vec2(0.0, -l)),
            ];
            for (o, dx, dy) in corners {
                p.line_segment([o, o + dx], s);
                p.line_segment([o, o + dy], s);
            }
        }
        IconKind::ExitFullscreen => {
            let l = 3.8;
            let o1 = pos2(c.x - 2.0, c.y - 2.0);
            let o2 = pos2(c.x + 2.0, c.y - 2.0);
            let o3 = pos2(c.x - 2.0, c.y + 2.0);
            let o4 = pos2(c.x + 2.0, c.y + 2.0);
            p.line_segment([o1, o1 + vec2(-l, 0.0)], s);
            p.line_segment([o1, o1 + vec2(0.0, -l)], s);
            p.line_segment([o2, o2 + vec2(l, 0.0)], s);
            p.line_segment([o2, o2 + vec2(0.0, -l)], s);
            p.line_segment([o3, o3 + vec2(-l, 0.0)], s);
            p.line_segment([o3, o3 + vec2(0.0, l)], s);
            p.line_segment([o4, o4 + vec2(l, 0.0)], s);
            p.line_segment([o4, o4 + vec2(0.0, l)], s);
        }
        IconKind::PopOut => {
            let box_r = Rect::from_min_max(
                pos2(r.left() + 1.5, r.top() + 4.5),
                pos2(r.right() - 4.5, r.bottom() - 1.5),
            );
            p.rect_stroke(box_r, 1.5, s, egui::StrokeKind::Inside);
            let tr = pos2(r.right() - 1.5, r.top() + 1.5);
            p.line_segment([pos2(c.x - 1.0, c.y + 1.0), tr], s);
            p.line_segment([tr, pos2(tr.x - 4.5, tr.y)], s);
            p.line_segment([tr, pos2(tr.x, tr.y + 4.5)], s);
        }
        IconKind::DockBack => {
            let box_r = Rect::from_min_max(
                pos2(r.left() + 1.5, r.top() + 4.5),
                pos2(r.right() - 4.5, r.bottom() - 1.5),
            );
            p.rect_stroke(box_r, 1.5, s, egui::StrokeKind::Inside);
            let target = pos2(c.x - 1.0, c.y + 1.0);
            p.line_segment([pos2(r.right() - 1.5, r.top() + 1.5), target], s);
            p.line_segment([target, pos2(target.x + 4.5, target.y)], s);
            p.line_segment([target, pos2(target.x, target.y - 4.5)], s);
        }
        IconKind::Copy => {
            let back = Rect::from_min_max(
                pos2(r.left() + 4.5, r.top() + 1.5),
                pos2(r.right() - 1.5, r.bottom() - 4.5),
            );
            let front = Rect::from_min_max(
                pos2(r.left() + 1.5, r.top() + 4.5),
                pos2(r.right() - 4.5, r.bottom() - 1.5),
            );
            p.rect_stroke(back, 1.5, s, egui::StrokeKind::Inside);
            p.rect_stroke(front, 1.5, s, egui::StrokeKind::Inside);
        }
        IconKind::Check => {
            p.line_segment(
                [pos2(r.left() + 2.0, c.y + 0.5), pos2(c.x - 1.0, r.bottom() - 3.0)],
                Stroke::new(2.0_f32, color),
            );
            p.line_segment(
                [pos2(c.x - 1.0, r.bottom() - 3.0), pos2(r.right() - 2.0, r.top() + 3.0)],
                Stroke::new(2.0_f32, color),
            );
        }
        IconKind::Volume => {
            p.rect_filled(
                Rect::from_min_max(
                    pos2(r.left() + 1.5, c.y - 2.5),
                    pos2(r.left() + 4.5, c.y + 2.5),
                ),
                1.0,
                color,
            );
            p.line_segment([pos2(r.left() + 4.5, c.y - 2.5), pos2(c.x + 1.0, r.top() + 2.5)], s);
            p.line_segment([pos2(c.x + 1.0, r.top() + 2.5), pos2(c.x + 1.0, r.bottom() - 2.5)], s);
            p.line_segment([pos2(c.x + 1.0, r.bottom() - 2.5), pos2(r.left() + 4.5, c.y + 2.5)], s);
            p.line_segment([pos2(c.x + 4.0, c.y - 2.5), pos2(c.x + 4.0, c.y + 2.5)], s);
            p.line_segment([pos2(r.right() - 1.5, c.y - 4.5), pos2(r.right() - 1.5, c.y + 4.5)], s);
        }
        IconKind::VolumeMute => {
            p.rect_filled(
                Rect::from_min_max(
                    pos2(r.left() + 1.5, c.y - 2.5),
                    pos2(r.left() + 4.5, c.y + 2.5),
                ),
                1.0,
                color,
            );
            p.line_segment([pos2(c.x + 1.5, c.y - 3.0), pos2(r.right() - 1.5, c.y + 3.0)], s);
            p.line_segment([pos2(r.right() - 1.5, c.y - 3.0), pos2(c.x + 1.5, c.y + 3.0)], s);
        }
        IconKind::Shield => {
            let top_l = pos2(r.left() + 2.5, r.top() + 2.5);
            let top_r = pos2(r.right() - 2.5, r.top() + 2.5);
            let mid_l = pos2(r.left() + 2.5, c.y + 1.5);
            let mid_r = pos2(r.right() - 2.5, c.y + 1.5);
            let bot = pos2(c.x, r.bottom() - 1.5);
            p.line_segment([top_l, top_r], s);
            p.line_segment([top_l, mid_l], s);
            p.line_segment([top_r, mid_r], s);
            p.line_segment([mid_l, bot], s);
            p.line_segment([mid_r, bot], s);
        }
        IconKind::ChevronRight => {
            p.line_segment([pos2(c.x - 2.5, r.top() + 3.0), pos2(c.x + 2.5, c.y)], s);
            p.line_segment([pos2(c.x + 2.5, c.y), pos2(c.x - 2.5, r.bottom() - 3.0)], s);
        }
        IconKind::Leave => {
            p.line_segment([pos2(r.left() + 2.0, r.top() + 2.0), pos2(c.x, r.top() + 2.0)], s);
            p.line_segment([pos2(r.left() + 2.0, r.top() + 2.0), pos2(r.left() + 2.0, r.bottom() - 2.0)], s);
            p.line_segment([pos2(r.left() + 2.0, r.bottom() - 2.0), pos2(c.x, r.bottom() - 2.0)], s);
            p.line_segment([pos2(c.x - 1.0, c.y), pos2(r.right() - 2.0, c.y)], s);
            p.line_segment([pos2(r.right() - 5.0, c.y - 3.0), pos2(r.right() - 2.0, c.y)], s);
            p.line_segment([pos2(r.right() - 5.0, c.y + 3.0), pos2(r.right() - 2.0, c.y)], s);
        }
    }
}

pub fn render_admin_warning(ui: &mut Ui, elevated: bool) {
    if !elevated {
        return;
    }
    egui::Frame::NONE
        .fill(Color32::from_rgb(56, 42, 16))
        .stroke(Stroke::new(1.0_f32, ACCENT_WARN))
        .corner_radius(10.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.colored_label(
                Color32::from_rgb(255, 232, 175),
                "Atenção: o PIT está rodando como administrador. O Windows pode bloquear a captura de tela — abra normalmente.",
            );
        });
}
