//! Máquina de estados da sala (`RoomSession`) 100% P2P via Iroh:
//!  - Criação/entrada com código `IROH-...` (sem abrir portas / funciona com CGNAT).
//!  - Troca automática de host quando o host sai da sala (`Handoff` -> `NewHost`).
//!  - Alteração ao vivo de resolução, FPS, qualidade de áudio, volume e apps bloqueados.
//!  - Escolha de GPU de captura quando há mais de 1 GPU disponível.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ct_core::adpcm::{encode_pcm16_stereo, encode_stereo_f32, resample_stereo_f32};
use ct_core::protocol::{
    AUDIO_CODEC_ADPCM, AUDIO_CODEC_PCM16, AudioPacketHeader, AudioQualityId, ContentType,
    ResolutionId, RoomMeta, RoomScope, encode_audio_packet, encode_video_packet,
};
use ct_core::room_code::generate_room_secret;
use ct_core::settings::{Settings, SettingsStore};
use tokio::sync::{Mutex, RwLock, mpsc, watch};
use tokio::task::JoinHandle;

use crate::audio::{AUDIO_SAMPLE_RATE, AudioApp, AudioCaptureEngine, AudioMode, AudioPlaybackMixer};
use crate::is_elevated;
use crate::p2p::{IrohRoomHost, IrohRoomTicket, RoomTransport, TransportEvent};
use crate::screen::{
    CapturedVideoFrame, DeltaVideoEncoder, GpuAdapterInfo, LiveCaptureConfig, ScreenCapturer,
    ScreenSource, list_gpu_adapters, list_screen_sources,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Connecting,
    Connected,
    Migrating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveQuality {
    Auto,
    Low,
    Off,
}

#[derive(Debug, Clone)]
pub struct JoinInfo {
    pub identity: String,
    pub code: String,
    pub secret: u32,
    pub is_host: bool,
    pub scope: RoomScope,
}

#[derive(Debug, Clone)]
pub struct PeerView {
    pub identity: String,
    pub name: String,
    pub is_local: bool,
    pub is_host: bool,
    pub can_host: Option<bool>,
    pub sharing: bool,
}

#[derive(Debug, Clone)]
pub struct StreamView {
    pub identity: String,
    pub name: String,
    pub is_local: bool,
    pub has_audio: bool,
    pub receive: ReceiveQuality,
}

#[derive(Debug, Clone)]
pub struct ShareOptions {
    pub source_id: String,
    pub resolution: ResolutionId,
    pub fps: u32,
    pub audio_quality: AudioQualityId,
    pub gpu_adapter_index: Option<u32>,
    pub content_type: ContentType,
    pub audio: bool,
    pub blocked: Vec<String>,
    pub send_volume: f32,
}

#[derive(Debug, Clone)]
pub struct ActiveShare {
    pub resolution: ResolutionId,
    pub fps: u32,
    pub audio_quality: AudioQualityId,
    pub content_type: ContentType,
    pub audio_mode: AudioMode,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionSnapshot {
    pub phase: Phase,
    pub notice: Option<String>,
    pub error: Option<String>,
    pub elevated: bool,
    pub info: Option<JoinInfo>,
    pub meta: Option<RoomMeta>,
    pub settings: Settings,
    pub share: Option<ActiveShare>,
    pub peers: Vec<PeerView>,
    pub streams: Vec<StreamView>,
}

impl SessionSnapshot {
    pub fn is_host(&self) -> bool {
        self.info.as_ref().is_some_and(|i| i.is_host)
    }

    pub fn code(&self) -> &str {
        self.meta
            .as_ref()
            .map(|m| m.code.as_str())
            .or_else(|| self.info.as_ref().map(|i| i.code.as_str()))
            .unwrap_or("")
    }

    pub fn volume_of(&self, name: &str) -> f32 {
        self.settings.volumes.get(name).copied().unwrap_or(1.0)
    }
}

pub struct RoomSession {
    settings: Mutex<SettingsStore>,
    transport: Mutex<Option<Arc<RoomTransport>>>,
    event_pump: std::sync::Mutex<Option<JoinHandle<()>>>,
    video_pump: Mutex<Option<JoinHandle<()>>>,
    audio_pump: Mutex<Option<JoinHandle<()>>>,
    audio: Mutex<AudioCaptureEngine>,
    playback: AudioPlaybackMixer,
    capturer: Mutex<Option<ScreenCapturer>>,
    live_audio_quality: RwLock<AudioQualityId>,
    force_keyframe: Arc<AtomicBool>,
    migrating: AtomicBool,
    leaving: AtomicBool,
    receive_modes: Mutex<HashMap<String, ReceiveQuality>>,
    frames: std::sync::RwLock<HashMap<String, CapturedVideoFrame>>,
    repaint_cb: std::sync::RwLock<Option<Arc<dyn Fn() + Send + Sync>>>,
    snapshot_tx: watch::Sender<SessionSnapshot>,
    snapshot_rx: watch::Receiver<SessionSnapshot>,
}

impl RoomSession {
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        let store = SettingsStore::load(data_dir.join("settings.json"));
        let settings_clone = store.get().clone();
        let init_audio_q = settings_clone.audio_quality;

        let initial = SessionSnapshot {
            phase: Phase::Idle,
            notice: None,
            error: None,
            elevated: is_elevated(),
            info: None,
            meta: None,
            settings: settings_clone,
            share: None,
            peers: Vec::new(),
            streams: Vec::new(),
        };
        let (tx, rx) = watch::channel(initial);

        Arc::new(Self {
            settings: Mutex::new(store),
            transport: Mutex::new(None),
            event_pump: std::sync::Mutex::new(None),
            video_pump: Mutex::new(None),
            audio_pump: Mutex::new(None),
            audio: Mutex::new(AudioCaptureEngine::new()),
            playback: AudioPlaybackMixer::new(),
            capturer: Mutex::new(None),
            live_audio_quality: RwLock::new(init_audio_q),
            force_keyframe: Arc::new(AtomicBool::new(true)),
            migrating: AtomicBool::new(false),
            leaving: AtomicBool::new(false),
            receive_modes: Mutex::new(HashMap::new()),
            frames: std::sync::RwLock::new(HashMap::new()),
            repaint_cb: std::sync::RwLock::new(None),
            snapshot_tx: tx,
            snapshot_rx: rx,
        })
    }

    pub fn set_repaint_callback(&self, cb: Arc<dyn Fn() + Send + Sync>) {
        if let Ok(mut guard) = self.repaint_cb.write() {
            *guard = Some(cb);
        }
    }

    fn request_repaint(&self) {
        if let Ok(guard) = self.repaint_cb.read()
            && let Some(ref cb) = *guard
        {
            cb();
        }
    }

    pub fn latest_frame(&self, identity: &str) -> Option<CapturedVideoFrame> {
        self.frames
            .read()
            .ok()
            .and_then(|map| map.get(identity).cloned())
    }

    pub fn subscribe(&self) -> watch::Receiver<SessionSnapshot> {
        self.snapshot_rx.clone()
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        self.snapshot_rx.borrow().clone()
    }

    pub fn list_screen_sources(&self) -> Vec<ScreenSource> {
        list_screen_sources()
    }

    pub fn list_gpu_adapters(&self) -> Vec<GpuAdapterInfo> {
        list_gpu_adapters()
    }

    pub async fn list_audio_apps(&self, blocked: &[String]) -> Vec<AudioApp> {
        let blocked_owned = blocked.to_vec();
        tokio::task::spawn_blocking(move || {
            let tmp = AudioCaptureEngine::new();
            tmp.list_apps(&blocked_owned)
        })
        .await
        .unwrap_or_default()
    }

    pub async fn update_settings<F>(&self, mutate: F)
    where
        F: FnOnce(&mut Settings),
    {
        let updated = self.settings.lock().await.update(mutate).clone();
        self.snapshot_tx.send_modify(|s| s.settings = updated);
        self.request_repaint();
    }

    pub async fn set_blocked_apps(&self, blocked: Vec<String>) {
        self.audio.lock().await.set_blocked(&blocked).await;
        self.update_settings(|s| s.blocked_apps = blocked).await;
    }

    pub async fn set_send_volume(&self, volume: f32) {
        self.audio.lock().await.set_send_volume(volume);
        self.update_settings(|s| s.send_volume = volume).await;
    }

    pub async fn set_audio_quality(&self, quality: AudioQualityId) {
        *self.live_audio_quality.write().await = quality;
        self.update_settings(|s| s.audio_quality = quality).await;
        self.snapshot_tx.send_modify(|s| {
            if let Some(ref mut share) = s.share {
                share.audio_quality = quality;
            }
        });
        self.request_repaint();
    }

    pub async fn set_peer_volume(&self, identity: String, name: String, volume: f32) {
        let v = volume.clamp(0.0, 2.0);
        self.playback.set_peer_volume(&identity, v);
        self.update_settings(|s| {
            s.volumes.insert(name, v);
        })
        .await;
    }

    pub async fn set_receive_quality(&self, identity: String, mode: ReceiveQuality) {
        self.receive_modes
            .lock()
            .await
            .insert(identity.clone(), mode);
        if mode == ReceiveQuality::Off {
            self.playback.remove_peer(&identity);
        }
        self.snapshot_tx.send_modify(|s| {
            if let Some(stream) = s.streams.iter_mut().find(|st| st.identity == identity) {
                stream.receive = mode;
            }
        });
        self.request_repaint();
    }

    /// Altera resolução e/ou FPS ao vivo durante a transmissão sem interromper o compartilhamento.
    pub async fn update_share_quality(&self, resolution: ResolutionId, fps: u32) {
        let fps = fps.clamp(15, 60);
        if let Some(ref cap) = *self.capturer.lock().await {
            cap.update_config(LiveCaptureConfig::from_res_fps(resolution, fps));
        }
        self.force_keyframe.store(true, Ordering::SeqCst);
        let qid = resolution.to_quality_id(fps);
        self.update_settings(|s| {
            s.quality = qid;
            s.fps = fps;
        })
        .await;
        self.snapshot_tx.send_modify(|s| {
            if let Some(ref mut share) = s.share {
                share.resolution = resolution;
                share.fps = fps;
            }
        });
        self.request_repaint();
    }

    pub async fn create_room(self: &Arc<Self>, nickname: String) {
        self.leaving.store(false, Ordering::SeqCst);
        self.migrating.store(false, Ordering::SeqCst);
        self.snapshot_tx.send_modify(|s| {
            s.phase = Phase::Connecting;
            s.error = None;
            s.notice = None;
        });
        self.request_repaint();

        let secret = generate_room_secret();
        let (ev_tx, ev_rx) = mpsc::channel(128);
        let client_id = self.settings.lock().await.get().client_id.clone();

        match IrohRoomHost::start(secret, 1, client_id.clone(), nickname.clone(), ev_tx).await {
            Ok(iroh_host) => {
                let ticket = iroh_host.ticket.clone();
                let meta = iroh_host.meta.clone();
                let info = JoinInfo {
                    identity: client_id.clone(),
                    code: ticket,
                    secret,
                    is_host: true,
                    scope: RoomScope::Internet,
                };
                let local_peer = PeerView {
                    identity: client_id,
                    name: nickname,
                    is_local: true,
                    is_host: true,
                    can_host: Some(true),
                    sharing: false,
                };
                *self.transport.lock().await = Some(Arc::new(RoomTransport::Host(iroh_host)));
                self.spawn_event_pump(ev_rx);

                self.snapshot_tx.send_modify(|s| {
                    s.phase = Phase::Connected;
                    s.info = Some(info);
                    s.meta = Some(meta);
                    s.peers = vec![local_peer];
                });
                self.request_repaint();
            }
            Err(err) => {
                self.reset_with_error(Some(err)).await;
            }
        }
    }

    pub async fn join_room(self: &Arc<Self>, code: String, nickname: String) {
        self.leaving.store(false, Ordering::SeqCst);
        self.migrating.store(false, Ordering::SeqCst);
        self.snapshot_tx.send_modify(|s| {
            s.phase = Phase::Connecting;
            s.error = None;
            s.notice = None;
        });
        self.request_repaint();

        let ticket = match IrohRoomTicket::decode(&code) {
            Ok(t) => t,
            Err(e) => {
                self.reset_with_error(Some(e)).await;
                return;
            }
        };
        let client_id = self.settings.lock().await.get().client_id.clone();
        let secret = ticket.secret;
        let (ev_tx, ev_rx) = mpsc::channel(128);

        match RoomTransport::connect_client(ticket, client_id.clone(), nickname.clone(), ev_tx)
            .await
        {
            Ok(tr) => {
                *self.transport.lock().await = Some(Arc::new(tr));
                self.spawn_event_pump(ev_rx);
                let info = JoinInfo {
                    identity: client_id.clone(),
                    code: code.trim().to_string(),
                    secret,
                    is_host: false,
                    scope: RoomScope::Internet,
                };
                let local_peer = PeerView {
                    identity: client_id,
                    name: nickname,
                    is_local: true,
                    is_host: false,
                    can_host: Some(true),
                    sharing: false,
                };
                self.snapshot_tx.send_modify(|s| {
                    s.phase = Phase::Connected;
                    s.info = Some(info);
                    s.peers = vec![local_peer];
                });
                self.request_repaint();
            }
            Err(err) => {
                self.reset_with_error(Some(err)).await;
            }
        }
    }

    fn spawn_event_pump(self: &Arc<Self>, mut ev_rx: mpsc::Receiver<TransportEvent>) {
        if let Ok(mut guard) = self.event_pump.lock()
            && let Some(old) = guard.take()
        {
            old.abort();
        }
        let weak = Arc::downgrade(self);
        let handle = tokio::spawn(async move {
            while let Some(ev) = ev_rx.recv().await {
                let Some(session) = weak.upgrade() else {
                    break;
                };
                match ev {
                    TransportEvent::RoomState { meta, peers } => {
                        let my_id = session.settings.lock().await.get().client_id.clone();
                        let recv_map = session.receive_modes.lock().await.clone();
                        let active_sharers: std::collections::HashSet<&str> = peers
                            .iter()
                            .filter(|p| p.sharing)
                            .map(|p| p.identity.as_str())
                            .collect();

                        // Limpa frames de quem parou de compartilhar
                        if let Ok(mut map) = session.frames.write() {
                            map.retain(|id, _| active_sharers.contains(id.as_str()));
                        }

                        let peer_views: Vec<PeerView> = peers
                            .iter()
                            .map(|p| PeerView {
                                identity: p.identity.clone(),
                                name: p.name.clone(),
                                is_local: p.identity == my_id,
                                is_host: p.is_host,
                                can_host: p.can_host,
                                sharing: p.sharing,
                            })
                            .collect();
                        let stream_views: Vec<StreamView> = peers
                            .iter()
                            .filter(|p| p.sharing)
                            .map(|p| StreamView {
                                identity: p.identity.clone(),
                                name: p.name.clone(),
                                is_local: p.identity == my_id,
                                has_audio: p.has_audio,
                                receive: recv_map
                                    .get(&p.identity)
                                    .copied()
                                    .unwrap_or(ReceiveQuality::Auto),
                            })
                            .collect();

                        session.snapshot_tx.send_modify(|s| {
                            s.meta = Some(meta);
                            s.peers = peer_views;
                            s.streams = stream_views;
                        });
                        session.request_repaint();
                    }
                    TransportEvent::VideoFrame { identity, frame } => {
                        let is_off = session
                            .receive_modes
                            .lock()
                            .await
                            .get(&identity)
                            .copied()
                            == Some(ReceiveQuality::Off);
                        if !is_off {
                            if let Ok(mut map) = session.frames.write() {
                                map.insert(identity, frame);
                            }
                            session.request_repaint();
                        }
                    }
                    TransportEvent::AudioSamples { identity, samples } => {
                        let is_off = session
                            .receive_modes
                            .lock()
                            .await
                            .get(&identity)
                            .copied()
                            == Some(ReceiveQuality::Off);
                        if !is_off {
                            let snap = session.snapshot();
                            let peer_name = snap
                                .peers
                                .iter()
                                .find(|p| p.identity == identity)
                                .map(|p| p.name.clone())
                                .unwrap_or_else(|| identity.clone());
                            let vol = snap.volume_of(&peer_name);
                            session.playback.push_samples(&identity, &samples, vol);
                        }
                    }
                    TransportEvent::RequestKeyframe => {
                        session.force_keyframe.store(true, Ordering::SeqCst);
                    }
                    TransportEvent::Handoff { to, epoch } => {
                        let my_id = session.settings.lock().await.get().client_id.clone();
                        if to == my_id {
                            let sess = session.clone();
                            tokio::spawn(async move {
                                sess.take_over_as_host(epoch).await;
                            });
                        } else {
                            session.snapshot_tx.send_modify(|s| {
                                s.notice = Some("O host está saindo — transferindo a sala…".to_string());
                            });
                            session.request_repaint();
                        }
                    }
                    TransportEvent::NewHost {
                        host_id,
                        host_name,
                        ticket,
                        ..
                    } => {
                        let my_id = session.settings.lock().await.get().client_id.clone();
                        if host_id != my_id && !session.migrating.load(Ordering::SeqCst) {
                            let sess = session.clone();
                            tokio::spawn(async move {
                                sess.migrate_to_new_host(host_name, ticket).await;
                            });
                            break;
                        }
                    }
                    TransportEvent::RoomEnd { reason } => {
                        if !session.migrating.load(Ordering::SeqCst) {
                            session.reset_with_error(Some(reason)).await;
                        }
                        break;
                    }
                    TransportEvent::Disconnected => {
                        if !session.migrating.load(Ordering::SeqCst)
                            && !session.leaving.load(Ordering::SeqCst)
                            && session.snapshot().phase != Phase::Idle
                        {
                            session
                                .reset_with_error(Some(
                                    "Conexão com o host encerrada (o host saiu da sala)."
                                        .to_string(),
                                ))
                                .await;
                        }
                        break;
                    }
                }
            }
        });
        if let Ok(mut guard) = self.event_pump.lock() {
            *guard = Some(handle);
        }
    }

    /// Assume automaticamente como o novo host da sala quando o host anterior sai.
    async fn take_over_as_host(self: &Arc<Self>, epoch: u64) {
        if self.migrating.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(old_info) = self.snapshot().info else {
            self.migrating.store(false, Ordering::SeqCst);
            return;
        };
        let (client_id, nickname) = {
            let st = self.settings.lock().await;
            let s = st.get();
            (s.client_id.clone(), s.nickname.clone())
        };

        self.snapshot_tx.send_modify(|s| {
            s.phase = Phase::Migrating;
            s.notice = Some("Você está assumindo como novo host…".to_string());
        });
        self.request_repaint();

        let (ev_tx, ev_rx) = mpsc::channel(128);
        match IrohRoomHost::start(
            old_info.secret,
            epoch,
            client_id.clone(),
            nickname.clone(),
            ev_tx,
        )
        .await
        {
            Ok(new_host) => {
                let new_ticket = new_host.ticket.clone();
                let new_meta = new_host.meta.clone();

                // Avisa o host antigo (que repassa para todos os amigos na sala)
                if let Some(old_tr) = self.transport.lock().await.clone() {
                    old_tr
                        .send_new_host(epoch, client_id.clone(), nickname.clone(), new_ticket.clone())
                        .await;
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }

                // Troca transporte local para o novo IrohRoomHost
                let old_tr = self
                    .transport
                    .lock()
                    .await
                    .replace(Arc::new(RoomTransport::Host(new_host)));
                if let Some(tr) = old_tr
                    && let Ok(owned) = Arc::try_unwrap(tr)
                {
                    owned.close(false).await;
                }

                self.spawn_event_pump(ev_rx);

                // Se já estava compartilhando tela, anuncia novamente no novo host!
                let active_share = self.snapshot().share;
                if let Some(ref sh) = active_share
                    && let Some(tr) = self.transport.lock().await.clone()
                {
                    tr.send_share_state(true, sh.audio_mode != AudioMode::None)
                        .await;
                    self.force_keyframe.store(true, Ordering::SeqCst);
                }

                let new_info = JoinInfo {
                    identity: client_id.clone(),
                    code: new_ticket,
                    secret: old_info.secret,
                    is_host: true,
                    scope: RoomScope::Internet,
                };
                self.snapshot_tx.send_modify(|s| {
                    s.phase = Phase::Connected;
                    s.info = Some(new_info);
                    s.meta = Some(new_meta);
                    s.notice = Some("Você agora é o host da sala!".to_string());
                });
                self.request_repaint();

                // Limpa aviso após 5s
                let sess = self.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    sess.snapshot_tx.send_modify(|s| {
                        if s.notice.as_deref() == Some("Você agora é o host da sala!") {
                            s.notice = None;
                        }
                    });
                    sess.request_repaint();
                });
            }
            Err(err) => {
                self.snapshot_tx.send_modify(|s| {
                    s.phase = Phase::Connected;
                    s.notice = Some(format!("Não consegui assumir como host: {err}"));
                });
                self.request_repaint();
            }
        }
        self.migrating.store(false, Ordering::SeqCst);
    }

    /// Reconecta automaticamente no novo host anunciado via `NewHost`.
    async fn migrate_to_new_host(self: &Arc<Self>, host_name: String, ticket_str: String) {
        if self.migrating.swap(true, Ordering::SeqCst) {
            return;
        }
        self.snapshot_tx.send_modify(|s| {
            s.phase = Phase::Migrating;
            s.notice = Some(format!("{host_name} agora é o host. Reconectando…"));
        });
        self.request_repaint();

        // Fecha conexão antiga
        if let Some(old_tr) = self.transport.lock().await.take()
            && let Ok(owned) = Arc::try_unwrap(old_tr)
        {
            owned.close(false).await;
        }

        let (client_id, nickname) = {
            let st = self.settings.lock().await;
            let s = st.get();
            (s.client_id.clone(), s.nickname.clone())
        };

        let Ok(ticket) = IrohRoomTicket::decode(&ticket_str) else {
            self.migrating.store(false, Ordering::SeqCst);
            self.reset_with_error(Some("Ticket do novo host inválido.".to_string()))
                .await;
            return;
        };
        let secret = ticket.secret;

        // Tenta conectar no novo host (até 4 tentativas rápidas)
        let mut connected = None;
        for _ in 0..4 {
            let (ev_tx, ev_rx) = mpsc::channel(128);
            if let Ok(tr) = RoomTransport::connect_client(
                ticket.clone(),
                client_id.clone(),
                nickname.clone(),
                ev_tx,
            )
            .await
            {
                connected = Some((tr, ev_rx));
                break;
            }
            tokio::time::sleep(Duration::from_millis(800)).await;
        }

        match connected {
            Some((tr, ev_rx)) => {
                let tr_arc = Arc::new(tr);
                *self.transport.lock().await = Some(tr_arc.clone());
                self.spawn_event_pump(ev_rx);

                if let Some(ref sh) = self.snapshot().share {
                    tr_arc
                        .send_share_state(true, sh.audio_mode != AudioMode::None)
                        .await;
                    self.force_keyframe.store(true, Ordering::SeqCst);
                }

                let new_info = JoinInfo {
                    identity: client_id,
                    code: ticket_str,
                    secret,
                    is_host: false,
                    scope: RoomScope::Internet,
                };
                let ok_notice = format!("{host_name} agora é o host da sala.");
                self.snapshot_tx.send_modify(|s| {
                    s.phase = Phase::Connected;
                    s.info = Some(new_info);
                    s.notice = Some(ok_notice.clone());
                });
                self.request_repaint();

                let sess = self.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    sess.snapshot_tx.send_modify(|s| {
                        if s.notice.as_deref() == Some(ok_notice.as_str()) {
                            s.notice = None;
                        }
                    });
                    sess.request_repaint();
                });
            }
            None => {
                self.migrating.store(false, Ordering::SeqCst);
                self.reset_with_error(Some(
                    "Não consegui reconectar no novo host da sala.".to_string(),
                ))
                .await;
                return;
            }
        }
        self.migrating.store(false, Ordering::SeqCst);
    }

    pub async fn leave_room(&self) {
        if self.leaving.swap(true, Ordering::SeqCst) {
            return;
        }
        let is_host = self.snapshot().is_host();
        self.snapshot_tx.send_modify(|s| {
            s.notice = Some(if is_host {
                "Transferindo o host para outro participante…".to_string()
            } else {
                "Saindo…".to_string()
            });
        });
        self.request_repaint();

        self.reset_with_error_internal(None, is_host).await;
        self.leaving.store(false, Ordering::SeqCst);
    }

    pub async fn start_share(self: &Arc<Self>, opts: ShareOptions) -> Result<(), String> {
        self.stop_share().await;
        *self.live_audio_quality.write().await = opts.audio_quality;

        let mut audio_mode = AudioMode::None;
        let mut warning = None;
        let mut audio_rx_opt = None;

        if opts.audio {
            let mut audio = self.audio.lock().await;
            audio.set_send_volume(opts.send_volume);
            match audio.start(&opts.blocked) {
                Ok((mode, rx)) => {
                    audio_mode = mode;
                    audio_rx_opt = Some(rx);
                    if mode == AudioMode::System {
                        warning = Some(
                            "Bloqueio por app indisponível neste Windows. Transmitindo áudio do sistema inteiro."
                                .to_string(),
                        );
                    }
                }
                Err(err) => {
                    warning = Some(format!("Áudio indisponível: {err}"));
                }
            }
        }

        let live_cfg = LiveCaptureConfig::from_res_fps(opts.resolution, opts.fps);
        let capturer =
            ScreenCapturer::start(opts.source_id.clone(), live_cfg, opts.gpu_adapter_index);
        let mut frame_rx = capturer.subscribe();
        *self.capturer.lock().await = Some(capturer);
        self.force_keyframe.store(true, Ordering::SeqCst);

        let local_id = self.settings.lock().await.get().client_id.clone();
        let local_name = self.settings.lock().await.get().nickname.clone();
        let has_audio = audio_mode != AudioMode::None;

        if let Some(tr) = self.transport.lock().await.clone() {
            tr.send_share_state(true, has_audio).await;
        }

        self.snapshot_tx.send_modify(|s| {
            s.share = Some(ActiveShare {
                resolution: opts.resolution,
                fps: opts.fps,
                audio_quality: opts.audio_quality,
                content_type: opts.content_type,
                audio_mode,
                warning,
            });
            if let Some(p) = s.peers.iter_mut().find(|p| p.is_local) {
                p.sharing = true;
            }
            if !s.streams.iter().any(|st| st.is_local) {
                s.streams.push(StreamView {
                    identity: local_id.clone(),
                    name: if local_name.is_empty() {
                        "Você".to_string()
                    } else {
                        local_name
                    },
                    is_local: true,
                    has_audio,
                    receive: ReceiveQuality::Auto,
                });
            }
        });
        self.request_repaint();

        // Pipeline de vídeo: Dirty-Rect + JPEG 4:2:0 + VideoMailbox (keyframes nunca descartados)
        let session_weak = Arc::downgrade(self);
        let vid_id = local_id.clone();
        let jpeg_quality = match opts.content_type {
            ContentType::Detail => 78u8,
            ContentType::Motion => 62u8,
        };
        let video_handle = tokio::spawn(async move {
            let encoder = Arc::new(std::sync::Mutex::new(DeltaVideoEncoder::new()));
            while frame_rx.changed().await.is_ok() {
                let Some(session) = session_weak.upgrade() else {
                    break;
                };
                let frame = frame_rx.borrow().clone();
                if let Some(frame) = frame {
                    if let Ok(mut map) = session.frames.write() {
                        map.insert(vid_id.clone(), frame.clone());
                    }
                    session.request_repaint();

                    if let Some(tr) = session.transport.lock().await.clone() {
                        let force_key = session.force_keyframe.swap(false, Ordering::SeqCst);
                        let enc_ref = encoder.clone();
                        let id_clone = vid_id.clone();
                        let encoded = tokio::task::spawn_blocking(move || {
                            let mut enc = enc_ref.lock().ok()?;
                            let (hdr, jpeg) = enc.encode(&frame, jpeg_quality, force_key)?;
                            let is_key = hdr.is_keyframe;
                            Some((encode_video_packet(&id_clone, hdr, &jpeg), is_key))
                        })
                        .await
                        .ok()
                        .flatten();

                        if let Some((pkt, is_key)) = encoded {
                            tr.send_video_packet(pkt, is_key).await;
                        }
                    }
                }
            }
        });
        *self.video_pump.lock().await = Some(video_handle);

        // Pipeline de áudio contínuo (sem quebrar o relógio do jitter buffer!)
        if let Some(mut audio_rx) = audio_rx_opt {
            let session_weak = Arc::downgrade(self);
            let aud_id = local_id;
            let audio_handle = tokio::spawn(async move {
                let mut seq = 0u64;
                while let Some(chunk) = audio_rx.recv().await {
                    let Some(session) = session_weak.upgrade() else {
                        break;
                    };
                    let quality = *session.live_audio_quality.read().await;
                    let target_rate = quality.sample_rate();
                    let codec = quality.codec_id();

                    if let Some(tr) = session.transport.lock().await.clone() {
                        seq += 1;
                        let prepared = if target_rate != AUDIO_SAMPLE_RATE {
                            resample_stereo_f32(&chunk, AUDIO_SAMPLE_RATE, target_rate)
                        } else {
                            chunk
                        };
                        let payload = match codec {
                            AUDIO_CODEC_PCM16 => encode_pcm16_stereo(&prepared),
                            AUDIO_CODEC_ADPCM | _ => encode_stereo_f32(&prepared),
                        };
                        if !payload.is_empty() {
                            let hdr = AudioPacketHeader {
                                seq,
                                sample_rate: target_rate,
                                codec,
                            };
                            let pkt = encode_audio_packet(&aud_id, hdr, &payload);
                            tr.send_audio_packet(pkt).await;
                        }
                    }
                }
            });
            *self.audio_pump.lock().await = Some(audio_handle);
        }

        Ok(())
    }

    pub async fn stop_share(&self) {
        if let Some(h) = self.video_pump.lock().await.take() {
            h.abort();
        }
        if let Some(h) = self.audio_pump.lock().await.take() {
            h.abort();
        }
        if let Some(mut cap) = self.capturer.lock().await.take() {
            cap.stop();
        }
        self.audio.lock().await.stop();
        if let Some(tr) = self.transport.lock().await.clone() {
            tr.send_share_state(false, false).await;
        }
        let local_id = self.settings.lock().await.get().client_id.clone();
        if let Ok(mut map) = self.frames.write() {
            map.remove(&local_id);
        }
        self.snapshot_tx.send_modify(|s| {
            s.share = None;
            if let Some(p) = s.peers.iter_mut().find(|p| p.is_local) {
                p.sharing = false;
            }
            s.streams.retain(|st| !st.is_local);
        });
        self.request_repaint();
    }

    async fn reset_with_error(&self, error: Option<String>) {
        self.reset_with_error_internal(error, false).await;
    }

    async fn reset_with_error_internal(&self, error: Option<String>, hand_off: bool) {
        if let Ok(mut guard) = self.event_pump.lock()
            && let Some(pump) = guard.take()
        {
            pump.abort();
        }
        self.stop_share().await;
        self.playback.clear();
        if let Some(tr) = self.transport.lock().await.take()
            && let Ok(owned) = Arc::try_unwrap(tr)
        {
            owned.close(hand_off).await;
        }
        self.receive_modes.lock().await.clear();
        if let Ok(mut map) = self.frames.write() {
            map.clear();
        }

        self.snapshot_tx.send_modify(|s| {
            s.phase = Phase::Idle;
            s.info = None;
            s.meta = None;
            s.notice = None;
            s.error = error;
            s.peers.clear();
            s.streams.clear();
        });
        self.request_repaint();
    }
}
