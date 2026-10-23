import os from 'node:os'

export interface LanInterface {
  address: string
  broadcast: string
}

function toInt(ip: string): number {
  return ip.split('.').reduce((n, part) => n * 256 + Number(part), 0)
}

function toIp(n: number): string {
  return [24, 16, 8, 0].map((shift) => (n >>> shift) & 255).join('.')
}

/** Faixas que não são alcançáveis pela internet (inclui CGNAT 100.64/10). */
export function isPrivateIPv4(ip: string): boolean {
  const [a, b] = ip.split('.').map(Number)
  return (
    a === 10 ||
    a === 127 ||
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168) ||
    (a === 169 && b === 254) ||
    (a === 100 && b >= 64 && b <= 127)
  )
}

/** Rede doméstica comum primeiro; VPNs (Radmin 26.x, Hamachi 25.x...) por último. */
function lanPriority(ip: string): number {
  if (ip.startsWith('192.168.')) return 0
  if (ip.startsWith('10.')) return 1
  if (isPrivateIPv4(ip)) return 2
  return 3
}

export function lanInterfaces(): LanInterface[] {
  const result: LanInterface[] = []
  for (const list of Object.values(os.networkInterfaces())) {
    for (const info of list ?? []) {
      if (info.family !== 'IPv4' || info.internal) continue
      const mask = toInt(info.netmask)
      result.push({ address: info.address, broadcast: toIp((toInt(info.address) & mask) | (~mask >>> 0)) })
    }
  }
  return result.sort((a, b) => lanPriority(a.address) - lanPriority(b.address))
}

/** "::ffff:1.2.3.4" -> "1.2.3.4" */
export function normalizeRemoteIp(address: string | undefined): string {
  if (!address) return ''
  return address.startsWith('::ffff:') ? address.slice(7) : address === '::1' ? '127.0.0.1' : address
}
