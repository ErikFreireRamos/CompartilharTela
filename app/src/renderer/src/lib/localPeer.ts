// RTCPeerConnection entre duas janelas do mesmo PC: leva o vídeo até a janela
// destacada sem servidor nenhum. Sem STUN/TURN — os dois lados são locais.

export function localPeer(): RTCPeerConnection {
  return new RTCPeerConnection({ iceServers: [] })
}

/** Espera os candidatos locais ficarem prontos (ou desiste no timeout). */
export function iceGathered(pc: RTCPeerConnection, timeoutMs = 2500): Promise<void> {
  if (pc.iceGatheringState === 'complete') return Promise.resolve()
  return new Promise((resolve) => {
    const done = (): void => {
      clearTimeout(timer)
      pc.removeEventListener('icegatheringstatechange', check)
      resolve()
    }
    const check = (): void => {
      if (pc.iceGatheringState === 'complete') done()
    }
    const timer = setTimeout(done, timeoutMs)
    pc.addEventListener('icegatheringstatechange', check)
  })
}
