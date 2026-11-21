//! Transporte em tempo real das salas:
//!  - **P2P (Iroh)**: QUIC P2P com NAT traversal + relay automático (`IROH-<ticket>`), sem abrir portas.
//!  - **Clássico (WebSocket)**: cliente `/ws/room` na porta 7880 para salas LiveKit (compatível com Electron).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ct_core::protocol::{
    MAX_PARTICIPANTS, QualityId, RoomMeta, RoomScope, RoomWireMessage, WirePeer,
    decode_video_packet,
};
use futures_util::{SinkExt, StreamExt};
use iroh::endpoint::Connection;
use iroh::{Endpoint, EndpointAddr};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{RwLock, mpsc};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message as WsMsg;

use crate::screen::{CapturedVideoFrame, decode_frame_jpeg};

pub const IROH_ALPN: &[u8] = b"compartilhartela/p2p/1";
pub const IROH_TICKET_PREFIX: &str = "IROH-";

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
            .ok_or_else(|| "Ticket Iroh inválido.".to_string())?;
        let bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| "Ticket Iroh corrompido (base64 inválido).".to_string())?;
        serde_json::from_slice(&bytes)
            .map_err(|_| "Ticket Iroh inválido ou incompatível.".to_string())
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
    RoomEnd {
        reason: String,
    },
    Disconnected,
}

async fn write_framed_json<W: AsyncWriteExt + Unpin>(
    w: &mut W,
    msg: &RoomWireMessage,
) -> Result<(), std::io::Error> {
    let data = serde_json::to_vec(msg).unwrap_or_default();
    let len = (data.len() as u32).to_le_bytes();
    w.write_all(&len).await?;
    w.write_all(&data).await?;
    w.flush().await
}

async fn read_framed_json<R: AsyncReadExt + Unpin>(
    r: &mut R,
) -> Result<Option<RoomWireMessage>, std::io::Error> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > 512 * 1024 {
        return Ok(None);
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(serde_json::from_slice(&buf).ok())
}

// ============================================================================
// Iroh P2P Host
// ============================================================================

struct IrohRemotePeer {
    info: WirePeer,
    conn: Connection,
    ctrl_tx: mpsc::Sender<RoomWireMessage>,
}

struct IrohHostShared {
    secret: u32,
    host_id: String,
    meta: RwLock<RoomMeta>,
    host_peer: RwLock<WirePeer>,
    remotes: RwLock<HashMap<String, IrohRemotePeer>>,
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

    async fn fanout_video(&self, sender_id: &str, packet: Arc<Vec<u8>>) {
        let conns: Vec<Connection> = {
            let remotes = self.remotes.read().await;
            remotes
                .iter()
                .filter(|(id, _)| id.as_str() != sender_id)
                .map(|(_, p)| p.conn.clone())
                .collect()
        };
        for conn in conns {
            let data = packet.clone();
            tokio::spawn(async move {
                if let Ok(mut send) = conn.open_uni().await {
                    let _ = send.write_all(&data).await;
                    let _ = send.finish();
                }
            });
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
            epoch: 1,
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

    pub async fn set_max_quality(&self, quality: Option<QualityId>) {
        self.shared.meta.write().await.max_quality = quality;
        self.shared.broadcast_state().await;
    }

    pub async fn send_video_packet(&self, packet: Vec<u8>) {
        self.shared
            .fanout_video(&self.shared.host_id, Arc::new(packet))
            .await;
    }

    pub async fn close(&self, reason: &str) {
        let msg = RoomWireMessage::RoomEnd {
            reason: reason.to_string(),
        };
        {
            let remotes = self.shared.remotes.read().await;
            for p in remotes.values() {
                let _ = p.ctrl_tx.try_send(msg.clone());
            }
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
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
                    can_host: Some(false),
                    sharing: false,
                    has_audio: false,
                },
                conn: conn.clone(),
                ctrl_tx,
            },
        );
    }

    shared.broadcast_state().await;

    let writer = tokio::spawn(async move {
        while let Some(msg) = ctrl_rx.recv().await {
            if write_framed_json(&mut send_bi, &msg).await.is_err() {
                break;
            }
        }
    });

    // Leitura de frames de vídeo enviados por este peer (uni streams)
    let shared_uni = shared.clone();
    let conn_uni = conn.clone();
    let peer_id_uni = identity.clone();
    let uni_reader = tokio::spawn(async move {
        while let Ok(mut recv_uni) = conn_uni.accept_uni().await {
            let shared_inner = shared_uni.clone();
            let sender_id = peer_id_uni.clone();
            tokio::spawn(async move {
                if let Ok(bytes) = recv_uni.read_to_end(4 * 1024 * 1024).await
                    && let Some((id, _w, _h, seq, jpeg)) = decode_video_packet(&bytes)
                {
                    if let Some(frame) = decode_frame_jpeg(jpeg, seq) {
                        let _ = shared_inner
                            .local_events
                            .try_send(TransportEvent::VideoFrame { identity: id, frame });
                    }
                    shared_inner.fanout_video(&sender_id, Arc::new(bytes)).await;
                }
            });
        }
    });

    // Leitura de mensagens de controle deste peer
    while let Ok(Some(msg)) = read_framed_json(&mut recv_bi).await {
        if let RoomWireMessage::ShareState { sharing, has_audio } = msg {
            if let Some(p) = shared.remotes.write().await.get_mut(&identity) {
                p.info.sharing = sharing;
                p.info.has_audio = has_audio;
            }
            shared.broadcast_state().await;
        }
    }

    writer.abort();
    uni_reader.abort();
    shared.remotes.write().await.remove(&identity);
    shared.broadcast_state().await;
}

// ============================================================================
// Unified RoomTransport (Iroh Host / Iroh Client / Classic WebSocket Client)
// ============================================================================

pub enum RoomTransport {
    IrohHost(IrohRoomHost),
    IrohClient {
        endpoint: Endpoint,
        conn: Connection,
        ctrl_tx: mpsc::Sender<RoomWireMessage>,
        tasks: Vec<JoinHandle<()>>,
    },
    ClassicWs {
        tx: mpsc::Sender<WsMsg>,
        tasks: Vec<JoinHandle<()>>,
    },
}

impl RoomTransport {
    pub async fn connect_iroh_client(
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
        .map_err(|_| "Tempo esgotado ao conectar no host P2P (Iroh).".to_string())?
        .map_err(|e| format!("Não consegui conectar no host P2P (Iroh): {e}"))?;

        let (mut send_bi, mut recv_bi) = conn
            .open_bi()
            .await
            .map_err(|e| format!("Falha ao abrir canal de controle Iroh: {e}"))?;

        write_framed_json(
            &mut send_bi,
            &RoomWireMessage::Hello {
                identity,
                name,
                secret: ticket.secret,
            },
        )
        .await
        .map_err(|e| format!("Falha ao autenticar na sala Iroh: {e}"))?;

        let (ctrl_tx, mut ctrl_rx) = mpsc::channel::<RoomWireMessage>(32);
        let t_write = tokio::spawn(async move {
            while let Some(msg) = ctrl_rx.recv().await {
                if write_framed_json(&mut send_bi, &msg).await.is_err() {
                    break;
                }
            }
        });

        let ev_ctrl = events_tx.clone();
        let t_read = tokio::spawn(async move {
            while let Ok(Some(msg)) = read_framed_json(&mut recv_bi).await {
                match msg {
                    RoomWireMessage::RoomState { meta, peers } => {
                        let _ = ev_ctrl.try_send(TransportEvent::RoomState { meta, peers });
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
        let t_uni = tokio::spawn(async move {
            while let Ok(mut recv_uni) = conn_uni.accept_uni().await {
                let ev = ev_uni.clone();
                tokio::spawn(async move {
                    if let Ok(bytes) = recv_uni.read_to_end(4 * 1024 * 1024).await
                        && let Some((id, _w, _h, seq, jpeg)) = decode_video_packet(&bytes)
                        && let Some(frame) = decode_frame_jpeg(jpeg, seq)
                    {
                        let _ = ev.try_send(TransportEvent::VideoFrame {
                            identity: id,
                            frame,
                        });
                    }
                });
            }
        });

        Ok(RoomTransport::IrohClient {
            endpoint,
            conn,
            ctrl_tx,
            tasks: vec![t_write, t_read, t_uni],
        })
    }

    pub async fn connect_classic_ws(
        ws_url: &str,
        identity: String,
        name: String,
        secret: u32,
        events_tx: mpsc::Sender<TransportEvent>,
    ) -> Result<Self, String> {
        let (ws_stream, _) = tokio::time::timeout(
            Duration::from_secs(6),
            tokio_tungstenite::connect_async(ws_url),
        )
        .await
        .map_err(|_| "Tempo esgotado ao conectar no WebSocket da sala.".to_string())?
        .map_err(|e| format!("Falha ao conectar no WebSocket da sala: {e}"))?;

        let (mut ws_tx, mut ws_rx) = ws_stream.split();
        let hello = serde_json::to_string(&RoomWireMessage::Hello {
            identity,
            name,
            secret,
        })
        .unwrap_or_default();
        ws_tx
            .send(WsMsg::Text(hello.into()))
            .await
            .map_err(|e| format!("Falha ao enviar Hello: {e}"))?;

        let (tx, mut rx) = mpsc::channel::<WsMsg>(16);
        let t_write = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if ws_tx.send(msg).await.is_err() {
                    break;
                }
            }
        });

        let t_read = tokio::spawn(async move {
            while let Some(Ok(msg)) = ws_rx.next().await {
                match msg {
                    WsMsg::Text(txt) => {
                        if let Ok(wire) = serde_json::from_str::<RoomWireMessage>(&txt) {
                            match wire {
                                RoomWireMessage::RoomState { meta, peers } => {
                                    let _ = events_tx
                                        .try_send(TransportEvent::RoomState { meta, peers });
                                }
                                RoomWireMessage::RoomEnd { reason } => {
                                    let _ = events_tx
                                        .try_send(TransportEvent::RoomEnd { reason });
                                    return;
                                }
                                _ => {}
                            }
                        }
                    }
                    WsMsg::Binary(bin) => {
                        if let Some((id, _w, _h, seq, jpeg)) = decode_video_packet(&bin)
                            && let Some(frame) = decode_frame_jpeg(jpeg, seq)
                        {
                            let _ = events_tx.try_send(TransportEvent::VideoFrame {
                                identity: id,
                                frame,
                            });
                        }
                    }
                    WsMsg::Close(_) => break,
                    _ => {}
                }
            }
            let _ = events_tx.try_send(TransportEvent::Disconnected);
        });

        Ok(RoomTransport::ClassicWs {
            tx,
            tasks: vec![t_write, t_read],
        })
    }

    pub async fn send_share_state(&self, sharing: bool, has_audio: bool) {
        match self {
            RoomTransport::IrohHost(host) => host.set_host_sharing(sharing, has_audio).await,
            RoomTransport::IrohClient { ctrl_tx, .. } => {
                let _ = ctrl_tx
                    .try_send(RoomWireMessage::ShareState { sharing, has_audio });
            }
            RoomTransport::ClassicWs { tx, .. } => {
                let txt = serde_json::to_string(&RoomWireMessage::ShareState { sharing, has_audio })
                    .unwrap_or_default();
                let _ = tx.try_send(WsMsg::Text(txt.into()));
            }
        }
    }

    pub async fn send_video_packet(&self, packet: Vec<u8>) {
        match self {
            RoomTransport::IrohHost(host) => host.send_video_packet(packet).await,
            RoomTransport::IrohClient { conn, .. } => {
                let c = conn.clone();
                tokio::spawn(async move {
                    if let Ok(mut send) = c.open_uni().await {
                        let _ = send.write_all(&packet).await;
                        let _ = send.finish();
                    }
                });
            }
            RoomTransport::ClassicWs { tx, .. } => {
                let _ = tx.try_send(WsMsg::Binary(packet.into()));
            }
        }
    }

    pub async fn set_max_quality(&self, quality: Option<QualityId>) {
        if let RoomTransport::IrohHost(host) = self {
            host.set_max_quality(quality).await;
        }
    }

    pub async fn close(self, reason: &str) {
        match self {
            RoomTransport::IrohHost(host) => host.close(reason).await,
            RoomTransport::IrohClient {
                endpoint, tasks, ..
            } => {
                for t in tasks {
                    t.abort();
                }
                endpoint.close().await;
            }
            RoomTransport::ClassicWs { tasks, .. } => {
                for t in tasks {
                    t.abort();
                }
            }
        }
    }
}
