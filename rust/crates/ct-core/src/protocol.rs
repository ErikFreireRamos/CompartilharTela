use serde::{Deserialize, Serialize};

pub const MAX_PARTICIPANTS: usize = 10;

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

/// Resolução independente escolhida por quem transmite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResolutionId {
    P480,
    P720,
    P1080,
    P1440,
}

pub const RESOLUTIONS: &[ResolutionId] = &[
    ResolutionId::P480,
    ResolutionId::P720,
    ResolutionId::P1080,
    ResolutionId::P1440,
];

pub const FPS_OPTIONS: &[u32] = &[15, 30, 60];

/// Qualidade recomendada para melhor equilíbrio entre nitidez e estabilidade no P2P.
pub const RECOMMENDED_RESOLUTION: ResolutionId = ResolutionId::P720;
pub const RECOMMENDED_FPS: u32 = 30;
pub const RECOMMENDED_AUDIO_QUALITY: AudioQualityId = AudioQualityId::Stable;

impl ResolutionId {
    pub fn dimensions(self) -> (u32, u32) {
        match self {
            ResolutionId::P480 => (854, 480),
            ResolutionId::P720 => (1280, 720),
            ResolutionId::P1080 => (1920, 1080),
            ResolutionId::P1440 => (2560, 1440),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ResolutionId::P480 => "480p",
            ResolutionId::P720 => "720p",
            ResolutionId::P1080 => "1080p",
            ResolutionId::P1440 => "1440p",
        }
    }

    pub fn from_quality(q: QualityId) -> Self {
        match q {
            QualityId::P480F30 | QualityId::P480F60 => ResolutionId::P480,
            QualityId::P720F30 | QualityId::P720F60 => ResolutionId::P720,
            QualityId::P1080F30 | QualityId::P1080F60 => ResolutionId::P1080,
            QualityId::P1440F60 => ResolutionId::P1440,
        }
    }

    pub fn to_quality_id(self, fps: u32) -> QualityId {
        match (self, fps >= 45) {
            (ResolutionId::P480, false) => QualityId::P480F30,
            (ResolutionId::P480, true) => QualityId::P480F60,
            (ResolutionId::P720, false) => QualityId::P720F30,
            (ResolutionId::P720, true) => QualityId::P720F60,
            (ResolutionId::P1080, false) => QualityId::P1080F30,
            (ResolutionId::P1080, true) => QualityId::P1080F60,
            (ResolutionId::P1440, _) => QualityId::P1440F60,
        }
    }
}

pub fn fps_from_quality(q: QualityId) -> u32 {
    match q {
        QualityId::P480F30 | QualityId::P720F30 | QualityId::P1080F30 => 30,
        QualityId::P480F60 | QualityId::P720F60 | QualityId::P1080F60 | QualityId::P1440F60 => 60,
    }
}

/// Qualidade de áudio selecionável por quem transmite (também alterável ao vivo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AudioQualityId {
    /// 32 kHz estéreo IMA-ADPCM (~256 kbps) — recomendada para estabilidade e baixa latência.
    #[default]
    Stable,
    /// 48 kHz estéreo IMA-ADPCM (~384 kbps) — áudio nítido para jogos e vídeos.
    High,
    /// 48 kHz estéreo PCM 16-bit (~1.5 Mbps) — sem compressão (para conexões muito rápidas).
    Studio,
}

pub const AUDIO_QUALITIES: &[AudioQualityId] = &[
    AudioQualityId::Stable,
    AudioQualityId::High,
    AudioQualityId::Studio,
];

pub const AUDIO_CODEC_ADPCM: u8 = 0;
pub const AUDIO_CODEC_PCM16: u8 = 1;

impl AudioQualityId {
    pub fn label(self) -> &'static str {
        match self {
            AudioQualityId::Stable => "Estável (Recomendado)",
            AudioQualityId::High => "Alta (48 kHz)",
            AudioQualityId::Studio => "Estúdio (Sem perdas)",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            AudioQualityId::Stable => "Áudio: Estável",
            AudioQualityId::High => "Áudio: Alta",
            AudioQualityId::Studio => "Áudio: Estúdio",
        }
    }

    pub fn sample_rate(self) -> u32 {
        match self {
            AudioQualityId::Stable => 32_000,
            AudioQualityId::High | AudioQualityId::Studio => 48_000,
        }
    }

    pub fn codec_id(self) -> u8 {
        match self {
            AudioQualityId::Stable | AudioQualityId::High => AUDIO_CODEC_ADPCM,
            AudioQualityId::Studio => AUDIO_CODEC_PCM16,
        }
    }
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
    /// Participantes aptos a virar host se o host atual sair (em ordem de entrada).
    pub queue: Vec<String>,
    pub max_quality: Option<QualityId>,
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
    RequestKeyframe,
    /// Host atual saindo: solicita que o participante `to` assuma como novo host.
    Handoff {
        to: String,
        epoch: u64,
    },
    /// Novo host pronto: todos os participantes reconectam automaticamente neste ticket Iroh.
    #[serde(rename_all = "camelCase")]
    NewHost {
        epoch: u64,
        host_id: String,
        host_name: String,
        ticket: String,
    },
    RoomEnd {
        reason: String,
    },
}

pub const WIRE_FRAME_VIDEO: u8 = 1;
pub const WIRE_FRAME_AUDIO: u8 = 2;
pub const VIDEO_FLAG_KEYFRAME: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoPacketHeader {
    pub full_width: u32,
    pub full_height: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub seq: u64,
    pub is_keyframe: bool,
}

/// Empacota um frame JPEG (keyframe completo ou região alterada) para envio binário.
pub fn encode_video_packet(identity: &str, hdr: VideoPacketHeader, jpeg: &[u8]) -> Vec<u8> {
    let id_bytes = identity.as_bytes();
    let id_len = (id_bytes.len().min(u16::MAX as usize)) as u16;
    let mut out = Vec::with_capacity(1 + 2 + id_len as usize + 33 + jpeg.len());
    out.push(WIRE_FRAME_VIDEO);
    out.extend_from_slice(&id_len.to_le_bytes());
    out.extend_from_slice(&id_bytes[..id_len as usize]);
    out.extend_from_slice(&hdr.full_width.to_le_bytes());
    out.extend_from_slice(&hdr.full_height.to_le_bytes());
    out.extend_from_slice(&hdr.x.to_le_bytes());
    out.extend_from_slice(&hdr.y.to_le_bytes());
    out.extend_from_slice(&hdr.width.to_le_bytes());
    out.extend_from_slice(&hdr.height.to_le_bytes());
    out.extend_from_slice(&hdr.seq.to_le_bytes());
    out.push(if hdr.is_keyframe { VIDEO_FLAG_KEYFRAME } else { 0 });
    out.extend_from_slice(jpeg);
    out
}

/// Desempacota um frame JPEG recebido pela rede.
pub fn decode_video_packet(data: &[u8]) -> Option<(String, VideoPacketHeader, &[u8])> {
    if data.len() < 36 || data[0] != WIRE_FRAME_VIDEO {
        return None;
    }
    let id_len = u16::from_le_bytes([data[1], data[2]]) as usize;
    let header_end = 3 + id_len + 33;
    if data.len() < header_end {
        return None;
    }
    let identity = std::str::from_utf8(&data[3..3 + id_len]).ok()?.to_string();
    let off = 3 + id_len;
    let hdr = VideoPacketHeader {
        full_width: u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]),
        full_height: u32::from_le_bytes([data[off + 4], data[off + 5], data[off + 6], data[off + 7]]),
        x: u32::from_le_bytes([data[off + 8], data[off + 9], data[off + 10], data[off + 11]]),
        y: u32::from_le_bytes([data[off + 12], data[off + 13], data[off + 14], data[off + 15]]),
        width: u32::from_le_bytes([data[off + 16], data[off + 17], data[off + 18], data[off + 19]]),
        height: u32::from_le_bytes([data[off + 20], data[off + 21], data[off + 22], data[off + 23]]),
        seq: u64::from_le_bytes([
            data[off + 24],
            data[off + 25],
            data[off + 26],
            data[off + 27],
            data[off + 28],
            data[off + 29],
            data[off + 30],
            data[off + 31],
        ]),
        is_keyframe: (data[off + 32] & VIDEO_FLAG_KEYFRAME) != 0,
    };
    Some((identity, hdr, &data[header_end..]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioPacketHeader {
    pub seq: u64,
    pub sample_rate: u32,
    pub codec: u8,
}

/// Empacota um bloco de áudio (ADPCM ou PCM16) com sample_rate e codec para envio binário.
pub fn encode_audio_packet(identity: &str, hdr: AudioPacketHeader, payload: &[u8]) -> Vec<u8> {
    let id_bytes = identity.as_bytes();
    let id_len = (id_bytes.len().min(u16::MAX as usize)) as u16;
    let mut out = Vec::with_capacity(1 + 2 + id_len as usize + 13 + payload.len());
    out.push(WIRE_FRAME_AUDIO);
    out.extend_from_slice(&id_len.to_le_bytes());
    out.extend_from_slice(&id_bytes[..id_len as usize]);
    out.extend_from_slice(&hdr.seq.to_le_bytes());
    out.extend_from_slice(&hdr.sample_rate.to_le_bytes());
    out.push(hdr.codec);
    out.extend_from_slice(payload);
    out
}

/// Desempacota um bloco de áudio recebido pela rede.
pub fn decode_audio_packet(data: &[u8]) -> Option<(String, AudioPacketHeader, &[u8])> {
    if data.len() < 16 || data[0] != WIRE_FRAME_AUDIO {
        return None;
    }
    let id_len = u16::from_le_bytes([data[1], data[2]]) as usize;
    let header_end = 3 + id_len + 13;
    if data.len() < header_end {
        return None;
    }
    let identity = std::str::from_utf8(&data[3..3 + id_len]).ok()?.to_string();
    let off = 3 + id_len;
    let hdr = AudioPacketHeader {
        seq: u64::from_le_bytes([
            data[off],
            data[off + 1],
            data[off + 2],
            data[off + 3],
            data[off + 4],
            data[off + 5],
            data[off + 6],
            data[off + 7],
        ]),
        sample_rate: u32::from_le_bytes([
            data[off + 8],
            data[off + 9],
            data[off + 10],
            data[off + 11],
        ]),
        codec: data[off + 12],
    };
    Some((identity, hdr, &data[header_end..]))
}
