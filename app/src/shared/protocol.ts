// Portas e mensagens trocadas entre os apps.

/** TCP: API de entrada, teste de alcance e proxy do LiveKit. */
export const PUBLIC_PORT = 7880
/** TCP: mídia WebRTC (fallback quando UDP não passa). */
export const RTC_TCP_PORT = 7881
/** UDP: mídia WebRTC (porta única). */
export const RTC_UDP_PORT = 7882
/** UDP broadcast para achar o host na mesma rede local. */
export const DISCOVERY_PORT = 7883
/** HTTP interno do livekit-server.exe, só em 127.0.0.1. */
export const LIVEKIT_INTERNAL_PORT = 7890

export const ROOM_NAME = 'sala'
export const MAX_PARTICIPANTS = 10

export type QualityId = '480p30' | '480p60' | '720p30' | '720p60' | '1080p30' | '1080p60' | '1440p60'

/** Metadados da sala, mantidos pelo host atual. */
export interface RoomMeta {
  v: 1
  epoch: number
  hostId: string
  hostName: string
  code: string
  /** 'lan' quando o host não tem IP público (só funciona na rede local). */
  scope: 'internet' | 'lan'
  /** Participantes aptos a virar host, em ordem de entrada. */
  queue: string[]
  maxQuality: QualityId | null
}

/** Atributos que o host grava em cada participante. */
export interface PeerAttributes {
  canHost?: '1' | '0'
  endpoint?: string
}

export type ControlMessage =
  | { t: 'handoff'; to: string; epoch: number }
  | { t: 'new-host'; epoch: number; hostId: string; hostName: string; endpoint: string; code: string }
  | { t: 'room-end'; reason: string }
  // leva a identidade: pode chegar ao host antes do aviso de que o participante entrou
  | { t: 'probe-ready'; id: string; publicIp: string | null; listening: boolean }

export const CONTROL_TOPIC = 'ctl'

export function parseRoomMeta(raw: string | undefined): RoomMeta | null {
  if (!raw) return null
  try {
    const meta = JSON.parse(raw) as RoomMeta
    return meta?.v === 1 ? meta : null
  } catch {
    return null
  }
}
