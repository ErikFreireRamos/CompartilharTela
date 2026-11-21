use serde::{Deserialize, Serialize};

/// TCP: API de entrada, teste de alcance e proxy do LiveKit.
pub const PUBLIC_PORT: u16 = 7880;
/// TCP: mídia WebRTC (fallback quando UDP não passa).
pub const RTC_TCP_PORT: u16 = 7881;
/// UDP: mídia WebRTC (porta única).
pub const RTC_UDP_PORT: u16 = 7882;
/// UDP broadcast para achar o host na mesma rede local.
pub const DISCOVERY_PORT: u16 = 7883;
/// HTTP interno do livekit-server.exe, só em 127.0.0.1.
pub const LIVEKIT_INTERNAL_PORT: u16 = 7890;

pub const ROOM_NAME: &str = "sala";
pub const MAX_PARTICIPANTS: usize = 10;
pub const CONTROL_TOPIC: &str = "ctl";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QualityId {
    #[serde(rename = "480p30")]
    P480F30,
    #[serde(rename = "480p60")]
    P480F60,
    #[serde(rename = "720p30")]
    P720F30,
    #[serde(rename = "720p60")]
    P720F60,
    #[serde(rename = "1080p30")]
    P1080F30,
    #[serde(rename = "1080p60")]
    P1080F60,
    #[serde(rename = "1440p60")]
    P1440F60,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Motion,
    Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualityPreset {
    pub id: QualityId,
    pub label: &'static str,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// bits/s da camada principal
    pub bitrate: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimulcastLayer {
    pub width: u32,
    pub height: u32,
    pub bitrate: u64,
    pub fps: u32,
}

pub const QUALITY_PRESETS: &[QualityPreset] = &[
    QualityPreset { id: QualityId::P480F30, label: "480p · 30 fps", width: 854, height: 480, fps: 30, bitrate: 1_000_000 },
    QualityPreset { id: QualityId::P480F60, label: "480p · 60 fps", width: 854, height: 480, fps: 60, bitrate: 1_500_000 },
    QualityPreset { id: QualityId::P720F30, label: "720p · 30 fps", width: 1280, height: 720, fps: 30, bitrate: 2_000_000 },
    QualityPreset { id: QualityId::P720F60, label: "720p · 60 fps", width: 1280, height: 720, fps: 60, bitrate: 3_000_000 },
    QualityPreset { id: QualityId::P1080F30, label: "1080p · 30 fps", width: 1920, height: 1080, fps: 30, bitrate: 4_000_000 },
    QualityPreset { id: QualityId::P1080F60, label: "1080p · 60 fps", width: 1920, height: 1080, fps: 60, bitrate: 6_000_000 },
    QualityPreset { id: QualityId::P1440F60, label: "1440p · 60 fps", width: 2560, height: 1440, fps: 60, bitrate: 10_000_000 },
];

pub fn preset_by_id(id: QualityId) -> QualityPreset {
    QUALITY_PRESETS
        .iter()
        .copied()
        .find(|p| p.id == id)
        .unwrap_or(QUALITY_PRESETS[4])
}

pub fn allowed_presets(max: Option<QualityId>) -> &'static [QualityPreset] {
    match max {
        None => QUALITY_PRESETS,
        Some(limit_id) => {
            let idx = QUALITY_PRESETS
                .iter()
                .position(|p| p.id == limit_id)
                .unwrap_or(QUALITY_PRESETS.len() - 1);
            &QUALITY_PRESETS[..=idx]
        }
    }
}

pub fn clamp_quality(id: QualityId, max: Option<QualityId>) -> QualityId {
    let allowed = allowed_presets(max);
    if allowed.iter().any(|p| p.id == id) {
        id
    } else {
        allowed.last().map(|p| p.id).unwrap_or(QualityId::P1080F30)
    }
}

pub fn simulcast_layers(preset: QualityPreset) -> Vec<SimulcastLayer> {
    if preset.height <= 480 {
        return vec![SimulcastLayer {
            width: 480,
            height: 270,
            bitrate: 200_000,
            fps: 15,
        }];
    }
    let mut layers = vec![SimulcastLayer {
        width: 640,
        height: 360,
        bitrate: 400_000,
        fps: 15,
    }];
    if preset.height >= 1080 {
        layers.push(SimulcastLayer {
            width: 1280,
            height: 720,
            bitrate: 1_500_000,
            fps: 30,
        });
    }
    layers
}

/// Upload aproximado (Mbps) que o host precisa: 9 pessoas vendo, cada uma com 1 tela grande.
pub fn host_upload_mbps(preset: QualityPreset, streamers: usize) -> u32 {
    let thumbs = streamers.saturating_sub(1) as f64 * 0.4;
    let total = 9.0 * (preset.bitrate as f64 / 1_000_000.0 + thumbs + 0.13 * streamers as f64);
    total.round() as u32
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoomScope {
    Internet,
    Lan,
}

/// Metadados da sala, mantidos pelo host atual.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomMeta {
    pub v: u32,
    pub epoch: u64,
    pub host_id: String,
    pub host_name: String,
    pub code: String,
    pub scope: RoomScope,
    pub queue: Vec<String>,
    pub max_quality: Option<QualityId>,
}

/// Atributos que o host grava em cada participante.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerAttributes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum ControlMessage {
    #[serde(rename = "handoff")]
    Handoff { to: String, epoch: u64 },
    #[serde(rename = "new-host", rename_all = "camelCase")]
    NewHost {
        epoch: u64,
        host_id: String,
        host_name: String,
        endpoint: String,
        code: String,
    },
    #[serde(rename = "room-end")]
    RoomEnd { reason: String },
    #[serde(rename = "probe-ready", rename_all = "camelCase")]
    ProbeReady {
        id: String,
        public_ip: Option<String>,
        listening: bool,
    },
}

pub fn parse_room_meta(raw: Option<&str>) -> Option<RoomMeta> {
    let raw = raw?;
    if raw.is_empty() {
        return None;
    }
    let meta: RoomMeta = serde_json::from_str(raw).ok()?;
    (meta.v == 1).then_some(meta)
}

/// Modo de criação da sala escolhido pelo usuário.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoomMode {
    /// P2P via Iroh (QUIC com NAT traversal + relay automático; sem abrir portas).
    Iroh,
    /// Clássico via LiveKit (compatível com o app Electron antigo).
    Classic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WirePeer {
    pub identity: String,
    pub name: String,
    pub is_host: bool,
    pub can_host: Option<bool>,
    pub sharing: bool,
    pub has_audio: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum RoomWireMessage {
    Hello {
        identity: String,
        name: String,
        secret: u32,
    },
    RoomState {
        meta: RoomMeta,
        peers: Vec<WirePeer>,
    },
    ShareState {
        sharing: bool,
        has_audio: bool,
    },
    RoomEnd {
        reason: String,
    },
}

pub const WIRE_FRAME_VIDEO: u8 = 1;

/// Empacota um frame JPEG para envio binário (Iroh QUIC / WebSocket).
pub fn encode_video_packet(identity: &str, width: u32, height: u32, seq: u64, jpeg: &[u8]) -> Vec<u8> {
    let id_bytes = identity.as_bytes();
    let id_len = (id_bytes.len().min(u16::MAX as usize)) as u16;
    let mut out = Vec::with_capacity(1 + 2 + id_len as usize + 4 + 4 + 8 + jpeg.len());
    out.push(WIRE_FRAME_VIDEO);
    out.extend_from_slice(&id_len.to_le_bytes());
    out.extend_from_slice(&id_bytes[..id_len as usize]);
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(jpeg);
    out
}

/// Desempacota um frame JPEG recebido pela rede.
pub fn decode_video_packet(data: &[u8]) -> Option<(String, u32, u32, u64, &[u8])> {
    if data.len() < 19 || data[0] != WIRE_FRAME_VIDEO {
        return None;
    }
    let id_len = u16::from_le_bytes([data[1], data[2]]) as usize;
    let header_end = 3 + id_len + 16;
    if data.len() < header_end {
        return None;
    }
    let identity = std::str::from_utf8(&data[3..3 + id_len]).ok()?.to_string();
    let off = 3 + id_len;
    let width = u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    let height = u32::from_le_bytes([data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);
    let seq = u64::from_le_bytes([
        data[off + 8],
        data[off + 9],
        data[off + 10],
        data[off + 11],
        data[off + 12],
        data[off + 13],
        data[off + 14],
        data[off + 15],
    ]);
    Some((identity, width, height, seq, &data[header_end..]))
}

