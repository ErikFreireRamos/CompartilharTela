// Cliente UPnP IGD mínimo: acha o roteador (SSDP), pega o IP externo e abre
// ou fecha portas. Sem dependências e sem nada para configurar.
import dgram from 'node:dgram'
import http from 'node:http'

export type Protocol = 'TCP' | 'UDP'
export interface PortSpec {
  port: number
  protocol: Protocol
}

interface Gateway {
  controlUrl: string
  serviceType: string
  /** IP deste PC na interface que fala com o roteador. */
  localAddress: string
}

class UpnpError extends Error {
  constructor(
    message: string,
    readonly code?: number
  ) {
    super(message)
  }
}

const SSDP_ADDRESS = '239.255.255.250'
const SSDP_PORT = 1900
const SEARCH_TARGETS = [
  'urn:schemas-upnp-org:device:InternetGatewayDevice:1',
  'urn:schemas-upnp-org:device:InternetGatewayDevice:2',
  'urn:schemas-upnp-org:service:WANIPConnection:1',
  'urn:schemas-upnp-org:service:WANIPConnection:2',
  'urn:schemas-upnp-org:service:WANPPPConnection:1'
]
const DESCRIPTION = 'CompartilhadorDeTela'
const RENEW_LEASE_SECONDS = 3600

function ssdpSearch(timeoutMs: number): Promise<string[]> {
  return new Promise((resolve) => {
    const socket = dgram.createSocket({ type: 'udp4', reuseAddr: true })
    const locations = new Set<string>()
    let graceTimer: NodeJS.Timeout | null = null
    const finish = (): void => {
      clearTimeout(timer)
      if (graceTimer) clearTimeout(graceTimer)
      try {
        socket.close()
      } catch {
        // já fechado
      }
      resolve([...locations])
    }
    const timer = setTimeout(finish, timeoutMs)
    socket.on('error', finish)
    socket.on('message', (msg) => {
      const match = /^location:\s*(\S+)/im.exec(msg.toString())
      if (!match) return
      locations.add(match[1])
      // espera um pouco por outras respostas e encerra
      graceTimer ??= setTimeout(finish, 400)
    })
    socket.bind(0, () => {
      for (const st of SEARCH_TARGETS) {
        const request =
          'M-SEARCH * HTTP/1.1\r\n' +
          `HOST: ${SSDP_ADDRESS}:${SSDP_PORT}\r\n` +
          'MAN: "ssdp:discover"\r\n' +
          'MX: 2\r\n' +
          `ST: ${st}\r\n\r\n`
        socket.send(request, SSDP_PORT, SSDP_ADDRESS)
      }
    })
  })
}

function httpRequest(
  url: string,
  options: { method?: string; headers?: Record<string, string>; body?: string; timeoutMs: number }
): Promise<{ status: number; body: string; localAddress: string }> {
  return new Promise((resolve, reject) => {
    const req = http.request(url, { method: options.method ?? 'GET', headers: options.headers }, (res) => {
      const chunks: Buffer[] = []
      res.on('data', (c: Buffer) => chunks.push(c))
      res.on('end', () =>
        resolve({
          status: res.statusCode ?? 0,
          body: Buffer.concat(chunks).toString('utf8'),
          localAddress: req.socket?.localAddress?.replace('::ffff:', '') ?? ''
        })
      )
      res.on('error', reject)
    })
    req.setTimeout(options.timeoutMs, () => req.destroy(new Error('timeout')))
    req.on('error', reject)
    req.end(options.body)
  })
}

function xmlValue(xml: string, tag: string): string | null {
  const match = new RegExp(`<(?:\\w+:)?${tag}>\\s*([^<]*?)\\s*</(?:\\w+:)?${tag}>`, 'i').exec(xml)
  return match ? match[1] : null
}

async function soap(gw: Gateway, action: string, args: Record<string, string | number>): Promise<string> {
  const argsXml = Object.entries(args)
    .map(([k, v]) => `<${k}>${v}</${k}>`)
    .join('')
  const body =
    '<?xml version="1.0"?>' +
    '<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">' +
    `<s:Body><u:${action} xmlns:u="${gw.serviceType}">${argsXml}</u:${action}></s:Body></s:Envelope>`
  const res = await httpRequest(gw.controlUrl, {
    method: 'POST',
    headers: {
      'Content-Type': 'text/xml; charset="utf-8"',
      SOAPAction: `"${gw.serviceType}#${action}"`
    },
    body,
    timeoutMs: 3000
  })
  if (res.status !== 200) {
    const code = Number(xmlValue(res.body, 'errorCode') ?? 0)
    throw new UpnpError(`${action} falhou (${xmlValue(res.body, 'errorDescription') ?? res.status})`, code)
  }
  return res.body
}

async function loadGateways(location: string): Promise<Gateway[]> {
  const { body, localAddress } = await httpRequest(location, { timeoutMs: 3000 })
  const base = xmlValue(body, 'URLBase') || location
  const gateways: Gateway[] = []
  for (const match of body.matchAll(/<service>([\s\S]*?)<\/service>/gi)) {
    const serviceType = xmlValue(match[1], 'serviceType')
    const controlUrl = xmlValue(match[1], 'controlURL')
    if (!serviceType || !controlUrl || !/WAN(IP|PPP)Connection:\d/.test(serviceType)) continue
    gateways.push({ serviceType, controlUrl: new URL(controlUrl, base).toString(), localAddress })
  }
  // WANIPConnection primeiro; PPP só se for a conexão ativa
  return gateways.sort((a, b) => Number(b.serviceType.includes('WANIP')) - Number(a.serviceType.includes('WANIP')))
}

export class Upnp {
  private gateway: Gateway | null = null
  private externalAddress: string | null = null
  private lastAttempt = 0
  private mapped: PortSpec[] = []
  private renewTimer: NodeJS.Timeout | null = null

  /** Acha um roteador que responda ao GetExternalIPAddress. */
  async discover(): Promise<boolean> {
    if (this.gateway) return true
    if (Date.now() - this.lastAttempt < 30_000) return false
    this.lastAttempt = Date.now()
    for (const location of await ssdpSearch(2500)) {
      let candidates: Gateway[] = []
      try {
        candidates = await loadGateways(location)
      } catch {
        continue
      }
      for (const gw of candidates) {
        try {
          const ip = xmlValue(await soap(gw, 'GetExternalIPAddress', {}), 'NewExternalIPAddress')
          if (ip && /^\d+\.\d+\.\d+\.\d+$/.test(ip) && ip !== '0.0.0.0') {
            this.gateway = gw
            this.externalAddress = ip
            return true
          }
        } catch {
          // tenta o próximo serviço
        }
      }
    }
    return false
  }

  async externalIp(): Promise<string | null> {
    return (await this.discover()) ? this.externalAddress : null
  }

  async mapPorts(ports: PortSpec[]): Promise<boolean> {
    if (!(await this.discover()) || !this.gateway) return false
    let allOk = true
    let needsRenew = false
    for (const spec of ports) {
      const result = await this.mapOne(this.gateway, spec)
      if (result === 'failed') allOk = false
      else {
        if (result === 'lease') needsRenew = true
        if (!this.mapped.some((m) => m.port === spec.port && m.protocol === spec.protocol)) this.mapped.push(spec)
      }
    }
    if (needsRenew && !this.renewTimer) {
      this.renewTimer = setInterval(() => void this.renew(), (RENEW_LEASE_SECONDS - 600) * 1000)
    }
    return allOk
  }

  private async mapOne(gw: Gateway, spec: PortSpec): Promise<'permanent' | 'lease' | 'failed'> {
    const args = (lease: number): Record<string, string | number> => ({
      NewRemoteHost: '',
      NewExternalPort: spec.port,
      NewProtocol: spec.protocol,
      NewInternalPort: spec.port,
      NewInternalClient: gw.localAddress,
      NewEnabled: 1,
      NewPortMappingDescription: DESCRIPTION,
      NewLeaseDuration: lease
    })
    try {
      await soap(gw, 'AddPortMapping', args(0))
      return 'permanent'
    } catch {
      // alguns roteadores (IGDv2) não aceitam mapeamento permanente
    }
    try {
      await soap(gw, 'AddPortMapping', args(RENEW_LEASE_SECONDS))
      return 'lease'
    } catch {
      // pode já existir um mapeamento nosso
    }
    try {
      const entry = await soap(gw, 'GetSpecificPortMappingEntry', {
        NewRemoteHost: '',
        NewExternalPort: spec.port,
        NewProtocol: spec.protocol
      })
      if (xmlValue(entry, 'NewInternalClient') === gw.localAddress) return 'permanent'
    } catch {
      // sem mapeamento
    }
    return 'failed'
  }

  private async renew(): Promise<void> {
    if (!this.gateway) return
    for (const spec of this.mapped) await this.mapOne(this.gateway, spec)
  }

  async unmapAll(): Promise<void> {
    if (this.renewTimer) clearInterval(this.renewTimer)
    this.renewTimer = null
    const gw = this.gateway
    const mapped = this.mapped
    this.mapped = []
    if (!gw) return
    await Promise.allSettled(
      mapped.map((spec) =>
        soap(gw, 'DeletePortMapping', { NewRemoteHost: '', NewExternalPort: spec.port, NewProtocol: spec.protocol })
      )
    )
  }
}
