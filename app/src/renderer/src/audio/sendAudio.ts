// Áudio enviado na transmissão: PCM do audio-capture.exe (ou o áudio do sistema
// inteiro, no fallback) -> ganho (volume enviado) -> MediaStreamTrack.

// Toca blocos PCM estéreo intercalados recebidos pela port. Mantém ~40 ms de
// buffer e descarta o excesso, porque o relógio da captura e o do AudioContext
// não andam exatamente juntos.
const WORKLET = `
class PcmPlayer extends AudioWorkletProcessor {
  constructor() {
    super()
    this.capacity = 48000
    this.left = new Float32Array(this.capacity)
    this.right = new Float32Array(this.capacity)
    this.readPos = 0
    this.writePos = 0
    this.count = 0
    this.primed = false
    this.target = 1920
    this.max = 7200
    this.port.onmessage = (e) => this.push(e.data)
  }
  push(data) {
    const frames = data.length >> 1
    for (let i = 0; i < frames; i++) {
      this.left[this.writePos] = data[2 * i]
      this.right[this.writePos] = data[2 * i + 1]
      this.writePos = (this.writePos + 1) % this.capacity
    }
    this.count += frames
    if (this.count > this.max) {
      const drop = this.count - this.target
      this.readPos = (this.readPos + drop) % this.capacity
      this.count -= drop
    }
  }
  process(_inputs, outputs) {
    const out = outputs[0]
    const L = out[0]
    const R = out[1] || out[0]
    const n = L.length
    if (!this.primed && this.count >= this.target) this.primed = true
    if (!this.primed || this.count < n) {
      this.primed = false
      L.fill(0)
      R.fill(0)
      return true
    }
    for (let i = 0; i < n; i++) {
      L[i] = this.left[this.readPos]
      R[i] = this.right[this.readPos]
      this.readPos = (this.readPos + 1) % this.capacity
    }
    this.count -= n
    return true
  }
}
registerProcessor('pcm-player', PcmPlayer)
`

let workletUrl: string | null = null

export class SendAudio {
  private constructor(
    private readonly ctx: AudioContext,
    private readonly gain: GainNode,
    private readonly dest: MediaStreamAudioDestinationNode,
    private readonly onClose: () => void
  ) {}

  get track(): MediaStreamTrack {
    return this.dest.stream.getAudioTracks()[0]
  }

  private static graph(volume: number): { ctx: AudioContext; gain: GainNode; dest: MediaStreamAudioDestinationNode } {
    const ctx = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' })
    const gain = ctx.createGain()
    gain.gain.value = volume
    const dest = ctx.createMediaStreamDestination()
    dest.channelCount = 2
    gain.connect(dest)
    return { ctx, gain, dest }
  }

  /** Áudio por app, vindo do audio-capture.exe. */
  static async fromPort(port: MessagePort, volume: number): Promise<SendAudio> {
    const { ctx, gain, dest } = SendAudio.graph(volume)
    workletUrl ??= URL.createObjectURL(new Blob([WORKLET], { type: 'application/javascript' }))
    await ctx.audioWorklet.addModule(workletUrl)
    const player = new AudioWorkletNode(ctx, 'pcm-player', {
      numberOfInputs: 0,
      numberOfOutputs: 1,
      outputChannelCount: [2]
    })
    player.connect(gain)
    port.onmessage = (e: MessageEvent<Float32Array>) => player.port.postMessage(e.data, [e.data.buffer])
    await ctx.resume()
    return new SendAudio(ctx, gain, dest, () => {
      port.onmessage = null
      port.close()
      player.disconnect()
    })
  }

  /** Fallback: áudio do sistema inteiro capturado pelo Chromium. */
  static fromTrack(track: MediaStreamTrack, volume: number): SendAudio {
    const { ctx, gain, dest } = SendAudio.graph(volume)
    const source = ctx.createMediaStreamSource(new MediaStream([track]))
    source.connect(gain)
    void ctx.resume()
    return new SendAudio(ctx, gain, dest, () => {
      source.disconnect()
      track.stop()
    })
  }

  setVolume(volume: number): void {
    this.gain.gain.setTargetAtTime(volume, this.ctx.currentTime, 0.02)
  }

  close(): void {
    this.onClose()
    this.track.stop()
    void this.ctx.close()
  }
}
