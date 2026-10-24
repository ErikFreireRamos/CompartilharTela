// Controla o audio-capture.exe (captura WASAPI por app) e repassa o PCM para
// a interface por uma MessagePort. Protocolo do stdout do helper:
//   [tipo u8][tamanho u32 LE][dados]  tipo 1 = PCM float32 estéreo 48 kHz, tipo 2 = JSON
import { spawn, type ChildProcess } from 'node:child_process'
import { EventEmitter } from 'node:events'
import fs from 'node:fs'
import { MessageChannelMain, type MessagePortMain, type WebContents } from 'electron'
import type { AudioApp, AudioStartResult, AudioStatus } from '../../shared/api'

const FRAME_PCM = 1
const FRAME_JSON = 2

interface HelperEvent {
  event: string
  [key: string]: unknown
}

type Waiter = { events: string[]; resolve: (msg: HelperEvent | null) => void }

export class AudioCaptureService extends EventEmitter<{ status: [AudioStatus] }> {
  private proc: ChildProcess | null = null
  private pending: Buffer = Buffer.alloc(0)
  private port: MessagePortMain | null = null
  private waiters: Waiter[] = []

  constructor(private readonly exePath: string) {
    super()
  }

  private ensureProcess(): ChildProcess | null {
    if (this.proc) return this.proc
    if (!fs.existsSync(this.exePath)) return null
    const proc = spawn(this.exePath, [], { windowsHide: true, stdio: ['pipe', 'pipe', 'ignore'] })
    proc.stdout!.on('data', (chunk: Buffer) => this.onData(chunk))
    proc.on('exit', () => {
      this.proc = null
      this.pending = Buffer.alloc(0)
      this.closePort()
      for (const w of this.waiters) w.resolve(null)
      this.waiters = []
      this.emit('status', { state: 'stopped' })
    })
    proc.stdin!.on('error', () => undefined)
    this.proc = proc
    return proc
  }

  private send(cmd: Record<string, unknown>): void {
    this.proc?.stdin?.write(JSON.stringify(cmd) + '\n')
  }

  private waitFor(events: string[], timeoutMs: number): Promise<HelperEvent | null> {
    return new Promise((resolve) => {
      const waiter: Waiter = {
        events,
        resolve: (msg) => {
          clearTimeout(timer)
          resolve(msg)
        }
      }
      const timer = setTimeout(() => {
        this.waiters = this.waiters.filter((w) => w !== waiter)
        resolve(null)
      }, timeoutMs)
      this.waiters.push(waiter)
    })
  }

  private onData(chunk: Buffer): void {
    this.pending = this.pending.length ? Buffer.concat([this.pending, chunk]) : chunk
    while (this.pending.length >= 5) {
      const type = this.pending[0]
      const length = this.pending.readUInt32LE(1)
      if (this.pending.length < 5 + length) break
      const payload = this.pending.subarray(5, 5 + length)
      this.pending = this.pending.subarray(5 + length)
      if (type === FRAME_PCM) this.onPcm(payload)
      else if (type === FRAME_JSON) this.onEvent(JSON.parse(payload.toString('utf8')) as HelperEvent)
    }
  }

  private onPcm(payload: Buffer): void {
    if (!this.port) return
    const copy = payload.buffer.slice(payload.byteOffset, payload.byteOffset + payload.byteLength)
    this.port.postMessage(new Float32Array(copy))
  }

  private onEvent(msg: HelperEvent): void {
    const index = this.waiters.findIndex((w) => w.events.includes(msg.event))
    if (index >= 0) this.waiters.splice(index, 1)[0].resolve(msg)
    if (msg.event === 'error') this.emit('status', { state: 'error', message: String(msg.message ?? '') })
  }

  async listApps(blocked: string[]): Promise<AudioApp[]> {
    if (!this.ensureProcess()) return []
    const reply = this.waitFor(['apps'], 3000)
    this.send({ cmd: 'list', blocked, excludePid: process.pid })
    const msg = await reply
    return (msg?.apps as AudioApp[] | undefined) ?? []
  }

  async start(blocked: string[], only: string[], excludePid: number, target: WebContents): Promise<AudioStartResult> {
    if (!this.ensureProcess()) return { ok: false, error: 'audio-capture.exe não encontrado (rode "npm run build:audio")' }
    const reply = this.waitFor(['started', 'error'], 6000)
    this.send({ cmd: 'start', blocked, only, excludePid })
    const msg = await reply
    if (msg?.event !== 'started') {
      return { ok: false, error: String(msg?.message ?? 'O módulo de áudio não respondeu') }
    }
    this.closePort()
    const { port1, port2 } = new MessageChannelMain()
    this.port = port1
    target.postMessage('audio:port', null, [port2])
    this.emit('status', { state: 'running' })
    return { ok: true }
  }

  /** Executável dono de uma janela, a partir do HWND do id da fonte (window:<hwnd>:0). */
  async resolveWindow(hwnd: number): Promise<{ pid: number; exe: string }> {
    if (!hwnd || !this.ensureProcess()) return { pid: 0, exe: '' }
    const reply = this.waitFor(['pid'], 3000)
    this.send({ cmd: 'pidOfWindow', hwnd })
    const msg = await reply
    return { pid: Number(msg?.pid ?? 0), exe: String(msg?.exe ?? '') }
  }

  setBlocked(blocked: string[]): void {
    this.send({ cmd: 'setBlocked', blocked })
  }

  stop(): void {
    this.send({ cmd: 'stop' })
    this.closePort()
  }

  dispose(): void {
    this.closePort()
    this.proc?.kill()
    this.proc = null
  }

  private closePort(): void {
    this.port?.close()
    this.port = null
  }
}
