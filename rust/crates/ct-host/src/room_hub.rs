//! Hub WebSocket em tempo real para salas no modo Clássico (`/ws/room` na porta 7880),
//! permitindo que clientes Rust troquem lista de participantes e frames de vídeo ao vivo
//! enquanto clientes Electron antigos continuam conectando ao LiveKit na mesma porta.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use ct_core::protocol::{MAX_PARTICIPANTS, RoomMeta, RoomWireMessage, WirePeer, decode_video_packet};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{RwLock, mpsc};

struct ConnectedPeer {
    info: WirePeer,
    tx: mpsc::Sender<Message>,
}

pub struct ClassicRoomHub {
    secret: u32,
    meta: RwLock<RoomMeta>,
    peers: RwLock<HashMap<String, ConnectedPeer>>,
}

impl ClassicRoomHub {
    pub fn new(secret: u32, meta: RoomMeta) -> Arc<Self> {
        Arc::new(Self {
            secret,
            meta: RwLock::new(meta),
            peers: RwLock::new(HashMap::new()),
        })
    }

    pub async fn update_meta(&self, meta: RoomMeta) {
        *self.meta.write().await = meta;
        self.broadcast_state().await;
    }

    pub async fn register_host_peer(&self, identity: String, name: String) {
        let mut peers = self.peers.write().await;
        if let Some(p) = peers.get_mut(&identity) {
            p.info.name = name;
            p.info.is_host = true;
            return;
        }
        // Dummy channel for host if it hasn't connected via WS yet
        let (tx, _rx) = mpsc::channel(1);
        peers.insert(
            identity.clone(),
            ConnectedPeer {
                info: WirePeer {
                    identity,
                    name,
                    is_host: true,
                    can_host: Some(true),
                    sharing: false,
                    has_audio: false,
                },
                tx,
            },
        );
    }

    pub async fn close_room(&self, reason: &str) {
        let msg = serde_json::to_string(&RoomWireMessage::RoomEnd {
            reason: reason.to_string(),
        })
        .unwrap_or_default();
        let peers = self.peers.read().await;
        for peer in peers.values() {
            let _ = peer.tx.try_send(Message::Text(msg.clone().into()));
        }
    }

    async fn broadcast_state(&self) {
        let meta = self.meta.read().await.clone();
        let peers_guard = self.peers.read().await;
        let peer_list: Vec<WirePeer> = peers_guard.values().map(|p| p.info.clone()).collect();
        let msg = serde_json::to_string(&RoomWireMessage::RoomState {
            meta,
            peers: peer_list,
        })
        .unwrap_or_default();
        for peer in peers_guard.values() {
            let _ = peer.tx.try_send(Message::Text(msg.clone().into()));
        }
    }

    async fn fanout_video(&self, sender_id: &str, data: axum::body::Bytes) {
        let peers_guard = self.peers.read().await;
        for (id, peer) in peers_guard.iter() {
            if id != sender_id {
                // try_send descarta frame se o receptor estiver lento (evita fila infinita/lag)
                let _ = peer.tx.try_send(Message::Binary(data.clone()));
            }
        }
    }

    pub async fn handle_socket(self: Arc<Self>, ws: WebSocket) {
        let (mut ws_tx, mut ws_rx) = ws.split();

        // Aguarda mensagem Hello com segredo válido
        let Some(Ok(Message::Text(hello_txt))) = ws_rx.next().await else {
            return;
        };
        let Ok(RoomWireMessage::Hello {
            identity,
            name,
            secret,
        }) = serde_json::from_str::<RoomWireMessage>(&hello_txt)
        else {
            return;
        };
        if secret != self.secret || identity.is_empty() {
            let _ = ws_tx.close().await;
            return;
        }

        let (tx, mut rx) = mpsc::channel::<Message>(16);
        let is_host = self.meta.read().await.host_id == identity;
        {
            let mut peers = self.peers.write().await;
            if peers.len() >= MAX_PARTICIPANTS && !peers.contains_key(&identity) {
                let _ = ws_tx.close().await;
                return;
            }
            peers.insert(
                identity.clone(),
                ConnectedPeer {
                    info: WirePeer {
                        identity: identity.clone(),
                        name,
                        is_host,
                        can_host: is_host.then_some(true),
                        sharing: false,
                        has_audio: false,
                    },
                    tx,
                },
            );
        }

        self.broadcast_state().await;

        let writer = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if ws_tx.send(msg).await.is_err() {
                    break;
                }
            }
        });

        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                Message::Text(txt) => {
                    if let Ok(RoomWireMessage::ShareState { sharing, has_audio }) =
                        serde_json::from_str::<RoomWireMessage>(&txt)
                    {
                        if let Some(peer) = self.peers.write().await.get_mut(&identity) {
                            peer.info.sharing = sharing;
                            peer.info.has_audio = has_audio;
                        }
                        self.broadcast_state().await;
                    }
                }
                Message::Binary(bin) => {
                    if decode_video_packet(&bin).is_some() {
                        self.fanout_video(&identity, bin).await;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }

        writer.abort();
        self.peers.write().await.remove(&identity);
        self.broadcast_state().await;
    }
}
