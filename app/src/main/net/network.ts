import { execFile } from 'node:child_process'
import path from 'node:path'
import { PUBLIC_PORT, RTC_TCP_PORT, RTC_UDP_PORT } from '../../shared/protocol'
import { isPrivateIPv4, lanInterfaces } from './ip'
import { stunPublicIp } from './stun'
import { Upnp, type PortSpec } from './upnp'

export interface NetworkInfo {
  publicIp: string | null
  /** O IP público está direto numa interface deste PC (sem roteador no meio). */
  direct: boolean
  /**
   * O roteador não tem o IP público: a operadora usa CGNAT (ou há outro roteador
   * na frente). Ninguém de fora alcança este PC pelo IPv4, nem com as portas abertas.
   */
  cgnat: boolean
  /** IPv6 público deste PC, se o IPv6 funciona. Contorna o CGNAT, que só existe no IPv4. */
  ipv6: string | null
  lanIps: string[]
}

const HOST_PORTS: PortSpec[] = [
  { port: PUBLIC_PORT, protocol: 'TCP' },
  { port: RTC_TCP_PORT, protocol: 'TCP' },
  { port: RTC_UDP_PORT, protocol: 'UDP' }
]

/**
 * Endereço IPv6 global estável. O Windows também cria endereços temporários
 * (aleatórios, trocados todo dia) e usa esses para sair; para receber
 * conexões preferimos o estável.
 */
function stableIpv6(): Promise<string | null> {
  const powershell = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe')
  const command =
    "Get-NetIPAddress -AddressFamily IPv6 -AddressState Preferred | Where-Object { $_.SuffixOrigin -ne 'Random' -and $_.IPAddress -match '^[23]' } | Select-Object -ExpandProperty IPAddress"
  return new Promise((resolve) =>
    execFile(powershell, ['-NoProfile', '-NonInteractive', '-Command', command], { windowsHide: true, timeout: 8000 }, (err, stdout) =>
      resolve(err ? null : (stdout.split(/\r?\n/).map((line) => line.trim()).find(Boolean) ?? null))
    )
  )
}

export class NetworkManager {
  private readonly upnp = new Upnp()
  /** CT_NO_UPNP=1 não mexe no roteador (testes automatizados). */
  private readonly upnpEnabled = process.env.CT_NO_UPNP !== '1'
  private cached: { info: NetworkInfo; at: number } | null = null

  async resolve(): Promise<NetworkInfo> {
    if (this.cached && Date.now() - this.cached.at < 60_000) return this.cached.info
    const lanIps = lanInterfaces().map((i) => i.address)
    const [routerIp, stunIp, ipv6] = await Promise.all([
      // IP que o roteador recebeu da operadora vs. IP com que a internet nos vê
      this.upnpEnabled ? this.upnp.externalIp() : Promise.resolve(null),
      stunPublicIp(4),
      this.resolveIpv6()
    ])
    let publicIp = stunIp
    if (!publicIp && routerIp && !isPrivateIPv4(routerIp)) publicIp = routerIp
    if (publicIp && isPrivateIPv4(publicIp)) publicIp = null
    const cgnat = !!routerIp && !!publicIp && routerIp !== publicIp
    const info: NetworkInfo = { publicIp, direct: !!publicIp && lanIps.includes(publicIp), cgnat, ipv6, lanIps }
    this.cached = { info, at: Date.now() }
    return info
  }

  private async resolveIpv6(): Promise<string | null> {
    const seen = await stunPublicIp(6)
    if (!seen) return null
    return (await stableIpv6()) ?? seen
  }

  /** Abre as portas de host no roteador. true se ficaram acessíveis. */
  async openPorts(): Promise<boolean> {
    const info = await this.resolve()
    if (info.direct) return true
    if (info.cgnat) return false // abrir porta no roteador não adianta
    return this.upnpEnabled && this.upnp.mapPorts(HOST_PORTS)
  }

  async closePorts(): Promise<void> {
    await this.upnp.unmapAll()
  }
}
