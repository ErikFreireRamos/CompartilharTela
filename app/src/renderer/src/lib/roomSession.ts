import {
  AudioPresets,
  ConnectionState,
  DisconnectReason,
  Room,
  RoomEvent,
  Track,
  VideoQuality,
  type LocalTrackPublication,
  type Participant,
  type RemoteParticipant,
  type RemoteTrackPublication,
  type TrackPublication
} from 'livekit-client'
import type { JoinInfo, Settings } from '../../../shared/api'
import {
  CONTROL_TOPIC,
  parseRoomMeta,
  type ControlMessage,
  type PeerAttributes,
  type QualityId,
  type RoomMeta
} from '../../../shared/protocol'
import { api, errorMessage, sleep } from './api'
import { successorCandidates } from './election'
import { contentSettings, simulcastLayers } from './qualityPresets'
import { startScreenShare, type LocalShare, type ShareOptions } from './screenCapture'

export type Phase = 'idle' | 'connecting' | 'connected' | 'migrating'
export type ReceiveQuality = 'auto' | 'low' | 'off'

export interface PeerView {
  identity: string
  name: string
  isLocal: boolean
  isHost: boolean
  canHost: boolean | null
  sharing: boolean
}

export interface StreamView {
  identity: string
  name: string
  isLocal: boolean
  video?: TrackPublication
  audio?: TrackPublication
  receive: ReceiveQuality
}

interface PeerSnapshot {
  name: string
  endpoint?: string
}

const HOST_LOST_AFTER_MS = 6000
const HANDOFF_WAIT_MS = 6000
const JOIN_RETRY_WINDOW_MS = 25_000

export class RoomSession {
  phase: Phase = 'idle'
  /** Aviso temporário no topo da sala (troca de host, conexão instável...). */
  notice: string | null = null
  /** Erro mostrado na tela inicial. */
  error: string | null = null
  info: JoinInfo | null = null
  meta: RoomMeta | null = null
  room: Room | null = null
  share: LocalShare | null = null
  settings: Settings | null = null
  /** App aberto como administrador: o Windows bloqueia a captura de tela. */
  elevated = false

  private listeners = new Set<() => void>()
  private version = 0
  private nickname = ''
  private leaving = false
  private switching = false
  private migrating = false
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private peers = new Map<string, PeerSnapshot>()
  private sharePubs: LocalTrackPublication[] = []
  private receive = new Map<string, ReceiveQuality>()
  private onNewHost: ((msg: Extract<ControlMessage, { t: 'new-host' }>) => void) | null = null

  // ---- estado para o React (useSyncExternalStore)
  subscribe = (cb: () => void): (() => void) => {
    this.listeners.add(cb)
    return () => this.listeners.delete(cb)
  }
  getVersion = (): number => this.version
  private changed(): void {
    this.version++
    for (const l of this.listeners) l()
  }

  get isHost(): boolean {
    return !!this.info?.isHost
  }
  get myId(): string {
    return this.info?.identity ?? this.settings?.clientId ?? ''
  }
  get code(): string {
    return this.meta?.code ?? this.info?.code ?? ''
  }

  async loadSettings(): Promise<Settings> {
    ;[this.settings, this.elevated] = await Promise.all([api.settings.get(), api.app.isElevated()])
    this.changed()
    return this.settings
  }

  async updateSettings(patch: Partial<Settings>): Promise<void> {
    this.settings = await api.settings.set(patch)
    this.changed()
  }

  // ---- entrar e sair

  async create(nickname: string): Promise<void> {
    await this.enter(nickname, () => api.room.create(nickname))
  }

  async join(code: string, nickname: string): Promise<void> {
    await this.enter(nickname, () => api.room.join(code, nickname))
  }

  private async enter(nickname: string, getInfo: () => Promise<JoinInfo>): Promise<void> {
    this.nickname = nickname
    this.error = null
    this.phase = 'connecting'
    this.changed()
    try {
      await this.connect(await getInfo())
    } catch (err) {
      await this.reset(errorMessage(err))
    }
  }

  /** Sai da sala. Se for o host, passa o host para o próximo da fila antes. */
  async leave(): Promise<void> {
    if (this.leaving || this.phase === 'idle') return
    this.leaving = true
    this.notice = this.isHost ? 'Passando o host para outra pessoa…' : 'Saindo…'
    this.changed()
    try {
      if (this.isHost && this.room) await this.handOff(this.room)
    } finally {
      await this.reset(null)
    }
  }

  private async reset(error: string | null): Promise<void> {
    this.clearReconnectTimer()
    this.stopShare()
    const room = this.room
    this.room = null
    await room?.disconnect().catch(() => undefined)
    await api.room.leave().catch(() => undefined)
    this.phase = 'idle'
    this.info = null
    this.meta = null
    this.notice = null
    this.error = error
    this.peers.clear()
    this.receive.clear()
    this.leaving = false
    this.switching = false
    this.migrating = false
    this.changed()
  }

  // ---- conexão

  private async connect(info: JoinInfo): Promise<void> {
    const room = new Room({
      adaptiveStream: { pixelDensity: 'screen' },
      dynacast: true,
      webAudioMix: true,
      disconnectOnPageLeave: false
    })
    this.wire(room)
    try {
      await room.connect(info.url, info.token, { autoSubscribe: true })
    } catch (err) {
      room.removeAllListeners()
      await room.disconnect().catch(() => undefined)
      throw err
    }
    this.room = room
    this.info = info
    this.meta = parseRoomMeta(room.metadata) ?? this.meta
    this.phase = 'connected'
    this.snapshotPeers()
    this.applyVolumes()
    this.changed()
    void room.startAudio()
    if (this.share) await this.publishShare(room)
    if (!info.isHost) void this.announceCandidate(room)
  }

  private wire(room: Room): void {
    const refresh = (): void => {
      if (room !== this.room) return
      this.snapshotPeers()
      this.changed()
    }
    // Sinalização ou mídia caiu: se não voltar logo, o host provavelmente saiu do ar
    const onReconnecting = (): void => {
      if (room !== this.room || this.reconnectTimer) return
      this.notice = 'Conexão com o host instável…'
      this.changed()
      this.reconnectTimer = setTimeout(() => void this.onHostLost(room), HOST_LOST_AFTER_MS)
    }
    room
      .on(RoomEvent.ParticipantConnected, refresh)
      .on(RoomEvent.ParticipantDisconnected, (p: RemoteParticipant) => {
        if (room === this.room && this.isHost) void api.host.participantLeft(p.identity)
        refresh()
      })
      .on(RoomEvent.ParticipantAttributesChanged, refresh)
      .on(RoomEvent.ParticipantNameChanged, refresh)
      .on(RoomEvent.TrackPublished, refresh)
      .on(RoomEvent.TrackUnpublished, refresh)
      .on(RoomEvent.TrackSubscribed, refresh)
      .on(RoomEvent.TrackUnsubscribed, refresh)
      .on(RoomEvent.LocalTrackPublished, refresh)
      .on(RoomEvent.LocalTrackUnpublished, refresh)
      .on(RoomEvent.RoomMetadataChanged, (raw: string) => {
        if (room !== this.room) return
        this.meta = parseRoomMeta(raw) ?? this.meta
        this.changed()
      })
      .on(RoomEvent.DataReceived, (payload, participant, _kind, topic) => {
        if (room !== this.room || topic !== CONTROL_TOPIC) return
        try {
          void this.onControl(JSON.parse(new TextDecoder().decode(payload)) as ControlMessage, participant)
        } catch {
          // mensagem malformada
        }
      })
      .on(RoomEvent.SignalReconnecting, onReconnecting)
      .on(RoomEvent.Reconnecting, onReconnecting)
      .on(RoomEvent.Reconnected, () => {
        if (room !== this.room) return
        this.clearReconnectTimer()
        this.notice = null
        this.changed()
      })
      .on(RoomEvent.Disconnected, (reason?: DisconnectReason) => {
        if (room !== this.room || this.leaving || this.switching) return
        this.clearReconnectTimer()
        if (reason === DisconnectReason.DUPLICATE_IDENTITY) {
          void this.reset('Você entrou nesta sala em outra janela.')
          return
        }
        void this.onHostLost(room)
      })
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer)
    this.reconnectTimer = null
  }

  /** Última lista conhecida de quem estava na sala; usada se o host cair. */
  private snapshotPeers(): void {
    const room = this.room
    // durante a queda o LiveKit esvazia a lista; mantém a última boa
    if (!room || room.state !== ConnectionState.Connected) return
    this.peers.clear()
    for (const p of [room.localParticipant, ...room.remoteParticipants.values()]) {
      const attrs = p.attributes as PeerAttributes
      this.peers.set(p.identity, { name: p.name || p.identity, endpoint: attrs.endpoint || undefined })
    }
  }

  private async sendControl(room: Room, msg: ControlMessage, to?: string[]): Promise<void> {
    await room.localParticipant.publishData(new TextEncoder().encode(JSON.stringify(msg)), {
      reliable: true,
      topic: CONTROL_TOPIC,
      destinationIdentities: to
    })
  }

  /** Abre as portas e pede ao host o teste de alcance (para entrar na fila de sucessão). */
  private async announceCandidate(room: Room): Promise<void> {
    try {
      const candidate = await api.room.prepareCandidate()
      if (room !== this.room) return
      const hostId = this.meta?.hostId
      await this.sendControl(
        room,
        { t: 'probe-ready', id: this.myId, publicIp: candidate.publicIp, listening: candidate.listening },
        hostId ? [hostId] : undefined
      )
    } catch {
      // sem teste, só não entra na fila
    }
  }

  private async onControl(msg: ControlMessage, from?: Participant): Promise<void> {
    const fromHost = !!from && from.identity === this.meta?.hostId
    switch (msg.t) {
      case 'probe-ready': {
        const id = from?.identity ?? msg.id
        if (this.isHost && id) void api.host.participantReady(id, msg.publicIp, msg.listening)
        break
      }
      case 'handoff':
        if (!fromHost) break
        if (msg.to === this.myId) void this.takeOver(msg.epoch)
        else {
          this.notice = 'O host está saindo, trocando de host…'
          this.changed()
        }
        break
      case 'new-host':
        if (!from || from.identity !== msg.hostId) break
        this.onNewHost?.(msg)
        if (msg.hostId !== this.myId && !this.isHost && !this.switching && !this.migrating) {
          void this.moveTo([{ endpoint: msg.endpoint, name: msg.hostName }], `${msg.hostName} agora é o host. Reconectando…`)
        }
        break
      case 'room-end':
        if (fromHost) void this.reset(msg.reason)
        break
    }
  }

  // ---- troca de host

  /** Host saindo: oferece o host a cada candidato até um aceitar. */
  private async handOff(room: Room): Promise<void> {
    const meta = this.meta
    const candidates = successorCandidates(meta, room.remoteParticipants.keys())
    for (const candidate of candidates) {
      const accepted = new Promise<boolean>((resolve) => {
        const timer = setTimeout(() => resolve(false), HANDOFF_WAIT_MS)
        this.onNewHost = (msg) => {
          if (msg.hostId !== candidate) return
          clearTimeout(timer)
          resolve(true)
        }
      })
      await this.sendControl(room, { t: 'handoff', to: candidate, epoch: (meta?.epoch ?? 0) + 1 })
      if (await accepted) {
        this.onNewHost = null
        await sleep(500) // dá tempo de todos receberem o new-host
        return
      }
    }
    this.onNewHost = null
    await this.sendControl(room, { t: 'room-end', reason: 'O host encerrou a sala.' }).catch(() => undefined)
    await sleep(300)
  }

  /** Recebi o host: subo o servidor, aviso todos e me conecto nele. */
  private async takeOver(epoch: number): Promise<void> {
    const oldRoom = this.room
    if (!oldRoom || !this.info) return
    this.notice = 'Você agora é o host. Preparando a sala…'
    this.changed()
    try {
      const info = await api.room.becomeHost(this.info.secret, epoch, this.nickname)
      await this.sendControl(oldRoom, {
        t: 'new-host',
        epoch,
        hostId: info.identity,
        hostName: this.nickname,
        endpoint: info.endpoint,
        code: info.code
      })
      await this.switchRoom(() => Promise.resolve(info), 'Você agora é o host.')
    } catch (err) {
      this.notice = `Não consegui assumir o host: ${errorMessage(err)}`
      this.changed()
    }
  }

  /** O host sumiu sem avisar: todos escolhem o mesmo sucessor pela fila. */
  private async onHostLost(room: Room): Promise<void> {
    if (this.migrating || this.leaving || room !== this.room || !this.info) return
    this.migrating = true
    const meta = this.meta
    const candidates = successorCandidates(meta, this.peers.keys())
    const peers = new Map(this.peers)
    try {
      for (const candidate of candidates) {
        if (candidate === this.myId) {
          const secret = this.info.secret
          const ok = await this.switchRoom(
            () => api.room.becomeHost(secret, (meta?.epoch ?? 0) + 1, this.nickname),
            'O host caiu. Você agora é o host.',
            1
          )
          if (ok) return
          continue
        }
        const peer = peers.get(candidate)
        if (!peer?.endpoint) continue
        if (await this.moveTo([{ endpoint: peer.endpoint, name: peer.name }], `O host caiu. Reconectando em ${peer.name}…`)) {
          return
        }
      }
      await this.reset('O host saiu e ninguém na sala consegue assumir (portas fechadas).')
    } finally {
      this.migrating = false
    }
  }

  private async moveTo(targets: Array<{ endpoint: string; name: string }>, notice: string): Promise<boolean> {
    const secret = this.info?.secret
    if (secret === undefined) return false
    for (const target of targets) {
      const ok = await this.switchRoom(() => api.room.joinEndpoint(target.endpoint, secret, this.nickname), notice)
      if (ok) return true
    }
    return false
  }

  /** Desconecta da sala atual e tenta entrar na nova até dar certo ou acabar o tempo. */
  private async switchRoom(getInfo: () => Promise<JoinInfo>, notice: string, maxAttempts = Infinity): Promise<boolean> {
    this.switching = true
    this.phase = 'migrating'
    this.notice = notice
    this.clearReconnectTimer()
    this.changed()
    const old = this.room
    this.room = null
    this.releasePublished()
    void old?.disconnect(false).catch(() => undefined)
    const deadline = Date.now() + JOIN_RETRY_WINDOW_MS
    try {
      for (let attempt = 0; attempt < maxAttempts && Date.now() < deadline && !this.leaving; attempt++) {
        try {
          await this.connect(await getInfo())
          this.notice = notice.endsWith('…') ? null : notice
          this.changed()
          if (this.notice) setTimeout(() => this.clearNotice(notice), 5000)
          return true
        } catch {
          await sleep(2000)
        }
      }
      return false
    } finally {
      this.switching = false
    }
  }

  private clearNotice(text: string): void {
    if (this.notice !== text) return
    this.notice = null
    this.changed()
  }

  // ---- transmitir

  async startShare(opts: ShareOptions): Promise<void> {
    this.stopShare()
    const share = await startScreenShare(opts)
    share.video.addEventListener('ended', () => this.stopShare())
    this.share = share
    if (this.room) await this.publishShare(this.room)
    this.changed()
  }

  stopShare(): void {
    const room = this.room
    for (const pub of this.sharePubs) {
      if (pub.track && room) void room.localParticipant.unpublishTrack(pub.track, true).catch(() => undefined)
    }
    this.sharePubs = []
    this.share?.stop()
    this.share = null
    this.changed()
  }

  setSendVolume(volume: number): void {
    this.share?.audio?.setVolume(volume)
  }

  private async publishShare(room: Room): Promise<void> {
    const share = this.share
    if (!share) return
    const { hint, degradation } = contentSettings(share.contentType)
    // publica clones: o LiveKit para a trilha ao despublicar, e o original
    // continua vivo para republicar depois de uma troca de host
    const video = share.video.clone()
    video.contentHint = hint
    // movimento: H.264 por hardware com simulcast. Texto: VP9 (software), mais
    // nítido em letras pequenas e leve com tela quase parada.
    const codec =
      share.contentType === 'motion'
        ? { videoCodec: 'h264' as const }
        : { videoCodec: 'vp9' as const, scalabilityMode: 'L3T3_KEY' as const }
    const pubs = [
      await room.localParticipant.publishTrack(video, {
        source: Track.Source.ScreenShare,
        name: 'screen',
        stream: 'screen',
        ...codec,
        backupCodec: false,
        simulcast: true,
        screenShareEncoding: { maxBitrate: share.preset.bitrate, maxFramerate: share.preset.fps },
        screenShareSimulcastLayers: simulcastLayers(share.preset),
        degradationPreference: degradation
      })
    ]
    if (share.audio) {
      pubs.push(
        await room.localParticipant.publishTrack(share.audio.track.clone(), {
          source: Track.Source.ScreenShareAudio,
          name: 'screen-audio',
          stream: 'screen',
          audioPreset: AudioPresets.musicHighQualityStereo,
          dtx: false,
          red: false,
          forceStereo: true
        })
      )
    }
    this.sharePubs = pubs
  }

  /** As publicações morrem com a sala antiga; para os clones. */
  private releasePublished(): void {
    for (const pub of this.sharePubs) pub.track?.mediaStreamTrack.stop()
    this.sharePubs = []
  }

  // ---- assistir

  peerList(): PeerView[] {
    const room = this.room
    if (!room) return []
    const hostId = this.meta?.hostId
    return [room.localParticipant, ...room.remoteParticipants.values()].map((p) => {
      const attrs = p.attributes as PeerAttributes
      return {
        identity: p.identity,
        name: p.name || p.identity,
        isLocal: p === room.localParticipant,
        isHost: p.identity === hostId,
        canHost: attrs.canHost === undefined ? null : attrs.canHost === '1',
        sharing: !!p.getTrackPublication(Track.Source.ScreenShare)
      }
    })
  }

  streams(): StreamView[] {
    const room = this.room
    if (!room) return []
    const result: StreamView[] = []
    for (const p of [room.localParticipant, ...room.remoteParticipants.values()]) {
      const video = p.getTrackPublication(Track.Source.ScreenShare)
      if (!video) continue
      result.push({
        identity: p.identity,
        name: p.name || p.identity,
        isLocal: p === room.localParticipant,
        video,
        audio: p.getTrackPublication(Track.Source.ScreenShareAudio),
        receive: this.receive.get(p.identity) ?? 'auto'
      })
    }
    return result
  }

  volumeOf(name: string): number {
    return this.settings?.volumes[name] ?? 1
  }

  setVolume(identity: string, name: string, volume: number): void {
    this.room?.remoteParticipants.get(identity)?.setVolume(volume, Track.Source.ScreenShareAudio)
    if (this.settings) this.settings.volumes = { ...this.settings.volumes, [name]: volume }
    this.changed()
    void api.settings.set({ volumes: this.settings?.volumes })
  }

  private applyVolumes(): void {
    for (const p of this.room?.remoteParticipants.values() ?? []) {
      p.setVolume(this.volumeOf(p.name || p.identity), Track.Source.ScreenShareAudio)
    }
  }

  setReceive(identity: string, mode: ReceiveQuality): void {
    const p = this.room?.remoteParticipants.get(identity)
    if (!p) return
    this.receive.set(identity, mode)
    const video = p.getTrackPublication(Track.Source.ScreenShare) as RemoteTrackPublication | undefined
    const audio = p.getTrackPublication(Track.Source.ScreenShareAudio) as RemoteTrackPublication | undefined
    video?.setSubscribed(mode !== 'off')
    audio?.setSubscribed(mode !== 'off')
    if (mode !== 'off') video?.setVideoQuality(mode === 'low' ? VideoQuality.LOW : VideoQuality.HIGH)
    this.changed()
  }

  async setMaxQuality(quality: QualityId | null): Promise<void> {
    await api.host.setMaxQuality(quality)
  }
}

export const session = new RoomSession()

// acesso pelo DevTools / testes automatizados
;(window as unknown as { ctDebug: unknown }).ctDebug = { session, api }
