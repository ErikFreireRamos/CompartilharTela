import { PUBLIC_PORT, RTC_TCP_PORT, RTC_UDP_PORT } from '../../shared/protocol'
import { isPrivateIPv4, lanInterfaces } from './ip'
import { stunPublicIp } from './stun'
import { Upnp, type PortSpec } from './upnp'

export interface NetworkInfo {
  publicIp: string | null
  /** O IP público está direto numa interface deste PC (sem roteador no meio). */
  direct: boolean
  lanIps: string[]
}

const HOST_PORTS: PortSpec[] = [
  { port: PUBLIC_PORT, protocol: 'TCP' },
  { port: RTC_TCP_PORT, protocol: 'TCP' },
  { port: RTC_UDP_PORT, protocol: 'UDP' }
]

export class NetworkManager {
  private readonly upnp = new Upnp()
  /** CT_NO_UPNP=1 não mexe no roteador (testes automatizados). */
  private readonly upnpEnabled = process.env.CT_NO_UPNP !== '1'
  private cached: { info: NetworkInfo; at: number } | null = null

  async resolve(): Promise<NetworkInfo> {
    if (this.cached && Date.now() - this.cached.at < 60_000) return this.cached.info
    const lanIps = lanInterfaces().map((i) => i.address)
    let publicIp = this.upnpEnabled ? await this.upnp.externalIp() : null
    if (!publicIp || isPrivateIPv4(publicIp)) publicIp = await stunPublicIp()
    if (publicIp && isPrivateIPv4(publicIp)) publicIp = null
    const info: NetworkInfo = { publicIp, direct: !!publicIp && lanIps.includes(publicIp), lanIps }
    this.cached = { info, at: Date.now() }
    return info
  }

  /** Abre as portas de host no roteador. true se ficaram acessíveis. */
  async openPorts(): Promise<boolean> {
    const info = await this.resolve()
    if (info.direct) return true
    return this.upnpEnabled && this.upnp.mapPorts(HOST_PORTS)
  }

  async closePorts(): Promise<void> {
    await this.upnp.unmapAll()
  }
}
