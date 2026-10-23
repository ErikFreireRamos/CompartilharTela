// Teste de alcance: o host confere se outro participante recebe conexões de
// fora nas portas de host (TCP 7880 e UDP 7882). Só quem passa pode virar host.
import dgram from 'node:dgram'
import { randomBytes } from 'node:crypto'
import { PUBLIC_PORT, RTC_UDP_PORT } from '../../shared/protocol'

export const UDP_PROBE = 'CT-PROBE '
export const UDP_PROBE_OK = 'CT-PROBE-OK '

async function probeTcp(ip: string, identity: string, nonce: string): Promise<boolean> {
  try {
    const res = await fetch(`http://${ip}:${PUBLIC_PORT}/probe?nonce=${nonce}`, {
      signal: AbortSignal.timeout(2500)
    })
    const body = (await res.json()) as { nonce?: string; id?: string }
    return body.nonce === nonce && body.id === identity
  } catch {
    return false
  }
}

function probeUdp(ip: string, nonce: string): Promise<boolean> {
  return new Promise((resolve) => {
    const socket = dgram.createSocket('udp4')
    let attempts = 0
    let done = false
    const finish = (ok: boolean): void => {
      if (done) return
      done = true
      clearInterval(timer)
      socket.close()
      resolve(ok)
    }
    const send = (): void => {
      if (++attempts > 4) return finish(false)
      socket.send(UDP_PROBE + nonce, RTC_UDP_PORT, ip, () => undefined)
    }
    socket.on('error', () => finish(false))
    socket.on('message', (msg) => msg.toString() === UDP_PROBE_OK + nonce && finish(true))
    const timer = setInterval(send, 500)
    socket.bind(0, send)
  })
}

export async function probeCandidate(ip: string, identity: string): Promise<boolean> {
  // o participante pode ainda estar abrindo as portas; tenta algumas vezes
  for (let attempt = 0; attempt < 3; attempt++) {
    const nonce = randomBytes(8).toString('hex')
    if ((await probeTcp(ip, identity, nonce)) && (await probeUdp(ip, nonce))) return true
    if (attempt < 2) await new Promise((r) => setTimeout(r, 3000))
  }
  return false
}
