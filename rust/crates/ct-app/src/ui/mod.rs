mod home;
mod room;
mod share_dialog;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;

use ct_core::protocol::{ResolutionId, RoomMode, fps_from_quality};
use ct_media::audio::AudioApp;
use ct_media::screen::ScreenSource;
use ct_media::session::{Phase, RoomSession, SessionSnapshot};
use egui::{
    Color32, ColorImage, Context, CornerRadius, Stroke, TextureHandle, TextureOptions,
    ViewportCommand, Visuals,
};
use tokio::runtime::Runtime;
use tokio::sync::watch;

// Paleta escura moderna, mais clara e confortável aos olhos
pub const BG_CANVAS: Color32 = Color32::from_rgb(31, 36, 48);
pub const BG_PANEL: Color32 = Color32::from_rgb(39, 45, 60);
pub const BG_CARD: Color32 = Color32::from_rgb(47, 54, 72);
pub const BG_CONTROL: Color32 = Color32::from_rgb(56, 65, 86);
pub const BG_CONTROL_HOVER: Color32 = Color32::from_rgb(68, 79, 104);
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(72, 82, 106);
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(240, 243, 250);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(168, 178, 200);
pub const ACCENT_PRIMARY: Color32 = Color32::from_rgb(92, 124, 250);
pub const ACCENT_GREEN: Color32 = Color32::from_rgb(52, 199, 123);
pub const ACCENT_WARN: Color32 = Color32::from_rgb(245, 185, 66);
pub const ACCENT_DANGER: Color32 = Color32::from_rgb(229, 83, 83);

pub fn configure_theme(ctx: &Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(16);

    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG_CANVAS;
    visuals.window_fill = BG_PANEL;
    visuals.extreme_bg_color = Color32::from_rgb(24, 28, 38);
    visuals.faint_bg_color = BG_CARD;
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.selection.bg_fill = ACCENT_PRIMARY;
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);

    visuals.widgets.noninteractive.bg_fill = BG_PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);

    visuals.widgets.inactive.bg_fill = BG_CONTROL;
    visuals.widgets.inactive.weak_bg_fill = BG_CONTROL;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(8);

    visuals.widgets.hovered.bg_fill = BG_CONTROL_HOVER;
    visuals.widgets.hovered.weak_bg_fill = BG_CONTROL_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT_PRIMARY);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(8);

    visuals.widgets.active.bg_fill = ACCENT_PRIMARY;
    visuals.widgets.active.weak_bg_fill = ACCENT_PRIMARY;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.active.corner_radius = CornerRadius::same(8);

    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);

    style.visuals = visuals;
    ctx.set_style(style);
}

pub struct CompartilharApp {
    pub rt: Arc<Runtime>,
    pub session: Arc<RoomSession>,
    pub snap_rx: watch::Receiver<SessionSnapshot>,
    pub snap: SessionSnapshot,

    // Home UI state
    pub nickname_input: String,
    pub code_input: String,
    pub create_mode: RoomMode,

    // Room UI state
    pub focused_id: Option<String>,
    pub grid_mode: bool,
    pub copied_until: Option<Instant>,
    pub show_share_dialog: bool,
    pub show_audio_popover: bool,
    pub closing_room: bool,
    /// Identidades de transmissões abertas em janelas separadas (pop-out).
    pub popped_out: HashSet<String>,

    // ShareDialog state (qualidade e FPS independentes)
    pub screen_sources: Vec<ScreenSource>,
    pub selected_source_idx: usize,
    pub selected_resolution: ResolutionId,
    pub selected_fps: u32,
    pub audio_apps: Arc<StdMutex<Vec<AudioApp>>>,

    // Texturas GPU atualizadas in-place (identity -> (last_seq, TextureHandle))
    pub textures: HashMap<String, (u64, TextureHandle)>,
}

impl CompartilharApp {
    pub fn new(rt: Arc<Runtime>, session: Arc<RoomSession>) -> Self {
        let snap_rx = session.subscribe();
        let snap = snap_rx.borrow().clone();
        let nickname_input = snap.settings.nickname.clone();
        let selected_resolution = ResolutionId::from_quality(snap.settings.quality);
        let selected_fps = fps_from_quality(snap.settings.quality);
        Self {
            rt,
            session,
            snap_rx,
            snap,
            nickname_input,
            code_input: String::new(),
            create_mode: RoomMode::Iroh,
            focused_id: None,
            grid_mode: false,
            copied_until: None,
            show_share_dialog: false,
            show_audio_popover: false,
            closing_room: false,
            popped_out: HashSet::new(),
            screen_sources: Vec::new(),
            selected_source_idx: 0,
            selected_resolution,
            selected_fps,
            audio_apps: Arc::new(StdMutex::new(Vec::new())),
            textures: HashMap::new(),
        }
    }

    pub fn refresh_audio_apps_async(&self, ctx: &Context) {
        let session = self.session.clone();
        let blocked = self.snap.settings.blocked_apps.clone();
        let target = self.audio_apps.clone();
        let ctx_c = ctx.clone();
        self.rt.spawn(async move {
            let list = session.list_audio_apps(&blocked).await;
            if let Ok(mut guard) = target.lock() {
                *guard = list;
            }
            ctx_c.request_repaint();
        });
    }

    pub fn open_share_dialog(&mut self, ctx: &Context) {
        self.screen_sources = self.session.list_screen_sources();
        if self.selected_source_idx >= self.screen_sources.len() {
            self.selected_source_idx = 0;
        }
        self.refresh_audio_apps_async(ctx);
        self.show_share_dialog = true;
    }

    fn sync_textures(&mut self, ctx: &Context) {
        for stream in &self.snap.streams {
            let Some(frame) = self.session.latest_frame(&stream.identity) else {
                continue;
            };
            let needs_upload = self
                .textures
                .get(&stream.identity)
                .is_none_or(|(seq, _)| *seq != frame.seq);
            if !needs_upload {
                continue;
            }
            let image = ColorImage::from_rgba_unmultiplied(
                [frame.width as usize, frame.height as usize],
                &frame.rgba,
            );
            if let Some((seq, handle)) = self.textures.get_mut(&stream.identity) {
                handle.set(image, TextureOptions::LINEAR);
                *seq = frame.seq;
            } else {
                let handle = ctx.load_texture(
                    format!("stream-{}", stream.identity),
                    image,
                    TextureOptions::LINEAR,
                );
                self.textures
                    .insert(stream.identity.clone(), (frame.seq, handle));
            }
        }
        self.textures
            .retain(|id, _| self.snap.streams.iter().any(|s| &s.identity == id));
        self.popped_out
            .retain(|id| self.snap.streams.iter().any(|s| &s.identity == id));
    }
}

impl eframe::App for CompartilharApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if self.snap_rx.has_changed().unwrap_or(false) {
            self.snap = self.snap_rx.borrow_and_update().clone();
        }
        self.sync_textures(ctx);

        if ctx.input(|i| i.viewport().close_requested())
            && self.snap.phase != Phase::Idle
            && !self.closing_room
        {
            self.closing_room = true;
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            let session = self.session.clone();
            let ctx_clone = ctx.clone();
            self.rt.spawn(async move {
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(6),
                    session.leave_room(),
                )
                .await;
                ctx_clone.send_viewport_cmd(ViewportCommand::Close);
            });
        }

        match self.snap.phase {
            Phase::Idle | Phase::Connecting => {
                self.popped_out.clear();
                home::render_home(self, ctx);
            }
            Phase::Connected | Phase::Migrating => room::render_room(self, ctx),
        }

        if self.snap.phase == Phase::Connecting {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }
}

pub fn primary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.into()).color(Color32::WHITE).strong())
        .fill(ACCENT_PRIMARY)
        .stroke(Stroke::NONE)
}

pub fn danger_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.into()).color(Color32::WHITE).strong())
        .fill(ACCENT_DANGER)
        .stroke(Stroke::NONE)
}

pub fn render_admin_warning(ui: &mut egui::Ui, elevated: bool) {
    if !elevated {
        return;
    }
    egui::Frame::NONE
        .fill(Color32::from_rgb(96, 68, 24))
        .stroke(Stroke::new(1.0_f32, ACCENT_WARN))
        .corner_radius(8.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.colored_label(
                Color32::from_rgb(255, 232, 170),
                "[!] O app está rodando como administrador. O Windows pode bloquear a captura de tela — abra normalmente.",
            );
        });
}
