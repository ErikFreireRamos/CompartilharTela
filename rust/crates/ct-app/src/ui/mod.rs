mod home;
mod room;
mod share_dialog;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use ct_core::protocol::RoomMode;
use ct_media::audio::AudioApp;
use ct_media::screen::ScreenSource;
use ct_media::session::{Phase, RoomSession, SessionSnapshot};
use egui::{Color32, ColorImage, Context, TextureHandle, TextureOptions, ViewportCommand, Visuals};
use tokio::runtime::Runtime;
use tokio::sync::watch;

pub fn configure_theme(ctx: &Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(16, 18, 22);
    visuals.window_fill = Color32::from_rgb(24, 27, 34);
    visuals.extreme_bg_color = Color32::from_rgb(12, 14, 18);
    ctx.set_visuals(visuals);
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

    // ShareDialog state
    pub screen_sources: Vec<ScreenSource>,
    pub selected_source_idx: usize,
    pub audio_apps: Vec<AudioApp>,

    // GPU textures for active video streams (identity -> (last_seq, TextureHandle))
    pub textures: HashMap<String, (u64, TextureHandle)>,
}

impl CompartilharApp {
    pub fn new(rt: Arc<Runtime>, session: Arc<RoomSession>) -> Self {
        let snap_rx = session.subscribe();
        let snap = snap_rx.borrow().clone();
        let nickname_input = snap.settings.nickname.clone();
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
            screen_sources: Vec::new(),
            selected_source_idx: 0,
            audio_apps: Vec::new(),
            textures: HashMap::new(),
        }
    }

    pub fn open_share_dialog(&mut self) {
        self.screen_sources = self.session.list_screen_sources();
        self.selected_source_idx = 0;
        let blocked = self.snap.settings.blocked_apps.clone();
        self.audio_apps = self.rt.block_on(self.session.list_audio_apps(&blocked));
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
        // Remove texturas de streams encerrados
        self.textures
            .retain(|id, _| self.snap.streams.iter().any(|s| &s.identity == id));
    }
}

impl eframe::App for CompartilharApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if self.snap_rx.has_changed().unwrap_or(false) {
            self.snap = self.snap_rx.borrow_and_update().clone();
        }
        self.sync_textures(ctx);

        // Intercepta fechamento enquanto está numa sala para passar o host antes de sair
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
                    std::time::Duration::from_secs(8),
                    session.leave_room(),
                )
                .await;
                ctx_clone.send_viewport_cmd(ViewportCommand::Close);
            });
        }

        match self.snap.phase {
            Phase::Idle | Phase::Connecting => home::render_home(self, ctx),
            Phase::Connected | Phase::Migrating => room::render_room(self, ctx),
        }

        if self.snap.phase == Phase::Connecting {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

pub fn render_admin_warning(ui: &mut egui::Ui, elevated: bool) {
    if !elevated {
        return;
    }
    egui::Frame::NONE
        .fill(Color32::from_rgb(90, 58, 18))
        .corner_radius(6.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.colored_label(
                Color32::from_rgb(255, 224, 150),
                "[!] O app está rodando como administrador. O Windows bloqueia a captura de tela nesse modo — feche e abra normalmente.",
            );
        });
}
