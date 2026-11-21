//! Máquina de estados da sala (`RoomSession`), coordenando `ct-core`, `ct-net`,
//! `ct-host`, captura de tela e áudio por app, e publicando snapshots imutáveis para a UI.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ct_core::election::successor_candidates;
use ct_core::protocol::{
    ContentType, PUBLIC_PORT, QualityId, QualityPreset, RoomMeta, RoomScope, preset_by_id,
};
use ct_core::room_code::{
    RoomAddress, decode_room_code, encode_room_code, generate_room_secret,
};
use ct_core::settings::{Settings, SettingsStore};
use ct_host::host_controller::{HostController, HostStartOptions};
use ct_host::livekit_process::LiveKitPaths;
use ct_host::public_server::PublicServer;
use ct_net::ip::{is_elevated, url_host};
use ct_net::lan_discovery::{LanResponder, find_on_lan};
use ct_net::network::NetworkManager;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{Mutex, watch};

use crate::audio::{AudioApp, AudioCaptureEngine, AudioMode};
use crate::screen::{CapturedVideoFrame, ScreenCapturer, ScreenSource, list_screen_sources};

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
    pub frame: Option<CapturedVideoFrame>,
}

#[derive(Debug, Clone)]
pub struct ShareOptions {
    pub source_id: String,
    pub quality: QualityId,
    pub content_type: ContentType,
    pub audio: bool,
    pub blocked: Vec<String>,
    pub send_volume: f32,
}

#[derive(Debug, Clone)]
pub struct ActiveShare {
    pub preset: QualityPreset,
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
    lan_responder: Mutex<Option<LanResponder>>,
    audio: Mutex<AudioCaptureEngine>,
    capturer: Mutex<Option<ScreenCapturer>>,
    receive_modes: Mutex<HashMap<String, ReceiveQuality>>,
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
            lan_responder: Mutex::new(None),
            audio: Mutex::new(AudioCaptureEngine::new()),
            capturer: Mutex::new(None),
            receive_modes: Mutex::new(HashMap::new()),
            http: Client::new(),
            snapshot_tx: tx,
            snapshot_rx: rx,
        })
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
        self.audio.lock().await.list_apps(blocked)
    }

    pub async fn update_settings<F>(&self, mutate: F)
    where
        F: FnOnce(&mut Settings),
    {
        let updated = self.settings.lock().await.update(mutate).clone();
        self.snapshot_tx.send_modify(|s| s.settings = updated);
    }

    pub async fn set_blocked_apps(&self, blocked: Vec<String>) {
        self.audio.lock().await.set_blocked(&blocked).await;
        self.update_settings(|s| s.blocked_apps = blocked).await;
    }

    pub async fn set_send_volume(&self, volume: f32) {
        self.audio.lock().await.set_send_volume(volume);
        self.update_settings(|s| s.send_volume = volume).await;
    }

    pub async fn set_peer_volume(&self, name: String, volume: f32) {
        self.update_settings(|s| {
            s.volumes.insert(name, volume.clamp(0.0, 2.0));
        })
        .await;
    }

    pub async fn set_receive_quality(&self, identity: String, mode: ReceiveQuality) {
        self.receive_modes
            .lock()
            .await
            .insert(identity.clone(), mode);
        self.snapshot_tx.send_modify(|s| {
            if let Some(stream) = s.streams.iter_mut().find(|st| st.identity == identity) {
                stream.receive = mode;
            }
        });
    }

    pub async fn set_max_quality(&self, quality: Option<QualityId>) {
        if let Some(host) = self.host.lock().await.clone() {
            host.set_max_quality(quality).await;
            let meta = host.room_meta().await;
            self.snapshot_tx.send_modify(|s| s.meta = Some(meta));
        }
    }

    pub async fn create_room(&self, nickname: String) {
        self.snapshot_tx.send_modify(|s| {
            s.phase = Phase::Connecting;
            s.error = None;
        });
        let secret = generate_room_secret();
        match self.become_host(secret, 1, nickname.clone()).await {
            Ok((info, meta)) => {
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
            }
            Err(err) => {
                self.reset_with_error(Some(err)).await;
            }
        }
    }

    pub async fn join_room(&self, code: String, nickname: String) {
        self.snapshot_tx.send_modify(|s| {
            s.phase = Phase::Connecting;
            s.error = None;
        });
        let addr = match decode_room_code(&code) {
            Ok(a) => a,
            Err(e) => {
                self.reset_with_error(Some(e.to_string())).await;
                return;
            }
        };
        match self.join_target(&addr.ip, addr.secret, &nickname).await {
            Ok(info) => {
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
            }
            Err(err) => {
                self.reset_with_error(Some(err)).await;
            }
        }
    }

    pub async fn leave_room(&self) {
        let is_host = self.snapshot().is_host();
        self.snapshot_tx.send_modify(|s| {
            s.notice = Some(if is_host {
                "Passando o host para outra pessoa…".to_string()
            } else {
                "Saindo…".to_string()
            });
        });

        // Verifica candidatos de sucessão disponíveis antes de encerrar
        let snap = self.snapshot();
        let present: Vec<&str> = snap.peers.iter().map(|p| p.identity.as_str()).collect();
        let _candidates = successor_candidates(snap.meta.as_ref(), present, &[]);

        self.reset_with_error(None).await;
    }

    pub async fn start_share(self: &Arc<Self>, opts: ShareOptions) -> Result<(), String> {
        self.stop_share().await;
        let preset = preset_by_id(opts.quality);
        let mut audio_mode = AudioMode::None;
        let mut warning = None;

        if opts.audio {
            let mut audio = self.audio.lock().await;
            audio.set_send_volume(opts.send_volume);
            match audio.start(&opts.blocked).await {
                Ok(_consumer) => {
                    audio_mode = AudioMode::Native;
                }
                Err(err) => {
                    warning = Some(format!(
                        "Bloqueio de apps indisponível ({err}). O áudio de todo o PC será transmitido."
                    ));
                    audio_mode = AudioMode::System;
                }
            }
        }

        let capturer = ScreenCapturer::start(opts.source_id.clone(), preset);
        let mut frame_rx = capturer.subscribe();
        *self.capturer.lock().await = Some(capturer);

        let local_id = self.settings.lock().await.get().client_id.clone();
        let local_name = self.settings.lock().await.get().nickname.clone();

        self.snapshot_tx.send_modify(|s| {
            s.share = Some(ActiveShare {
                preset,
                content_type: opts.content_type,
                audio_mode,
                warning,
            });
            if let Some(p) = s.peers.iter_mut().find(|p| p.is_local) {
                p.sharing = true;
            }
            s.streams.retain(|st| !st.is_local);
            s.streams.push(StreamView {
                identity: local_id.clone(),
                name: if local_name.is_empty() {
                    "Você".to_string()
                } else {
                    local_name
                },
                is_local: true,
                has_audio: audio_mode != AudioMode::None,
                receive: ReceiveQuality::Auto,
                frame: None,
            });
        });

        let session_weak = Arc::downgrade(self);
        tokio::spawn(async move {
            while frame_rx.changed().await.is_ok() {
                let Some(session) = session_weak.upgrade() else {
                    break;
                };
                let frame = frame_rx.borrow().clone();
                session.snapshot_tx.send_modify(|s| {
                    if let Some(st) = s.streams.iter_mut().find(|st| st.is_local) {
                        st.frame = frame;
                    }
                });
            }
        });

        Ok(())
    }

    pub async fn stop_share(&self) {
        if let Some(mut cap) = self.capturer.lock().await.take() {
            cap.stop();
        }
        self.audio.lock().await.stop();
        self.snapshot_tx.send_modify(|s| {
            s.share = None;
            if let Some(p) = s.peers.iter_mut().find(|p| p.is_local) {
                p.sharing = false;
            }
            s.streams.retain(|st| !st.is_local);
        });
    }

    async fn become_host(
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

        *self.host.lock().await = Some(ctl.clone());
        self.public_server.lock().await.set_host(Some(ctl)).await;
        *self.lan_responder.lock().await = LanResponder::start(secret).await;

        let info = JoinInfo {
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

    async fn join_target(&self, ip: &str, secret: u32, nickname: &str) -> Result<JoinInfo, String> {
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

        Ok(JoinInfo {
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
        })
    }

    async fn stop_hosting(&self) {
        if let Some(resp) = self.lan_responder.lock().await.take() {
            resp.close();
        }
        self.public_server.lock().await.set_host(None).await;
        if let Some(host) = self.host.lock().await.take() {
            host.stop().await;
        }
    }

    async fn reset_with_error(&self, error: Option<String>) {
        self.stop_share().await;
        self.stop_hosting().await;
        self.public_server.lock().await.close();
        self.network.lock().await.close_ports().await;
        self.receive_modes.lock().await.clear();

        self.snapshot_tx.send_modify(|s| {
            s.phase = Phase::Idle;
            s.info = None;
            s.meta = None;
            s.notice = None;
            s.error = error;
            s.peers.clear();
            s.streams.clear();
        });
    }
}
