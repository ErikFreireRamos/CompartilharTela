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
  /** Com janela escolhida: 'app' = só o áudio do app dela; 'pc' = todo o PC. */
  audioScope: 'app' | 'pc'
  sendVolume: number
}

export interface ScreenSource {
  id: string
  name: string
  thumbnail: string
  /** screen = monitor inteiro; window = uma janela específica */
  kind: 'screen' | 'window'
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
  /** Só para o host: a operadora usa CGNAT, então ninguém de fora alcança este PC. */
  cgnat?: boolean
  scope?: 'internet' | 'lan'
}

export interface CandidateInfo {
  listening: boolean
  publicIp: string | null
  portsOpen: boolean
}

/** Situação da rede deste PC (não abre portas nem mexe no roteador). */
export interface NetworkStatus {
  publicIp: string | null
  /** O IP público está direto numa interface deste PC (sem roteador no meio). */
  direct: boolean
  /** A operadora usa CGNAT: ninguém de fora alcança este PC pelo IPv4. */
  cgnat: boolean
  /** IPv6 público deste PC, se o IPv6 funciona. Contorna o CGNAT do IPv4. */
  ipv6: string | null
  lanIps: string[]
}

export type AudioStartResult = { ok: true } | { ok: false; error: string }

/** Pedido para abrir uma transmissão numa janela própria ("destacar"). */
export interface PopoutRequest {
  /** identity de quem transmite: chave única da janela */
  key: string
  title: string
  /** largura / altura do vídeo, para abrir a janela no formato certo */
  aspect: number
  /** oferta SDP do RTCPeerConnection local (o MediaStream não cruza processos) */
  sdp: string
}

export interface PopoutOffer {
  key: string
  title: string
  sdp: string
}

export interface PopoutAnswer {
  key: string
  sdp: string
}

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
     * `only` vazio = todos menos `blocked`; preenchido = só aqueles apps.
     */
    start(blocked: string[], only?: string[]): Promise<AudioStartResult>
    /** Executável dono da janela escolhida (para o modo "só este app"). */
    resolveWindow(sourceId: string): Promise<{ pid: number; exe: string }>
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
    /** Lê a área de transferência pelo processo main (sem pedir permissão ao Chromium). */
    readClipboard(): Promise<string>
  }
  net: {
    /** `refresh` ignora o cache de 60 s. */
    status(refresh?: boolean): Promise<NetworkStatus>
  }
  /** Janela isolada com uma transmissão (para pôr no segundo monitor, por exemplo). */
  popout: {
    open(request: PopoutRequest): Promise<{ ok: true } | { ok: false; error: string }>
    /** A janela destacada avisa que já pode receber a oferta. */
    ready(): void
    answer(key: string, sdp: string): void
    close(key: string): void
    onOffer(cb: (offer: PopoutOffer) => void): () => void
    onAnswer(cb: (answer: PopoutAnswer) => void): () => void
    onClosed(cb: (key: string) => void): () => void
  }
}
