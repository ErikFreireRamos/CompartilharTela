// Contrato entre o processo main e a interface (exposto em window.api pelo preload).
import type { QualityId } from './protocol'

export type ContentType = 'motion' | 'detail'

export interface Settings {
  clientId: string
  nickname: string
  /** Executáveis cujo áudio não é transmitido (minúsculas, ex.: "discord.exe"). */
  blockedApps: string[]
  /** Volume de cada amigo, pelo apelido (1 = 100%). */
  volumes: Record<string, number>
  quality: QualityId
  contentType: ContentType
  shareAudio: boolean
  sendVolume: number
}

export interface ScreenSource {
  id: string
  name: string
  thumbnail: string
}

export interface AudioApp {
  exe: string
  name: string
  active: boolean
  blocked: boolean
}

export interface JoinInfo {
  url: string
  token: string
  identity: string
  /** IP público de quem é o host (o que vai no código). */
  endpoint: string
  code: string
  secret: number
  isHost: boolean
  /** Só para o host: se as portas foram abertas (UPnP ou IP direto). */
  portsOpen?: boolean
  scope?: 'internet' | 'lan'
}

export interface CandidateInfo {
  listening: boolean
  publicIp: string | null
  portsOpen: boolean
}

export type AudioStartResult = { ok: true } | { ok: false; error: string }

export interface AudioStatus {
  state: 'running' | 'stopped' | 'error'
  message?: string
}

export interface Api {
  settings: {
    get(): Promise<Settings>
    set(patch: Partial<Settings>): Promise<Settings>
  }
  screen: {
    sources(): Promise<ScreenSource[]>
    /** Define o monitor (e se vai o áudio do sistema inteiro) do próximo getDisplayMedia. */
    select(sourceId: string, systemAudio: boolean): Promise<void>
  }
  audio: {
    listApps(blocked: string[]): Promise<AudioApp[]>
    /**
     * Inicia a captura por app. O PCM chega numa MessagePort entregue por
     * window.postMessage({ type: 'audio:port' }) (ela não passa pelo contextBridge).
     */
    start(blocked: string[]): Promise<AudioStartResult>
    setBlocked(blocked: string[]): Promise<void>
    stop(): Promise<void>
    onStatus(cb: (status: AudioStatus) => void): () => void
  }
  room: {
    create(nickname: string): Promise<JoinInfo>
    join(code: string, nickname: string): Promise<JoinInfo>
    joinEndpoint(endpoint: string, secret: number, nickname: string): Promise<JoinInfo>
    becomeHost(secret: number, epoch: number, nickname: string): Promise<JoinInfo>
    prepareCandidate(): Promise<CandidateInfo>
    leave(): Promise<void>
  }
  host: {
    participantReady(identity: string, publicIp: string | null, listening: boolean): Promise<void>
    participantLeft(identity: string): Promise<void>
    setMaxQuality(quality: QualityId | null): Promise<void>
  }
  app: {
    /** Rodando como administrador: o Windows bloqueia a captura de tela nesse modo. */
    isElevated(): Promise<boolean>
    onCloseRequested(cb: () => void): () => void
    quitNow(): Promise<void>
    clipboard(text: string): Promise<void>
  }
}
