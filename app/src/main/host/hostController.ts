// Tudo o que só o host faz: servidor LiveKit, convites (tokens), teste de
// alcance dos participantes e a fila de quem assume se o host sair.
import { timingSafeEqual } from 'node:crypto'
import { AccessToken, RoomServiceClient, TrackSource } from 'livekit-server-sdk'
import { MAX_PARTICIPANTS, ROOM_NAME, type QualityId, type RoomMeta } from '../../shared/protocol'
import { isPrivateIPv4 } from '../net/ip'
import { LiveKitProcess, type LiveKitPaths } from './livekitProcess'
import { probeCandidate } from './probe'

export interface HostStartOptions {
  secret: number
  epoch: number
  hostId: string
  hostName: string
  publicIp: string | null
  code: string
}

export interface JoinRequest {
  secret?: unknown
  identity?: unknown
  name?: unknown
}

export interface HttpReply {
  status: number
  body: Record<string, unknown>
}

const MAX_FAILED_JOINS = 8
const FAILED_JOIN_WINDOW_MS = 10 * 60_000

export class HostController {
  private readonly livekit: LiveKitProcess
  private svc!: RoomServiceClient
  private meta!: RoomMeta
  private secret = 0
  private publicIp: string | null = null
  /** IP de cada participante como o host o vê. */
  private readonly seenIps = new Map<string, string>()
  private readonly failedJoins = new Map<string, { count: number; since: number }>()

  constructor(paths: LiveKitPaths) {
    this.livekit = new LiveKitProcess(paths)
  }

  get roomMeta(): RoomMeta {
    return this.meta
  }

  async start(opts: HostStartOptions): Promise<void> {
    this.secret = opts.secret
    this.publicIp = opts.publicIp
    await this.livekit.start(opts.publicIp)
    this.svc = new RoomServiceClient(this.livekit.url, this.livekit.apiKey, this.livekit.apiSecret)
    this.meta = {
      v: 1,
      epoch: opts.epoch,
      hostId: opts.hostId,
      hostName: opts.hostName,
      code: opts.code,
      scope: opts.publicIp ? 'internet' : 'lan',
      queue: [],
      maxQuality: null
    }
    await this.svc.createRoom({
      name: ROOM_NAME,
      maxParticipants: MAX_PARTICIPANTS,
      emptyTimeout: 600,
      departureTimeout: 20,
      metadata: JSON.stringify(this.meta)
    })
  }

  stop(): void {
    this.livekit.stop()
  }

  async mintToken(identity: string, name: string): Promise<string> {
    const token = new AccessToken(this.livekit.apiKey, this.livekit.apiSecret, { identity, name, ttl: '12h' })
    token.addGrant({
      room: ROOM_NAME,
      roomJoin: true,
      canSubscribe: true,
      canPublish: true,
      canPublishData: true,
      // só tela e áudio da tela: sem microfone e sem câmera
      canPublishSources: [TrackSource.SCREEN_SHARE, TrackSource.SCREEN_SHARE_AUDIO]
    })
    return token.toJwt()
  }

  /** POST /api/join vindo de um amigo. */
  async handleJoin(req: JoinRequest, remoteIp: string): Promise<HttpReply> {
    const failures = this.failedJoins.get(remoteIp)
    if (failures && Date.now() - failures.since > FAILED_JOIN_WINDOW_MS) this.failedJoins.delete(remoteIp)
    if ((this.failedJoins.get(remoteIp)?.count ?? 0) >= MAX_FAILED_JOINS) {
      return { status: 429, body: { error: 'too-many-attempts' } }
    }
    if (typeof req.secret !== 'number' || !this.secretMatches(req.secret)) {
      const entry = this.failedJoins.get(remoteIp) ?? { count: 0, since: Date.now() }
      entry.count++
      this.failedJoins.set(remoteIp, entry)
      return { status: 403, body: { error: 'wrong-code' } }
    }
    if (typeof req.identity !== 'string' || typeof req.name !== 'string' || !req.identity || !req.name.trim()) {
      return { status: 400, body: { error: 'bad-request' } }
    }
    const identity = req.identity.slice(0, 64)
    const name = req.name.trim().slice(0, 32)
    const participants = await this.svc.listParticipants(ROOM_NAME)
    if (participants.length >= MAX_PARTICIPANTS && !participants.some((p) => p.identity === identity)) {
      return { status: 409, body: { error: 'room-full' } }
    }
    this.seenIps.set(identity, remoteIp)
    return { status: 200, body: { token: await this.mintToken(identity, name), code: this.meta.code, yourIp: remoteIp } }
  }

  private secretMatches(secret: number): boolean {
    const a = Buffer.from(String(secret))
    const b = Buffer.from(String(this.secret))
    return a.length === b.length && timingSafeEqual(a, b)
  }

  /** O participante abriu as portas dele; testa se ele pode virar host. */
  async participantReady(identity: string, reportedIp: string | null, listening: boolean): Promise<void> {
    const target = reportedIp ?? this.seenIps.get(identity) ?? null
    let canHost = false
    if (listening && target && target !== this.publicIp) {
      const reachableScope = this.meta.scope === 'lan' || !isPrivateIPv4(target)
      canHost = reachableScope && (await probeCandidate(target, identity))
    }
    try {
      await this.svc.updateParticipant(ROOM_NAME, identity, {
        attributes: { canHost: canHost ? '1' : '0', endpoint: target ?? '' }
      })
    } catch {
      return // saiu antes do fim do teste
    }
    if (canHost && !this.meta.queue.includes(identity)) {
      this.meta.queue.push(identity)
      await this.pushMeta()
    }
  }

  async participantLeft(identity: string): Promise<void> {
    this.seenIps.delete(identity)
    if (!this.meta.queue.includes(identity)) return
    this.meta.queue = this.meta.queue.filter((id) => id !== identity)
    await this.pushMeta()
  }

  async setMaxQuality(quality: QualityId | null): Promise<void> {
    this.meta.maxQuality = quality
    await this.pushMeta()
  }

  private async pushMeta(): Promise<void> {
    await this.svc.updateRoomMetadata(ROOM_NAME, JSON.stringify(this.meta))
  }
}
