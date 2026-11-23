mod home;
mod room;
mod share_dialog;
pub mod theme;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;

use ct_core::protocol::{AudioQualityId, ResolutionId};
use ct_core::settings::ThemeId;
use ct_media::audio::AudioApp;
use ct_media::screen::{GpuAdapterInfo, ScreenSource};
use ct_media::session::{Phase, RoomSession, SessionSnapshot};
use egui::{
    ColorImage, Context, IconData, TextureHandle, TextureOptions, ViewportCommand,
};
use tokio::runtime::Runtime;
use tokio::sync::watch;

pub struct CompartilharApp {
    pub rt: Arc<Runtime>,
    pub session: Arc<RoomSession>,
    pub snap_rx: watch::Receiver<SessionSnapshot>,
    pub snap: SessionSnapshot,

    // Identidade visual (ícone da janela/pop-out, textura do logo e tema ativo)
    pub app_icon: Option<IconData>,
    pub logo_texture: Option<TextureHandle>,
    pub active_theme: ThemeId,

    // Home UI state
    pub nickname_input: String,
    pub code_input: String,

    // Room UI state
    pub focused_id: Option<String>,
    pub grid_mode: bool,
    pub show_participants: bool,
    pub copied_until: Option<Instant>,
    pub show_share_dialog: bool,
    pub show_audio_popover: bool,
    pub closing_room: bool,
    /// Transmissão em tela cheia na janela principal.
    pub fullscreen_stream_id: Option<String>,
    /// Identidades de transmissões abertas em janelas separadas (pop-out).
    pub popped_out: HashSet<String>,
    /// Janelas pop-out atualmente em tela cheia.
    pub popout_fullscreen: HashSet<String>,

    // ShareDialog state (qualidade de vídeo, FPS, qualidade de áudio e GPU independentes)
    pub screen_sources: Vec<ScreenSource>,
    pub selected_source_idx: usize,
    pub gpu_adapters: Vec<GpuAdapterInfo>,
    pub selected_gpu_idx: usize,
    pub selected_resolution: ResolutionId,
    pub selected_fps: u32,
    pub selected_audio_quality: AudioQualityId,
    pub audio_apps: Arc<StdMutex<Vec<AudioApp>>>,

    // Texturas GPU atualizadas in-place (identity -> (last_seq, TextureHandle))
    pub textures: HashMap<String, (u64, TextureHandle)>,
}

impl CompartilharApp {
    pub fn new(
        ctx: &Context,
        rt: Arc<Runtime>,
        session: Arc<RoomSession>,
        app_icon: Option<IconData>,
    ) -> Self {
        let snap_rx = session.subscribe();
        let snap = snap_rx.borrow().clone();
        let nickname_input = snap.settings.nickname.clone();
        let selected_resolution = ResolutionId::from_quality(snap.settings.quality);
        let selected_fps = snap.settings.fps;
        let selected_audio_quality = snap.settings.audio_quality;
        let active_theme = snap.settings.theme;
        let gpu_adapters = session.list_gpu_adapters();
        let selected_gpu_idx = gpu_adapters
            .iter()
            .position(|g| g.index == snap.settings.gpu_adapter_index)
            .unwrap_or(0);
        let logo_texture = theme::load_logo_texture(ctx);

        Self {
            rt,
            session,
            snap_rx,
            snap,
            app_icon,
            logo_texture,
            active_theme,
            nickname_input,
            code_input: String::new(),
            focused_id: None,
            grid_mode: false,
            show_participants: true,
            copied_until: None,
            show_share_dialog: false,
            show_audio_popover: false,
            closing_room: false,
            fullscreen_stream_id: None,
            popped_out: HashSet::new(),
            popout_fullscreen: HashSet::new(),
            screen_sources: Vec::new(),
            selected_source_idx: 0,
            gpu_adapters,
            selected_gpu_idx,
            selected_resolution,
            selected_fps,
            selected_audio_quality,
            audio_apps: Arc::new(StdMutex::new(Vec::new())),
            textures: HashMap::new(),
        }
    }

    pub fn palette(&self) -> theme::ThemePalette {
        theme::ThemePalette::for_theme(self.active_theme)
    }

    pub fn set_theme(&mut self, ctx: &Context, new_theme: ThemeId) {
        if self.active_theme == new_theme {
            return;
        }
        self.active_theme = new_theme;
        theme::apply_theme_accent(ctx, new_theme);
        let session = self.session.clone();
        self.rt.spawn(async move {
            session.update_settings(|s| s.theme = new_theme).await;
        });
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
        self.gpu_adapters = self.session.list_gpu_adapters();
        self.selected_gpu_idx = self
            .gpu_adapters
            .iter()
            .position(|g| g.index == self.snap.settings.gpu_adapter_index)
            .unwrap_or(0);
        self.refresh_audio_apps_async(ctx);
        self.show_share_dialog = true;
    }

    pub fn enter_main_fullscreen(&mut self, ctx: &Context, stream_id: String) {
        self.fullscreen_stream_id = Some(stream_id);
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
    }

    pub fn exit_main_fullscreen(&mut self, ctx: &Context) {
        if self.fullscreen_stream_id.take().is_some() {
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
        }
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
        self.popout_fullscreen
            .retain(|id| self.popped_out.contains(id));
        if let Some(ref fs_id) = self.fullscreen_stream_id
            && !self.snap.streams.iter().any(|s| &s.identity == fs_id)
        {
            self.exit_main_fullscreen(ctx);
        }
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
                    std::time::Duration::from_secs(7),
                    session.leave_room(),
                )
                .await;
                ctx_clone.send_viewport_cmd(ViewportCommand::Close);
            });
        }

        match self.snap.phase {
            Phase::Idle | Phase::Connecting => {
                self.exit_main_fullscreen(ctx);
                self.popped_out.clear();
                self.popout_fullscreen.clear();
                home::render_home(self, ctx);
            }
            Phase::Connected | Phase::Migrating => room::render_room(self, ctx),
        }

        if self.snap.phase == Phase::Connecting || self.snap.phase == Phase::Migrating {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }
}
