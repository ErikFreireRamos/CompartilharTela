// Acha o host na mesma rede local. Necessário porque nem todo roteador deixa
// um PC da casa acessar o próprio IP público (NAT loopback).
import dgram from 'node:dgram'
import { DISCOVERY_PORT } from '../../shared/protocol'
import { lanInterfaces } from './ip'

const FIND = 'CT-FIND '
const HERE = 'CT-HERE '

export class LanResponder {
  private constructor(private readonly socket: dgram.Socket) {}

  static start(secret: number): Promise<LanResponder | null> {
    return new Promise((resolve) => {
      const socket = dgram.createSocket({ type: 'udp4', reuseAddr: true })
      socket.on('message', (msg, rinfo) => {
        if (msg.toString() === FIND + secret) socket.send(HERE + secret, rinfo.port, rinfo.address)
      })
      socket.once('error', () => resolve(null))
      socket.bind(DISCOVERY_PORT, () => resolve(new LanResponder(socket)))
    })
  }

  close(): void {
    try {
      this.socket.close()
    } catch {
      // já fechado
    }
  }
}

/**
 * Devolve o IP local do host da sala, ou null se não estiver na mesma rede.
 * CT_NO_LAN_DISCOVERY=1 desliga a busca (testes: força usar o IP do código).
 */
export function findOnLan(secret: number, timeoutMs = 500): Promise<string | null> {
  if (process.env.CT_NO_LAN_DISCOVERY === '1') return Promise.resolve(null)
  return new Promise((resolve) => {
    const socket = dgram.createSocket('udp4')
    let done = false
    const finish = (ip: string | null): void => {
      if (done) return
      done = true
      clearTimeout(timer)
      socket.close()
      resolve(ip)
    }
    const timer = setTimeout(() => finish(null), timeoutMs)
    socket.on('error', () => finish(null))
    socket.on('message', (msg, rinfo) => {
      if (msg.toString() === HERE + secret) finish(rinfo.address)
    })
    socket.bind(0, () => {
      socket.setBroadcast(true)
      const targets = new Set(['127.0.0.1', '255.255.255.255', ...lanInterfaces().map((i) => i.broadcast)])
      for (const target of targets) socket.send(FIND + secret, DISCOVERY_PORT, target, () => undefined)
    })
  })
}
