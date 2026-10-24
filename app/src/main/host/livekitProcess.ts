// Sobe e derruba o livekit-server.exe embutido, com chaves novas a cada sala.
import { spawn, type ChildProcess } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { LIVEKIT_INTERNAL_PORT, MAX_PARTICIPANTS, RTC_TCP_PORT, RTC_UDP_PORT } from '../../shared/protocol'

export interface LiveKitPaths {
  exe: string
  workDir: string
}

export class LiveKitProcess {
  readonly apiKey = 'API' + randomBytes(6).toString('hex')
  readonly apiSecret = randomBytes(32).toString('base64url')
  private proc: ChildProcess | null = null

  constructor(private readonly paths: LiveKitPaths) {}

  get url(): string {
    return `http://127.0.0.1:${LIVEKIT_INTERNAL_PORT}`
  }

  async start(nodeIp: string | null): Promise<void> {
    if (!fs.existsSync(this.paths.exe)) {
      throw new Error(`livekit-server.exe não encontrado em ${this.paths.exe}. Rode "npm run setup:livekit".`)
    }
    fs.mkdirSync(this.paths.workDir, { recursive: true })
    const configPath = path.join(this.paths.workDir, 'livekit.yaml')
    fs.writeFileSync(configPath, this.config(nodeIp))

    const log = fs.createWriteStream(path.join(this.paths.workDir, 'livekit.log'), { flags: 'w' })
    const proc = spawn(this.paths.exe, ['--config', configPath], { windowsHide: true })
    this.proc = proc
    proc.stdout?.pipe(log)
    proc.stderr?.pipe(log)

    const exited = new Promise<never>((_, reject) =>
      proc.once('exit', (code) =>
        reject(new Error(`O servidor LiveKit fechou (código ${code}). Veja ${path.join(this.paths.workDir, 'livekit.log')}`))
      )
    )
    exited.catch(() => undefined)
    try {
      await Promise.race([this.waitReady(15_000), exited])
    } catch (err) {
      this.stop()
      throw err
    }
  }

  stop(): void {
    if (this.proc && this.proc.exitCode === null) this.proc.kill()
    this.proc = null
  }

  private async waitReady(timeoutMs: number): Promise<void> {
    const deadline = Date.now() + timeoutMs
    while (Date.now() < deadline) {
      try {
        const res = await fetch(this.url, { signal: AbortSignal.timeout(1000) })
        if (res.ok) return
      } catch {
        // ainda subindo
      }
      await new Promise((r) => setTimeout(r, 200))
    }
    throw new Error('O servidor LiveKit não respondeu a tempo')
  }

  private config(nodeIp: string | null): string {
    // node_ip + advertise_internal_ip: anuncia o IP público e também os IPs da
    // rede local; enable_loopback_candidate deixa o próprio host conectar via 127.0.0.1.
    return [
      `port: ${LIVEKIT_INTERNAL_PORT}`,
      'logging:',
      '  level: info',
      'rtc:',
      `  tcp_port: ${RTC_TCP_PORT}`,
      `  udp_port: ${RTC_UDP_PORT}`,
      '  use_external_ip: false',
      ...(nodeIp ? [`  node_ip: ${nodeIp}`, '  advertise_internal_ip: true'] : []),
      '  enable_loopback_candidate: true',
      // O default do LiveKit inclui o STUN da Twilio, que falha no DNS em muitas
      // redes (ERR_NAME_NOT_RESOLVED = -105) e só polui o log. Os dois do Google
      // já fazem parte do default e a conexão direta usa node_ip/UPnP mesmo.
      '  stun_servers:',
      '    - stun.l.google.com:19302',
      '    - stun1.l.google.com:19302',
      'keys:',
      `  ${this.apiKey}: ${this.apiSecret}`,
      'room:',
      '  auto_create: false',
      `  max_participants: ${MAX_PARTICIPANTS}`,
      '  empty_timeout: 600',
      '  departure_timeout: 20',
      ''
    ].join('\n')
  }
}
