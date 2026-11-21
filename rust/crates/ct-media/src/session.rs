//! Máquina de estados da sala (`RoomSession`), suportando:
//!  - **P2P (Iroh)**: QUIC direto + relay automático, vídeo delta 4:2:0 + áudio IMA-ADPCM.
//!  - **Clássico (LiveKit)**: compatível com o app Electron antigo + hub WebSocket `/ws/room`.
//!  - Alteração ao vivo de resolução, FPS, volume e apps bloqueados durante a transmissão.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ct_core::adpcm::encode_stereo_f32;
use ct_core::protocol::{
    ContentType, PUBLIC_PORT, QualityId, ResolutionId, RoomMeta, RoomMode, RoomScope,
    encode_audio_packet, encode_video_packet,
};
use ct_core::room_code::{
    RoomAddress, decode_room_code, encode_room_code, generate_room_secret,
};
use ct_core::settings::{Settings, SettingsStore};
use ct_host::host_controller::{HostController, HostStartOptions};
use ct_host::livekit_process::LiveKitPaths;
use ct_host::public_server::PublicServer;
use ct_host::room_hub::ClassicRoomHub;
use ct_net::ip::{is_elevated, url_host};
use ct_net::lan_discovery::{LanResponder, find_on_lan};
use ct_net::network::NetworkManager;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{Mutex, mpsc, watch};
use tokio::task::JoinHandle;

use crate::audio::{AudioApp, AudioCaptureEngine, AudioMode, AudioPlaybackMixer};
use crate::p2p::{IrohRoomHost, IrohRoomTicket, RoomTransport, TransportEvent, is_iroh_ticket};
use crate::screen::{
    CapturedVideoFrame, DeltaVideoEncoder, LiveCaptureConfig, ScreenCapturer, ScreenSource,
    list_screen_sources,
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
    pub mode: RoomMode,
    pub url: String,
    pub token: String,
    pub identity: String,
    pub endpoint: String,
    pub code: String,
    pub secret: u32,
    pub is_host: bool,
    pub ports_open: Option<bool>,
    pub cgnat: Option<bool>,
    pub scope: Option<RoomScope>,
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
    pub content_type: ContentType,
    pub audio: bool,
    pub blocked: Vec<String>,
    pub send_volume: f32,
}

#[derive(Debug, Clone)]
pub struct ActiveShare {
    pub resolution: ResolutionId,
    pub fps: u32,
    pub content_type: ContentType,
    pub audio_mode: AudioMode,
    pub warning: Option<String>,
}

impl ActiveShare {
    pub fn label(&self) -> String {
        format!("{} · {} fps", self.resolution.label(), self.fps)
    }
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

    pub fn mode(&self) -> Option<RoomMode> {
        self.info.as_ref().map(|i| i.mode)
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

#[derive(Deserialize)]
struct JoinApiResponse {
    error: Option<String>,
    token: Option<String>,
    code: Option<String>,
}

pub struct RoomSession {
    data_dir: PathBuf,
    bin_dir: PathBuf,
    settings: Mutex<SettingsStore>,
    network: Mutex<NetworkManager>,
    public_server: Mutex<PublicServer>,
    host: Mutex<Option<Arc<HostController>>>,
    classic_hub: Mutex<Option<Arc<ClassicRoomHub>>>,
    lan_responder: Mutex<Option<LanResponder>>,
    transport: Mutex<Option<Arc<RoomTransport>>>,
    event_pump: Mutex<Option<JoinHandle<()>>>,
    video_pump: Mutex<Option<JoinHandle<()>>>,
    audio_pump: Mutex<Option<JoinHandle<()>>>,
    audio: Mutex<AudioCaptureEngine>,
    playback: AudioPlaybackMixer,
    capturer: Mutex<Option<ScreenCapturer>>,
    force_keyframe: Arc<AtomicBool>,
    receive_modes: Mutex<HashMap<String, ReceiveQuality>>,
    frames: std::sync::RwLock<HashMap<String, CapturedVideoFrame>>,
    repaint_cb: std::sync::RwLock<Option<Arc<dyn Fn() + Send + Sync>>>,
    http: Client,
    snapshot_tx: watch::Sender<SessionSnapshot>,
    snapshot_rx: watch::Receiver<SessionSnapshot>,
}

impl RoomSession {
    pub fn new(data_dir: PathBuf, bin_dir: PathBuf) -> Arc<Self> {
        let store = SettingsStore::load(data_dir.join("settings.json"));
        let settings_clone = store.get().clone();
        let client_id = settings_clone.client_id.clone();

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
            data_dir,
            bin_dir,
            settings: Mutex::new(store),
            network: Mutex::new(NetworkManager::new()),
            public_server: Mutex::new(PublicServer::new(client_id)),
            host: Mutex::new(None),
            classic_hub: Mutex::new(None),
            lan_responder: Mutex::new(None),
            transport: Mutex::new(None),
            event_pump: Mutex::new(None),
            video_pump: Mutex::new(None),
            audio_pump: Mutex::new(None),
            audio: Mutex::new(AudioCaptureEngine::new()),
            playback: AudioPlaybackMixer::new(),
            capturer: Mutex::new(None),
            force_keyframe: Arc::new(AtomicBool::new(true)),
            receive_modes: Mutex::new(HashMap::new()),
            frames: std::sync::RwLock::new(HashMap::new()),
            repaint_cb: std::sync::RwLock::new(None),
            http: Client::new(),
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
        self.update_settings(|s| s.quality = qid).await;
        self.snapshot_tx.send_modify(|s| {
            if let Some(ref mut share) = s.share {
                share.resolution = resolution;
                share.fps = fps;
            }
        });
        self.request_repaint();
    }

    pub async fn set_max_quality(&self, quality: Option<QualityId>) {
        if let Some(transport) = self.transport.lock().await.clone() {
            transport.set_max_quality(quality).await;
        }
        if let Some(host) = self.host.lock().await.clone() {
            host.set_max_quality(quality).await;
            let meta = host.room_meta().await;
            if let Some(hub) = self.classic_hub.lock().await.clone() {
                hub.update_meta(meta.clone()).await;
            }
            self.snapshot_tx.send_modify(|s| s.meta = Some(meta));
            self.request_repaint();
        }
    }

    pub async fn create_room(self: &Arc<Self>, nickname: String, mode: RoomMode) {
        self.snapshot_tx.send_modify(|s| {
            s.phase = Phase::Connecting;
            s.error = None;
        });
        self.request_repaint();

        let secret = generate_room_secret();
        match mode {
            RoomMode::Iroh => {
                let (ev_tx, ev_rx) = mpsc::channel(128);
                let client_id = self.settings.lock().await.get().client_id.clone();
                match IrohRoomHost::start(secret, client_id.clone(), nickname.clone(), ev_tx).await
                {
                    Ok(iroh_host) => {
                        let ticket = iroh_host.ticket.clone();
                        let meta = iroh_host.meta.clone();
                        let info = JoinInfo {
                            mode: RoomMode::Iroh,
                            url: "iroh://p2p".to_string(),
                            token: String::new(),
                            identity: client_id.clone(),
                            endpoint: "iroh".to_string(),
                            code: ticket,
                            secret,
                            is_host: true,
                            ports_open: Some(true),
                            cgnat: Some(false),
                            scope: Some(RoomScope::Internet),
                        };
                        let local_peer = PeerView {
                            identity: client_id,
                            name: nickname,
                            is_local: true,
                            is_host: true,
                            can_host: Some(true),
                            sharing: false,
                        };
                        *self.transport.lock().await =
                            Some(Arc::new(RoomTransport::IrohHost(iroh_host)));
                        self.spawn_event_pump(ev_rx).await;

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
            RoomMode::Classic => match self.become_classic_host(secret, 1, nickname.clone()).await {
                Ok((info, meta)) => {
                    let (ev_tx, ev_rx) = mpsc::channel(128);
                    let ws_url = format!("ws://127.0.0.1:{PUBLIC_PORT}/ws/room");
                    if let Ok(tr) = RoomTransport::connect_classic_ws(
                        &ws_url,
                        info.identity.clone(),
                        nickname.clone(),
                        secret,
                        ev_tx,
                    )
                    .await
                    {
                        *self.transport.lock().await = Some(Arc::new(tr));
                        self.spawn_event_pump(ev_rx).await;
                    }
                    let local_peer = PeerView {
                        identity: info.identity.clone(),
                        name: nickname,
                        is_local: true,
                        is_host: true,
                        can_host: Some(true),
                        sharing: false,
                    };
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
            },
        }
    }

    pub async fn join_room(self: &Arc<Self>, code: String, nickname: String) {
        self.snapshot_tx.send_modify(|s| {
            s.phase = Phase::Connecting;
            s.error = None;
        });
        self.request_repaint();

        if is_iroh_ticket(&code) {
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
            match RoomTransport::connect_iroh_client(
                ticket,
                client_id.clone(),
                nickname.clone(),
                ev_tx,
            )
            .await
            {
                Ok(tr) => {
                    *self.transport.lock().await = Some(Arc::new(tr));
                    self.spawn_event_pump(ev_rx).await;
                    let info = JoinInfo {
                        mode: RoomMode::Iroh,
                        url: "iroh://p2p".to_string(),
                        token: String::new(),
                        identity: client_id.clone(),
                        endpoint: "iroh".to_string(),
                        code: code.trim().to_string(),
                        secret,
                        is_host: false,
                        ports_open: None,
                        cgnat: None,
                        scope: Some(RoomScope::Internet),
                    };
                    let local_peer = PeerView {
                        identity: client_id,
                        name: nickname,
                        is_local: true,
                        is_host: false,
                        can_host: Some(false),
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
            return;
        }

        let addr = match decode_room_code(&code) {
            Ok(a) => a,
            Err(e) => {
                self.reset_with_error(Some(e.to_string())).await;
                return;
            }
        };
        match self
            .join_classic_target(&addr.ip, addr.secret, &nickname)
            .await
        {
            Ok((info, target_ip)) => {
                let (ev_tx, ev_rx) = mpsc::channel(128);
                let ws_url = format!("ws://{}:{PUBLIC_PORT}/ws/room", url_host(&target_ip));
                if let Ok(tr) = RoomTransport::connect_classic_ws(
                    &ws_url,
                    info.identity.clone(),
                    nickname.clone(),
                    addr.secret,
                    ev_tx,
                )
                .await
                {
                    *self.transport.lock().await = Some(Arc::new(tr));
                    self.spawn_event_pump(ev_rx).await;
                }

                let local_peer = PeerView {
                    identity: info.identity.clone(),
                    name: nickname,
                    is_local: true,
                    is_host: false,
                    can_host: None,
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

    async fn spawn_event_pump(self: &Arc<Self>, mut ev_rx: mpsc::Receiver<TransportEvent>) {
        if let Some(old) = self.event_pump.lock().await.take() {
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
                            let updated = if let Ok(mut map) = session.frames.write() {
                                let should_replace = map
                                    .get(&identity)
                                    .is_none_or(|prev| frame.seq > prev.seq || frame.width != prev.width || frame.height != prev.height);
                                if should_replace {
                                    map.insert(identity, frame);
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                            if updated {
                                session.request_repaint();
                            }
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
                    TransportEvent::RoomEnd { reason } => {
                        session.reset_with_error(Some(reason)).await;
                        break;
                    }
                    TransportEvent::Disconnected => {
                        if session.snapshot().phase != Phase::Idle {
                            session
                                .reset_with_error(Some(
                                    "Conexão com o host perdida (o host saiu da sala).".to_string(),
                                ))
                                .await;
                        }
                        break;
                    }
                }
            }
        });
        *self.event_pump.lock().await = Some(handle);
    }

    pub async fn leave_room(&self) {
        let is_host = self.snapshot().is_host();
        self.snapshot_tx.send_modify(|s| {
            s.notice = Some(if is_host {
                "Encerrando sala…".to_string()
            } else {
                "Saindo…".to_string()
            });
        });
        self.request_repaint();

        if let Some(hub) = self.classic_hub.lock().await.clone() {
            hub.close_room("O host encerrou a sala.").await;
        }
        self.reset_with_error(None).await;
    }

    pub async fn start_share(self: &Arc<Self>, opts: ShareOptions) -> Result<(), String> {
        self.stop_share().await;
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
        let capturer = ScreenCapturer::start(opts.source_id.clone(), live_cfg);
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

        // Pipeline de vídeo: Dirty-Rect + JPEG 4:2:0 em thread bloqueante + envio ordenado
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
                        let pkt = tokio::task::spawn_blocking(move || {
                            let mut enc = enc_ref.lock().ok()?;
                            let (hdr, jpeg) = enc.encode(&frame, jpeg_quality, force_key)?;
                            Some(encode_video_packet(&id_clone, hdr, &jpeg))
                        })
                        .await
                        .ok()
                        .flatten();

                        if let Some(pkt) = pkt {
                            tr.send_video_packet(pkt).await;
                        }
                    }
                }
            }
        });
        *self.video_pump.lock().await = Some(video_handle);

        // Pipeline de áudio: PCM 48 kHz estéreo -> IMA-ADPCM 4:1 -> stream ordenado dedicado
        if let Some(mut audio_rx) = audio_rx_opt {
            let session_weak = Arc::downgrade(self);
            let aud_id = local_id;
            let audio_handle = tokio::spawn(async move {
                let mut seq = 0u64;
                while let Some(chunk) = audio_rx.recv().await {
                    let Some(session) = session_weak.upgrade() else {
                        break;
                    };
                    // Pula envio de blocos 100% silenciosos para economizar banda
                    let peak = chunk.iter().fold(0.0f32, |acc, &s| acc.max(s.abs()));
                    if peak < 1e-4 {
                        continue;
                    }
                    if let Some(tr) = session.transport.lock().await.clone() {
                        seq += 1;
                        let adpcm = encode_stereo_f32(&chunk);
                        if !adpcm.is_empty() {
                            let pkt = encode_audio_packet(&aud_id, seq, &adpcm);
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

    async fn become_classic_host(
        &self,
        secret: u32,
        epoch: u64,
        nickname: String,
    ) -> Result<(JoinInfo, RoomMeta), String> {
        self.stop_hosting().await;
        {
            let mut srv = self.public_server.lock().await;
            srv.stop_udp_probe();
            if !srv.listen().await {
                return Err(format!(
                    "A porta {PUBLIC_PORT} já está em uso. Há outra cópia do app aberta?"
                ));
            }
        }

        let (net_info, ports_open) = {
            let mut net = self.network.lock().await;
            let info = net.resolve().await;
            let via_v4 = info.public_ip.is_some() && !info.cgnat;
            let open = if via_v4 { net.open_ports().await } else { false };
            (info, open)
        };

        let via_ipv4 = net_info.public_ip.is_some() && !net_info.cgnat;
        let public_ip = if via_ipv4 {
            net_info.public_ip.clone()
        } else {
            net_info.ipv6.clone()
        };
        let ip = public_ip
            .clone()
            .or_else(|| net_info.lan_ips.first().cloned())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let code = encode_room_code(&RoomAddress {
            ip: ip.clone(),
            secret,
        })
        .map_err(|e| e.to_string())?;

        let client_id = self.settings.lock().await.get().client_id.clone();
        let exe_name = if cfg!(windows) {
            "livekit-server.exe"
        } else {
            "livekit-server"
        };
        let ctl = Arc::new(HostController::new(LiveKitPaths {
            exe: self.bin_dir.join(exe_name),
            work_dir: self.data_dir.join("livekit"),
        }));
        let meta = ctl
            .start(HostStartOptions {
                secret,
                epoch,
                host_id: client_id.clone(),
                host_name: nickname.clone(),
                public_ip,
                node_ip: if via_ipv4 { net_info.public_ip } else { None },
                code: code.clone(),
            })
            .await
            .map_err(|e| e.to_string())?;

        let token = ctl
            .mint_token(&client_id, &nickname)
            .map_err(|e| e.to_string())?;

        let hub = ClassicRoomHub::new(secret, meta.clone());
        hub.register_host_peer(client_id.clone(), nickname).await;

        *self.host.lock().await = Some(ctl.clone());
        *self.classic_hub.lock().await = Some(hub.clone());
        {
            let srv = self.public_server.lock().await;
            srv.set_host(Some(ctl)).await;
            srv.set_hub(Some(hub)).await;
        }
        *self.lan_responder.lock().await = LanResponder::start(secret).await;

        let info = JoinInfo {
            mode: RoomMode::Classic,
            url: format!("ws://127.0.0.1:{PUBLIC_PORT}"),
            token,
            identity: client_id,
            endpoint: ip,
            code,
            secret,
            is_host: true,
            ports_open: Some(ports_open),
            cgnat: Some(net_info.cgnat),
            scope: Some(meta.scope),
        };
        Ok((info, meta))
    }

    async fn join_classic_target(
        &self,
        ip: &str,
        secret: u32,
        nickname: &str,
    ) -> Result<(JoinInfo, String), String> {
        let target = find_on_lan(secret, Duration::from_millis(500))
            .await
            .unwrap_or_else(|| ip.to_string());
        let client_id = self.settings.lock().await.get().client_id.clone();
        let url = format!("http://{}:{PUBLIC_PORT}/api/join", url_host(&target));

        let res = self
            .http
            .post(url)
            .json(&json!({
                "secret": secret,
                "identity": client_id,
                "name": nickname,
            }))
            .timeout(Duration::from_secs(6))
            .send()
            .await
            .map_err(|_| {
                if target.contains(':') {
                    "Não consegui falar com o host pelo IPv6. Ele pode ter saído ou o firewall IPv6 do roteador está bloqueando.".to_string()
                } else {
                    "Não consegui falar com o host. Ele pode ter saído ou estar com as portas fechadas.".to_string()
                }
            })?;

        let status = res.status();
        let body: JoinApiResponse = res.json().await.unwrap_or(JoinApiResponse {
            error: None,
            token: None,
            code: None,
        });

        let Some(token) = body.token.filter(|_| status.is_success()) else {
            let msg = match body.error.as_deref() {
                Some("wrong-code") => "Código incorreto ou sala encerrada.".to_string(),
                Some("room-full") => "A sala está cheia (máximo de 10 pessoas).".to_string(),
                Some("too-many-attempts") => {
                    "Muitas tentativas com código errado. Espere alguns minutos.".to_string()
                }
                Some("not-host") => "Esse PC não está mais hospedando a sala.".to_string(),
                _ => format!("O host recusou a entrada ({status})."),
            };
            return Err(msg);
        };

        let fallback_code = encode_room_code(&RoomAddress {
            ip: ip.to_string(),
            secret,
        })
        .unwrap_or_default();

        Ok((
            JoinInfo {
                mode: RoomMode::Classic,
                url: format!("ws://{}:{PUBLIC_PORT}", url_host(&target)),
                token,
                identity: client_id,
                endpoint: ip.to_string(),
                code: body.code.unwrap_or(fallback_code),
                secret,
                is_host: false,
                ports_open: None,
                cgnat: None,
                scope: None,
            },
            target,
        ))
    }

    async fn stop_hosting(&self) {
        if let Some(resp) = self.lan_responder.lock().await.take() {
            resp.close();
        }
        {
            let srv = self.public_server.lock().await;
            srv.set_host(None).await;
            srv.set_hub(None).await;
        }
        *self.classic_hub.lock().await = None;
        if let Some(host) = self.host.lock().await.take() {
            host.stop().await;
        }
    }

    async fn reset_with_error(&self, error: Option<String>) {
        if let Some(pump) = self.event_pump.lock().await.take() {
            pump.abort();
        }
        self.stop_share().await;
        self.playback.clear();
        if let Some(tr) = self.transport.lock().await.take()
            && let Ok(owned) = Arc::try_unwrap(tr)
        {
            owned.close("O host encerrou a sala.").await;
        }
        self.stop_hosting().await;
        self.public_server.lock().await.close();
        self.network.lock().await.close_ports().await;
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
