// Lado de quem abre a janela destacada: monta a oferta com o vídeo da
// transmissão e acompanha o ciclo de vida (fechou a janela, caiu a transmissão).
import { api } from './api'
import { iceGathered, localPeer } from './localPeer'
import type { StreamView } from './roomSession'

/** chaves (identity) com uma janela destacada aberta */
const opened = new Set<string>()
const peers = new Map<string, RTCPeerConnection>()
const listeners = new Set<() => void>()

function emit(): void {
  for (const listener of listeners) listener()
}

api.popout.onAnswer(({ key, sdp }) => {
  void peers.get(key)?.setRemoteDescription({ type: 'answer', sdp })
})

api.popout.onClosed((key) => {
  peers.get(key)?.close()
  peers.delete(key)
  if (opened.delete(key)) emit()
})

export function isPopped(key: string): boolean {
  return opened.has(key)
}

export function poppedKeys(): string[] {
  return [...opened]
}

export function onPopoutChange(cb: () => void): () => void {
  listeners.add(cb)
  return () => {
    listeners.delete(cb)
  }
}

export async function popOut(stream: StreamView): Promise<void> {
  const key = stream.identity
  if (opened.has(key)) return
  const track = stream.video?.track?.mediaStreamTrack
  if (!track) throw new Error('Esta transmissão ainda não tem vídeo para destacar.')

  const pc = localPeer()
  // o loopback não disputa banda: pede bitrate alto e resolução cheia
  const transceiver = pc.addTransceiver(track, {
    direction: 'sendonly',
    streams: [new MediaStream([track])],
    sendEncodings: [{ maxBitrate: 16_000_000, scaleResolutionDownBy: 1 }]
  })
  const offer = await pc.createOffer()
  await pc.setLocalDescription(offer)
  // degradationPreference só pode ser ajustada depois da oferta
  const sender = transceiver.sender
  const params = sender.getParameters()
  if (params.encodings.length) {
    params.degradationPreference = 'maintain-resolution'
    for (const encoding of params.encodings) {
      encoding.maxBitrate = 16_000_000
      encoding.scaleResolutionDownBy = 1
    }
    await sender.setParameters(params).catch(() => undefined)
  }
  await iceGathered(pc)

  const settings = track.getSettings()
  const aspect = settings.width && settings.height ? settings.width / settings.height : 16 / 9
  peers.set(key, pc)
  const result = await api.popout.open({
    key,
    title: stream.isLocal ? 'Sua transmissão' : stream.name,
    aspect,
    sdp: pc.localDescription?.sdp ?? offer.sdp ?? ''
  })
  if (!result.ok) {
    pc.close()
    peers.delete(key)
    throw new Error(result.error)
  }
  opened.add(key)
  emit()
}

export function closePopout(key: string): void {
  if (opened.has(key)) api.popout.close(key)
}

export function closeAllPopouts(): void {
  for (const key of [...opened]) api.popout.close(key)
}
