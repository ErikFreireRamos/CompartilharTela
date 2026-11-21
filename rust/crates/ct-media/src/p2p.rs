//! Transporte P2P 100% em Iroh (QUIC com NAT traversal + relay automático):
//!  - 1 stream bidirecional de controle (`Hello`, `RoomState`, `ShareState`, `RequestKeyframe`,
//!    `Handoff`, `NewHost`, `RoomEnd`).
//!  - 1 stream unidirecional persistente de vídeo apoiado por `VideoMailbox` (keyframes NUNCA
//!    são descartados; deltas antigos são substituídos pelo mais novo -> latência mínima e zero travamento).
//!  - 1 stream unidirecional persistente de áudio (ADPCM / PCM16 com resample automático para 48 kHz).
//!  - Troca automática de host (`Handoff` -> `NewHost`) quando o host sai da sala!

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ct_core::adpcm::{decode_pcm16_stereo, decode_stereo_f32, resample_stereo_f32};
use ct_core::protocol::{
    AUDIO_CODEC_ADPCM, AUDIO_CODEC_PCM16, AudioPacketHeader, MAX_PARTICIPANTS, RoomMeta, RoomScope,
    RoomWireMessage, VideoPacketHeader, WirePeer, decode_audio_packet, decode_video_packet,
};
use iroh::endpoint::Connection;
use iroh::{Endpoint, EndpointAddr};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Notify, RwLock, mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::audio::AUDIO_SAMPLE_RATE;
use crate::screen::{CanvasUpdate, CapturedVideoFrame, RemoteVideoCanvas};

pub const IROH_ALPN: &[u8] = b"compartilhartela/p2p/3";
pub const IROH_TICKET_PREFIX: &str = "IROH-";

const STREAM_KIND_VIDEO: u8 = 1;
const STREAM_KIND_AUDIO: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrohRoomTicket {
    pub addr: EndpointAddr,
    pub secret: u32,
}

impl IrohRoomTicket {
    pub fn encode(&self) -> Result<String, String> {
        let json = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        Ok(format!("{IROH_TICKET_PREFIX}{}", URL_SAFE_NO_PAD.encode(json)))
    }

    pub fn decode(raw: &str) -> Result<Self, String> {
        let trimmed = raw.trim();
        let payload = trimmed
            .strip_prefix(IROH_TICKET_PREFIX)
            .or_else(|| trimmed.strip_prefix("iroh-"))
            .ok_or_else(|| "Código da sala inválido. Cole um código IROH-...".to_string())?;
        let bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| "Código IROH corrompido (base64 inválido).".to_string())?;
        serde_json::from_slice(&bytes)
            .map_err(|_| "Código IROH inválido ou incompatível.".to_string())
    }
}

pub fn is_iroh_ticket(raw: &str) -> bool {
    let t = raw.trim();
    t.starts_with(IROH_TICKET_PREFIX) || t.starts_with("iroh-")
}

#[derive(Debug, Clone)]
pub enum TransportEvent {
    RoomState {
        meta: RoomMeta,
        peers: Vec<WirePeer>,
    },
    VideoFrame {
        identity: String,
        frame: CapturedVideoFrame,
    },
    AudioSamples {
        identity: String,
        samples: Vec<f32>,
    },
    RequestKeyframe,
    Handoff {
        to: String,
        epoch: u64,
    },
    NewHost {
        epoch: u64,
        host_id: String,
        host_name: String,
        ticket: String,
    },
    RoomEnd {
        reason: String,
    },
    Disconnected,
}

/// Caixa postal de vídeo de slot único por peer:
/// - Keyframes NUNCA são descartados (garantindo que troca de qualidade/FPS ou reinício nunca congele).
/// - Deltas antigos são sobrescritos pelo mais recente quando a rede está mais lenta que o FPS.
struct VideoMailboxInner {
    pending_keyframe: Option<Arc<Vec<u8>>>,
    latest_delta: Option<Arc<Vec<u8>>>,
    closed: bool,
}

#[derive(Clone)]
pub struct VideoMailbox {
    inner: Arc<StdMutex<VideoMailboxInner>>,
    notify: Arc<Notify>,
}

impl VideoMailbox {
    fn new() -> Self {
        Self {
            inner: Arc::new(StdMutex::new(VideoMailboxInner {
                pending_keyframe: None,
                latest_delta: None,
                closed: false,
            })),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn push(&self, packet: Arc<Vec<u8>>, is_keyframe: bool) {
        if let Ok(mut guard) = self.inner.lock() {
            if guard.closed {
                return;
            }
            if is_keyframe {
                guard.pending_keyframe = Some(packet);
                guard.latest_delta = None; // keyframe mais novo invalida deltas anteriores
            } else {
                guard.latest_delta = Some(packet);
            }
            self.notify.notify_one();
        }
    }

    pub fn close(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.closed = true;
            self.notify.notify_waiters();
        }
    }

    async fn take_next(&self) -> Option<Arc<Vec<u8>>> {
        loop {
            {
                let mut guard = self.inner.lock().ok()?;
                if let Some(kf) = guard.pending_keyframe.take() {
                    return Some(kf);
                }
                if let Some(delta) = guard.latest_delta.take() {
                    return Some(delta);
                }
                if guard.closed {
                    return None;
                }
            }
            self.notify.notified().await;
        }
    }
}

async fn write_framed_bytes<W: AsyncWriteExt + Unpin>(
    w: &mut W,
    data: &[u8],
) -> Result<(), std::io::Error> {
    let len = (data.len() as u32).to_le_bytes();
    w.write_all(&len).await?;
    w.write_all(data).await?;
    w.flush().await
}

async fn read_framed_bytes<R: AsyncReadExt + Unpin>(
    r: &mut R,
    max_len: usize,
) -> Result<Option<Vec<u8>>, std::io::Error> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > max_len {
        return Ok(None);
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(Some(buf))
}

async fn write_framed_json<W: AsyncWriteExt + Unpin>(
    w: &mut W,
    msg: &RoomWireMessage,
) -> Result<(), std::io::Error> {
    let data = serde_json::to_vec(msg).unwrap_or_default();
    write_framed_bytes(w, &data).await
}

async fn read_framed_json<R: AsyncReadExt + Unpin>(
    r: &mut R,
) -> Result<Option<RoomWireMessage>, std::io::Error> {
    let Some(buf) = read_framed_bytes(r, 512 * 1024).await? else {
        return Ok(None);
    };
    Ok(serde_json::from_slice(&buf).ok())
}

fn spawn_video_uni_sender(conn: Connection, mailbox: VideoMailbox) -> JoinHandle<()> {
    tokio::spawn(async move {
        let Ok(mut send) = conn.open_uni().await else {
            return;
        };
        if send.write_all(&[STREAM_KIND_VIDEO]).await.is_err() {
            return;
        }
        while let Some(pkt) = mailbox.take_next().await {
            if write_framed_bytes(&mut send, &pkt).await.is_err() {
                break;
            }
        }
        let _ = send.finish();
    })
}

fn spawn_audio_uni_sender(conn: Connection, mut rx: mpsc::Receiver<Arc<Vec<u8>>>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let Ok(mut send) = conn.open_uni().await else {
            return;
        };
        if send.write_all(&[STREAM_KIND_AUDIO]).await.is_err() {
            return;
        }
        while let Some(pkt) = rx.recv().await {
            if write_framed_bytes(&mut send, &pkt).await.is_err() {
                break;
            }
        }
        let _ = send.finish();
    })
}

fn decode_audio_payload(hdr: AudioPacketHeader, payload: &[u8]) -> Option<Vec<f32>> {
    let raw = match hdr.codec {
        AUDIO_CODEC_ADPCM => decode_stereo_f32(payload)?,
        AUDIO_CODEC_PCM16 => decode_pcm16_stereo(payload)?,
        _ => return None,
    };
    if hdr.sample_rate != AUDIO_SAMPLE_RATE && hdr.sample_rate >= 8_000 && hdr.sample_rate <= 96_000
    {
        Some(resample_stereo_f32(&raw, hdr.sample_rate, AUDIO_SAMPLE_RATE))
    } else {
        Some(raw)
    }
}

fn apply_video_to_canvases(
    canvases: &mut HashMap<String, RemoteVideoCanvas>,
    id: String,
    hdr: VideoPacketHeader,
    jpeg: &[u8],
) -> (String, CanvasUpdate) {
    let canvas = canvases.entry(id.clone()).or_default();
    let res = canvas.apply_packet(hdr, jpeg);
    (id, res)
}

// ============================================================================
// Iroh P2P Host (com fila de sucessão automática para troca de host)
// ============================================================================

struct IrohRemotePeer {
    info: WirePeer,
    ctrl_tx: mpsc::Sender<RoomWireMessage>,
    video_box: VideoMailbox,
    audio_tx: mpsc::Sender<Arc<Vec<u8>>>,
}

type HandoffWaiter = (String, oneshot::Sender<(String, String, String)>);

struct IrohHostShared {
    secret: u32,
    host_id: String,
    meta: RwLock<RoomMeta>,
    host_peer: RwLock<WirePeer>,
    remotes: RwLock<HashMap<String, IrohRemotePeer>>,
    handoff_waiter: StdMutex<Option<HandoffWaiter>>,
    local_events: mpsc::Sender<TransportEvent>,
}

impl IrohHostShared {
    async fn broadcast_state(&self) {
        let meta = self.meta.read().await.clone();
        let host_peer = self.host_peer.read().await.clone();
        let remotes = self.remotes.read().await;
        let mut peers = Vec::with_capacity(remotes.len() + 1);
        peers.push(host_peer);
        for p in remotes.values() {
            peers.push(p.info.clone());
        }

        let _ = self.local_events.try_send(TransportEvent::RoomState {
            meta: meta.clone(),
            peers: peers.clone(),
        });

        let msg = RoomWireMessage::RoomState { meta, peers };
        for p in remotes.values() {
            let _ = p.ctrl_tx.try_send(msg.clone());
        }
    }

    async fn request_keyframes(&self) {
        let _ = self.local_events.try_send(TransportEvent::RequestKeyframe);
        let remotes = self.remotes.read().await;
        for p in remotes.values() {
            if p.info.sharing {
                let _ = p.ctrl_tx.try_send(RoomWireMessage::RequestKeyframe);
            }
        }
    }

    async fn fanout_video(&self, sender_id: &str, packet: Arc<Vec<u8>>, is_keyframe: bool) {
        let remotes = self.remotes.read().await;
        for (id, peer) in remotes.iter() {
            if id != sender_id {
                peer.video_box.push(packet.clone(), is_keyframe);
            }
        }
    }

    async fn fanout_audio(&self, sender_id: &str, packet: Arc<Vec<u8>>) {
        let remotes = self.remotes.read().await;
        for (id, peer) in remotes.iter() {
            if id != sender_id {
                let _ = peer.audio_tx.try_send(packet.clone());
            }
        }
    }
}

pub struct IrohRoomHost {
    endpoint: Endpoint,
    shared: Arc<IrohHostShared>,
    accept_task: JoinHandle<()>,
    pub ticket: String,
    pub meta: RoomMeta,
}

impl IrohRoomHost {
    pub async fn start(
        secret: u32,
        epoch: u64,
        host_id: String,
        host_name: String,
        events_tx: mpsc::Sender<TransportEvent>,
    ) -> Result<Self, String> {
        let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
            .alpns(vec![IROH_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| format!("Falha ao iniciar endpoint Iroh P2P: {e}"))?;

        let _ = tokio::time::timeout(Duration::from_secs(5), endpoint.online()).await;
        let addr = endpoint.addr();
        let ticket = IrohRoomTicket { addr, secret }.encode()?;

        let meta = RoomMeta {
            v: 1,
            epoch,
            host_id: host_id.clone(),
            host_name: host_name.clone(),
            code: ticket.clone(),
            scope: RoomScope::Internet,
            queue: Vec::new(),
            max_quality: None,
        };

        let host_peer = WirePeer {
            identity: host_id.clone(),
            name: host_name,
            is_host: true,
            can_host: Some(true),
            sharing: false,
            has_audio: false,
        };

        let shared = Arc::new(IrohHostShared {
            secret,
            host_id,
            meta: RwLock::new(meta.clone()),
            host_peer: RwLock::new(host_peer),
            remotes: RwLock::new(HashMap::new()),
            handoff_waiter: StdMutex::new(None),
            local_events: events_tx,
        });
        shared.broadcast_state().await;

        let ep_clone = endpoint.clone();
        let shared_clone = shared.clone();
        let accept_task = tokio::spawn(async move {
            while let Some(incoming) = ep_clone.accept().await {
                let shared_peer = shared_clone.clone();
                tokio::spawn(async move {
                    let Ok(conn) = incoming.await else {
                        return;
                    };
                    handle_iroh_peer_connection(shared_peer, conn).await;
                });
            }
        });

        Ok(Self {
            endpoint,
            shared,
            accept_task,
            ticket,
            meta,
        })
    }

    pub async fn set_host_sharing(&self, sharing: bool, has_audio: bool) {
        {
            let mut hp = self.shared.host_peer.write().await;
            hp.sharing = sharing;
            hp.has_audio = has_audio;
        }
        self.shared.broadcast_state().await;
    }

    pub async fn send_video_packet(&self, packet: Vec<u8>, is_keyframe: bool) {
        self.shared
            .fanout_video(&self.shared.host_id, Arc::new(packet), is_keyframe)
            .await;
    }

    pub async fn send_audio_packet(&self, packet: Vec<u8>) {
        self.shared
            .fanout_audio(&self.shared.host_id, Arc::new(packet))
            .await;
    }

    /// Transfere o host para o próximo participante da fila antes de encerrar.
    /// Se não houver ninguém (ou ninguém aceitar em 5s), encerra a sala com `RoomEnd`.
    pub async fn hand_off_and_close(&self) {
        let (candidates, next_epoch) = {
            let meta = self.shared.meta.read().await;
            (meta.queue.clone(), meta.epoch + 1)
        };

        let mut handed_off = false;
        for cand_id in candidates {
            let ctrl_tx = {
                let remotes = self.shared.remotes.read().await;
                remotes.get(&cand_id).map(|p| p.ctrl_tx.clone())
            };
            let Some(ctrl_tx) = ctrl_tx else {
                continue;
            };

            let (tx, rx) = oneshot::channel();
            if let Ok(mut w) = self.shared.handoff_waiter.lock() {
                *w = Some((cand_id.clone(), tx));
            }

            let handoff_msg = RoomWireMessage::Handoff {
                to: cand_id.clone(),
                epoch: next_epoch,
            };
            // Avisa o candidato e todos os demais que o host está trocando
            let _ = ctrl_tx.send(handoff_msg.clone()).await;
            {
                let remotes = self.shared.remotes.read().await;
                for (id, peer) in remotes.iter() {
                    if id != &cand_id {
                        let _ = peer.ctrl_tx.try_send(handoff_msg.clone());
                    }
                }
            }

            if let Ok(Ok((new_id, new_name, new_ticket))) =
                tokio::time::timeout(Duration::from_secs(6), rx).await
            {
                let announce = RoomWireMessage::NewHost {
                    epoch: next_epoch,
                    host_id: new_id,
                    host_name: new_name,
                    ticket: new_ticket,
                };
                let senders: Vec<mpsc::Sender<RoomWireMessage>> = {
                    let remotes = self.shared.remotes.read().await;
                    remotes.values().map(|p| p.ctrl_tx.clone()).collect()
                };
                for s in senders {
                    let _ = s.send(announce.clone()).await;
                }
                // Tempo para os pacotes QUIC chegarem a todos os participantes
                tokio::time::sleep(Duration::from_millis(450)).await;
                handed_off = true;
                break;
            }
        }

        if let Ok(mut w) = self.shared.handoff_waiter.lock() {
            *w = None;
        }

        if !handed_off {
            let msg = RoomWireMessage::RoomEnd {
                reason: "O host encerrou a sala.".to_string(),
            };
            let senders: Vec<mpsc::Sender<RoomWireMessage>> = {
                let remotes = self.shared.remotes.read().await;
                remotes.values().map(|p| p.ctrl_tx.clone()).collect()
            };
            for s in senders {
                let _ = s.send(msg.clone()).await;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        {
            let remotes = self.shared.remotes.read().await;
            for p in remotes.values() {
                p.video_box.close();
            }
        }
        self.accept_task.abort();
        self.endpoint.close().await;
    }
}

async fn handle_iroh_peer_connection(shared: Arc<IrohHostShared>, conn: Connection) {
    let Ok((mut send_bi, mut recv_bi)) = conn.accept_bi().await else {
        return;
    };

    let Ok(Some(RoomWireMessage::Hello {
        identity,
        name,
        secret,
    })) = read_framed_json(&mut recv_bi).await
    else {
        return;
    };

    if secret != shared.secret || identity.is_empty() {
        let _ = write_framed_json(
            &mut send_bi,
            &RoomWireMessage::RoomEnd {
                reason: "Código incorreto ou sala encerrada.".to_string(),
            },
        )
        .await;
        return;
    }

    let (ctrl_tx, mut ctrl_rx) = mpsc::channel::<RoomWireMessage>(32);
    let video_box = VideoMailbox::new();
    let (audio_tx, audio_rx) = mpsc::channel::<Arc<Vec<u8>>>(16);

    {
        let mut remotes = shared.remotes.write().await;
        if remotes.len() + 1 >= MAX_PARTICIPANTS && !remotes.contains_key(&identity) {
            let _ = write_framed_json(
                &mut send_bi,
                &RoomWireMessage::RoomEnd {
                    reason: "A sala está cheia (máximo de 10 pessoas).".to_string(),
                },
            )
            .await;
            return;
        }
        remotes.insert(
            identity.clone(),
            IrohRemotePeer {
                info: WirePeer {
                    identity: identity.clone(),
                    name,
                    is_host: false,
                    // No Iroh P2P todos podem assumir como host (sem precisar abrir portas!)
                    can_host: Some(true),
                    sharing: false,
                    has_audio: false,
                },
                ctrl_tx,
                video_box: video_box.clone(),
                audio_tx,
            },
        );
    }

    {
        let mut meta = shared.meta.write().await;
        if !meta.queue.iter().any(|id| id == &identity) {
            meta.queue.push(identity.clone());
        }
    }

    shared.broadcast_state().await;
    shared.request_keyframes().await;

    let ctrl_writer = tokio::spawn(async move {
        while let Some(msg) = ctrl_rx.recv().await {
            if write_framed_json(&mut send_bi, &msg).await.is_err() {
                break;
            }
        }
    });
    let video_writer = spawn_video_uni_sender(conn.clone(), video_box.clone());
    let audio_writer = spawn_audio_uni_sender(conn.clone(), audio_rx);

    let shared_uni = shared.clone();
    let conn_uni = conn.clone();
    let peer_id_uni = identity.clone();
    let uni_reader = tokio::spawn(async move {
        let mut child_tasks = Vec::new();
        while let Ok(mut recv_uni) = conn_uni.accept_uni().await {
            let shared_inner = shared_uni.clone();
            let sender_id = peer_id_uni.clone();
            let t = tokio::spawn(async move {
                let mut kind = [0u8; 1];
                if recv_uni.read_exact(&mut kind).await.is_err() {
                    return;
                }
                match kind[0] {
                    STREAM_KIND_VIDEO => {
                        let mut canvases: HashMap<String, RemoteVideoCanvas> = HashMap::new();
                        while let Ok(Some(bytes)) =
                            read_framed_bytes(&mut recv_uni, 4 * 1024 * 1024).await
                        {
                            if let Some((id, hdr, jpeg)) = decode_video_packet(&bytes) {
                                let is_key = hdr.is_keyframe;
                                let arc_bytes = Arc::new(bytes.clone());
                                shared_inner
                                    .fanout_video(&sender_id, arc_bytes, is_key)
                                    .await;
                                match apply_video_to_canvases(&mut canvases, id, hdr, jpeg) {
                                    (fid, CanvasUpdate::Frame(frame)) => {
                                        let _ = shared_inner.local_events.try_send(
                                            TransportEvent::VideoFrame {
                                                identity: fid,
                                                frame,
                                            },
                                        );
                                    }
                                    (_, CanvasUpdate::NeedKeyframe) => {
                                        shared_inner.request_keyframes().await;
                                    }
                                    (_, CanvasUpdate::Ignored) => {}
                                }
                            }
                        }
                    }
                    STREAM_KIND_AUDIO => {
                        while let Ok(Some(bytes)) =
                            read_framed_bytes(&mut recv_uni, 256 * 1024).await
                        {
                            let arc_bytes = Arc::new(bytes);
                            shared_inner
                                .fanout_audio(&sender_id, arc_bytes.clone())
                                .await;
                            if let Some((id, hdr, payload)) = decode_audio_packet(&arc_bytes)
                                && let Some(samples) = decode_audio_payload(hdr, payload)
                            {
                                let _ = shared_inner.local_events.try_send(
                                    TransportEvent::AudioSamples {
                                        identity: id,
                                        samples,
                                    },
                                );
                            }
                        }
                    }
                    _ => {}
                }
            });
            child_tasks.push(t);
        }
        for t in child_tasks {
            t.abort();
        }
    });

    while let Ok(Some(msg)) = read_framed_json(&mut recv_bi).await {
        match msg {
            RoomWireMessage::ShareState { sharing, has_audio } => {
                if let Some(p) = shared.remotes.write().await.get_mut(&identity) {
                    p.info.sharing = sharing;
                    p.info.has_audio = has_audio;
                }
                shared.broadcast_state().await;
            }
            RoomWireMessage::RequestKeyframe => {
                shared.request_keyframes().await;
            }
            RoomWireMessage::NewHost {
                host_id,
                host_name,
                ticket,
                ..
            } => {
                if host_id == identity
                    && let Ok(mut w) = shared.handoff_waiter.lock()
                    && w.as_ref().is_some_and(|(cand, _)| cand == &identity)
                    && let Some((_, tx)) = w.take()
                {
                    let _ = tx.send((host_id, host_name, ticket));
                }
            }
            _ => {}
        }
    }

    video_box.close();
    ctrl_writer.abort();
    video_writer.abort();
    audio_writer.abort();
    uni_reader.abort();
    shared.remotes.write().await.remove(&identity);
    shared.meta.write().await.queue.retain(|id| id != &identity);
    shared.broadcast_state().await;
}

// ============================================================================
// Unified Pure Iroh Transport (Host ou Client)
// ============================================================================

pub enum RoomTransport {
    Host(IrohRoomHost),
    Client {
        endpoint: Endpoint,
        ctrl_tx: mpsc::Sender<RoomWireMessage>,
        video_box: VideoMailbox,
        audio_tx: mpsc::Sender<Arc<Vec<u8>>>,
        tasks: Vec<JoinHandle<()>>,
    },
}

impl RoomTransport {
    pub async fn connect_client(
        ticket: IrohRoomTicket,
        identity: String,
        name: String,
        events_tx: mpsc::Sender<TransportEvent>,
    ) -> Result<Self, String> {
        let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
            .bind()
            .await
            .map_err(|e| format!("Falha ao iniciar endpoint Iroh: {e}"))?;

        let conn = tokio::time::timeout(
            Duration::from_secs(12),
            endpoint.connect(ticket.addr, IROH_ALPN),
        )
        .await
        .map_err(|_| "Tempo esgotado ao conectar no host P2P.".to_string())?
        .map_err(|e| format!("Não consegui conectar no host P2P: {e}"))?;

        let (mut send_bi, mut recv_bi) = conn
            .open_bi()
            .await
            .map_err(|e| format!("Falha ao abrir canal de controle P2P: {e}"))?;

        write_framed_json(
            &mut send_bi,
            &RoomWireMessage::Hello {
                identity,
                name,
                secret: ticket.secret,
            },
        )
        .await
        .map_err(|e| format!("Falha ao autenticar na sala P2P: {e}"))?;

        let (ctrl_tx, mut ctrl_rx) = mpsc::channel::<RoomWireMessage>(32);
        let video_box = VideoMailbox::new();
        let (audio_tx, audio_rx) = mpsc::channel::<Arc<Vec<u8>>>(16);

        let t_write = tokio::spawn(async move {
            while let Some(msg) = ctrl_rx.recv().await {
                if write_framed_json(&mut send_bi, &msg).await.is_err() {
                    break;
                }
            }
        });
        let t_vid_tx = spawn_video_uni_sender(conn.clone(), video_box.clone());
        let t_aud_tx = spawn_audio_uni_sender(conn.clone(), audio_rx);

        let ev_ctrl = events_tx.clone();
        let t_read = tokio::spawn(async move {
            while let Ok(Some(msg)) = read_framed_json(&mut recv_bi).await {
                match msg {
                    RoomWireMessage::RoomState { meta, peers } => {
                        let _ = ev_ctrl.try_send(TransportEvent::RoomState { meta, peers });
                    }
                    RoomWireMessage::RequestKeyframe => {
                        let _ = ev_ctrl.try_send(TransportEvent::RequestKeyframe);
                    }
                    RoomWireMessage::Handoff { to, epoch } => {
                        let _ = ev_ctrl.try_send(TransportEvent::Handoff { to, epoch });
                    }
                    RoomWireMessage::NewHost {
                        epoch,
                        host_id,
                        host_name,
                        ticket,
                    } => {
                        let _ = ev_ctrl.try_send(TransportEvent::NewHost {
                            epoch,
                            host_id,
                            host_name,
                            ticket,
                        });
                    }
                    RoomWireMessage::RoomEnd { reason } => {
                        let _ = ev_ctrl.try_send(TransportEvent::RoomEnd { reason });
                        return;
                    }
                    _ => {}
                }
            }
            let _ = ev_ctrl.try_send(TransportEvent::Disconnected);
        });

        let conn_uni = conn.clone();
        let ev_uni = events_tx;
        let ctrl_for_kf = ctrl_tx.clone();
        let t_uni = tokio::spawn(async move {
            let mut child_tasks = Vec::new();
            while let Ok(mut recv_uni) = conn_uni.accept_uni().await {
                let ev = ev_uni.clone();
                let ctrl_kf = ctrl_for_kf.clone();
                let t = tokio::spawn(async move {
                    let mut kind = [0u8; 1];
                    if recv_uni.read_exact(&mut kind).await.is_err() {
                        return;
                    }
                    match kind[0] {
                        STREAM_KIND_VIDEO => {
                            let mut canvases: HashMap<String, RemoteVideoCanvas> = HashMap::new();
                            while let Ok(Some(bytes)) =
                                read_framed_bytes(&mut recv_uni, 4 * 1024 * 1024).await
                            {
                                if let Some((id, hdr, jpeg)) = decode_video_packet(&bytes) {
                                    match apply_video_to_canvases(&mut canvases, id, hdr, jpeg) {
                                        (fid, CanvasUpdate::Frame(frame)) => {
                                            let _ = ev.try_send(TransportEvent::VideoFrame {
                                                identity: fid,
                                                frame,
                                            });
                                        }
                                        (_, CanvasUpdate::NeedKeyframe) => {
                                            let _ =
                                                ctrl_kf.try_send(RoomWireMessage::RequestKeyframe);
                                        }
                                        (_, CanvasUpdate::Ignored) => {}
                                    }
                                }
                            }
                        }
                        STREAM_KIND_AUDIO => {
                            while let Ok(Some(bytes)) =
                                read_framed_bytes(&mut recv_uni, 256 * 1024).await
                            {
                                if let Some((id, hdr, payload)) = decode_audio_packet(&bytes)
                                    && let Some(samples) = decode_audio_payload(hdr, payload)
                                {
                                    let _ = ev.try_send(TransportEvent::AudioSamples {
                                        identity: id,
                                        samples,
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                });
                child_tasks.push(t);
            }
            for t in child_tasks {
                t.abort();
            }
        });

        Ok(RoomTransport::Client {
            endpoint,
            ctrl_tx,
            video_box,
            audio_tx,
            tasks: vec![t_write, t_vid_tx, t_aud_tx, t_read, t_uni],
        })
    }

    pub async fn send_share_state(&self, sharing: bool, has_audio: bool) {
        match self {
            RoomTransport::Host(host) => host.set_host_sharing(sharing, has_audio).await,
            RoomTransport::Client { ctrl_tx, .. } => {
                let _ = ctrl_tx.try_send(RoomWireMessage::ShareState { sharing, has_audio });
            }
        }
    }

    pub async fn send_new_host(&self, epoch: u64, host_id: String, host_name: String, ticket: String) {
        if let RoomTransport::Client { ctrl_tx, .. } = self {
            let _ = ctrl_tx
                .send(RoomWireMessage::NewHost {
                    epoch,
                    host_id,
                    host_name,
                    ticket,
                })
                .await;
        }
    }

    pub async fn send_video_packet(&self, packet: Vec<u8>, is_keyframe: bool) {
        match self {
            RoomTransport::Host(host) => host.send_video_packet(packet, is_keyframe).await,
            RoomTransport::Client { video_box, .. } => {
                video_box.push(Arc::new(packet), is_keyframe);
            }
        }
    }

    pub async fn send_audio_packet(&self, packet: Vec<u8>) {
        match self {
            RoomTransport::Host(host) => host.send_audio_packet(packet).await,
            RoomTransport::Client { audio_tx, .. } => {
                let _ = audio_tx.try_send(Arc::new(packet));
            }
        }
    }

    pub async fn close(self, hand_off: bool) {
        match self {
            RoomTransport::Host(host) => {
                if hand_off {
                    host.hand_off_and_close().await;
                } else {
                    host.endpoint.close().await;
                }
            }
            RoomTransport::Client {
                endpoint,
                video_box,
                tasks,
                ..
            } => {
                video_box.close();
                for t in tasks {
                    t.abort();
                }
                endpoint.close().await;
            }
        }
    }
}
