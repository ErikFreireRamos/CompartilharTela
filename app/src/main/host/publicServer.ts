// Servidor na porta pública 7880, ligado enquanto se está numa sala:
//  - todos:     GET /probe (teste de alcance) e eco UDP na 7882
//  - só o host: POST /api/join e proxy (HTTP e WebSocket) para o LiveKit
import dgram from 'node:dgram'
import http from 'node:http'
import net from 'node:net'
import { LIVEKIT_INTERNAL_PORT, PUBLIC_PORT, RTC_UDP_PORT } from '../../shared/protocol'
import { normalizeRemoteIp } from '../net/ip'
import type { HostController } from './hostController'
import { UDP_PROBE, UDP_PROBE_OK } from './probe'

const MAX_BODY = 16 * 1024

export class PublicServer {
  host: HostController | null = null
  private server: http.Server | null = null
  private udpProbe: dgram.Socket | null = null

  constructor(private readonly identity: () => string) {}

  /** Abre a porta 7880 (IPv4 e IPv6). false se estiver ocupada (outra instância do app). */
  async listen(): Promise<boolean> {
    if (this.server) return true
    // '::' atende IPv4 e IPv6 juntos; sem IPv6 no PC, só IPv4
    return (await this.listenOn('::')) || (await this.listenOn('0.0.0.0'))
  }

  private listenOn(address: string): Promise<boolean> {
    return new Promise((resolve) => {
      const server = http.createServer((req, res) => void this.onRequest(req, res))
      server.on('upgrade', (req, socket, head) => this.onUpgrade(req, socket as net.Socket, head))
      server.once('error', () => resolve(false))
      server.listen({ port: PUBLIC_PORT, host: address, ipv6Only: false }, () => {
        this.server = server
        resolve(true)
      })
    })
  }

  close(): void {
    this.server?.close()
    this.server?.closeAllConnections()
    this.server = null
    this.stopUdpProbe()
  }

  /** Eco UDP na porta de mídia (IPv4 e IPv6); precisa ser fechado antes de virar host. */
  async startUdpProbe(): Promise<boolean> {
    if (this.udpProbe) return true
    return (await this.bindUdpProbe('udp6')) || (await this.bindUdpProbe('udp4'))
  }

  private bindUdpProbe(type: 'udp4' | 'udp6'): Promise<boolean> {
    return new Promise((resolve) => {
      const socket = dgram.createSocket({ type, ipv6Only: false })
      socket.on('message', (msg, rinfo) => {
        const text = msg.toString()
        if (text.startsWith(UDP_PROBE)) {
          socket.send(UDP_PROBE_OK + text.slice(UDP_PROBE.length), rinfo.port, rinfo.address)
        }
      })
      socket.once('error', () => {
        socket.close()
        resolve(false)
      })
      socket.bind(RTC_UDP_PORT, () => {
        this.udpProbe = socket
        resolve(true)
      })
    })
  }

  stopUdpProbe(): void {
    try {
      this.udpProbe?.close()
    } catch {
      // já fechado
    }
    this.udpProbe = null
  }

  private async onRequest(req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
    const url = new URL(req.url ?? '/', 'http://localhost')
    if (req.method === 'GET' && url.pathname === '/probe') {
      return sendJson(res, 200, { nonce: url.searchParams.get('nonce'), id: this.identity() })
    }
    if (url.pathname === '/api/join' && req.method === 'POST') {
      if (!this.host) return sendJson(res, 503, { error: 'not-host' })
      try {
        const body = JSON.parse(await readBody(req))
        const reply = await this.host.handleJoin(body, normalizeRemoteIp(req.socket.remoteAddress))
        return sendJson(res, reply.status, reply.body)
      } catch {
        return sendJson(res, 400, { error: 'bad-request' })
      }
    }
    if (!this.host) return sendJson(res, 503, { error: 'not-host' })
    this.proxyHttp(req, res)
  }

  private proxyHttp(req: http.IncomingMessage, res: http.ServerResponse): void {
    const upstream = http.request(
      { host: '127.0.0.1', port: LIVEKIT_INTERNAL_PORT, method: req.method, path: req.url, headers: req.headers },
      (upRes) => {
        res.writeHead(upRes.statusCode ?? 502, upRes.headers)
        upRes.pipe(res)
      }
    )
    upstream.on('error', () => {
      if (!res.headersSent) res.writeHead(502)
      res.end()
    })
    req.pipe(upstream)
  }

  /** Repassa o WebSocket de sinalização (/rtc) byte a byte para o LiveKit. */
  private onUpgrade(req: http.IncomingMessage, socket: net.Socket, head: Buffer): void {
    if (!this.host) {
      socket.destroy()
      return
    }
    const upstream = net.connect(LIVEKIT_INTERNAL_PORT, '127.0.0.1', () => {
      let raw = `${req.method} ${req.url} HTTP/${req.httpVersion}\r\n`
      for (let i = 0; i < req.rawHeaders.length; i += 2) raw += `${req.rawHeaders[i]}: ${req.rawHeaders[i + 1]}\r\n`
      upstream.write(raw + '\r\n')
      if (head.length) upstream.write(head)
      socket.pipe(upstream).pipe(socket)
    })
    socket.setNoDelay(true)
    upstream.setNoDelay(true)
    const destroy = (): void => {
      socket.destroy()
      upstream.destroy()
    }
    socket.on('error', destroy)
    socket.on('close', destroy)
    upstream.on('error', destroy)
    upstream.on('close', destroy)
  }
}

function sendJson(res: http.ServerResponse, status: number, body: unknown): void {
  res.writeHead(status, { 'Content-Type': 'application/json' })
  res.end(JSON.stringify(body))
}

function readBody(req: http.IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    let size = 0
    const chunks: Buffer[] = []
    req.on('data', (chunk: Buffer) => {
      size += chunk.length
      if (size > MAX_BODY) {
        reject(new Error('body too large'))
        req.destroy()
        return
      }
      chunks.push(chunk)
    })
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')))
    req.on('error', reject)
  })
}
