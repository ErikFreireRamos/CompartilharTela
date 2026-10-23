// Descobre o IP público com uma requisição STUN (RFC 5389), pelo IPv4 ou pelo
// IPv6. Servidores públicos, sem conta.
import dgram from 'node:dgram'
import { randomBytes } from 'node:crypto'

const MAGIC_COOKIE = 0x2112a442
const SERVERS: Array<[string, number]> = [
  ['stun.l.google.com', 19302],
  ['stun.cloudflare.com', 3478],
  ['stun1.l.google.com', 19302]
]

function formatIPv6(bytes: Buffer): string {
  const groups: string[] = []
  for (let i = 0; i < 16; i += 2) groups.push(bytes.readUInt16BE(i).toString(16))
  return groups.join(':')
}

function parseMappedAddress(msg: Buffer, transactionId: Buffer): string | null {
  const end = Math.min(msg.length, 20 + msg.readUInt16BE(2))
  let offset = 20
  while (offset + 4 <= end) {
    const type = msg.readUInt16BE(offset)
    const length = msg.readUInt16BE(offset + 2)
    const value = offset + 4
    const xor = type === 0x0020
    if (xor || type === 0x0001) {
      const family = msg[value + 1]
      if (family === 0x01) {
        const raw = msg.readUInt32BE(value + 4)
        const ip = xor ? (raw ^ MAGIC_COOKIE) >>> 0 : raw
        return [24, 16, 8, 0].map((shift) => (ip >>> shift) & 255).join('.')
      }
      if (family === 0x02) {
        const bytes = Buffer.from(msg.subarray(value + 4, value + 20))
        if (xor) {
          // IPv6: XOR com o magic cookie seguido do transaction id
          const mask = Buffer.concat([msg.subarray(4, 8), transactionId])
          for (let i = 0; i < 16; i++) bytes[i] ^= mask[i]
        }
        return formatIPv6(bytes)
      }
    }
    offset = value + length + ((4 - (length % 4)) % 4)
  }
  return null
}

function query(host: string, port: number, family: 4 | 6, timeoutMs: number): Promise<string | null> {
  return new Promise((resolve) => {
    const socket = dgram.createSocket(family === 6 ? 'udp6' : 'udp4')
    const transactionId = randomBytes(12)
    const request = Buffer.alloc(20)
    request.writeUInt16BE(0x0001, 0)
    request.writeUInt16BE(0, 2)
    request.writeUInt32BE(MAGIC_COOKIE, 4)
    transactionId.copy(request, 8)

    let finished = false
    const finish = (ip: string | null): void => {
      if (finished) return
      finished = true
      clearTimeout(timer)
      socket.close()
      resolve(ip)
    }
    const timer = setTimeout(() => finish(null), timeoutMs)
    socket.on('error', () => finish(null))
    socket.on('message', (msg) => {
      if (msg.length < 20 || msg.readUInt16BE(0) !== 0x0101) return
      if (!msg.subarray(8, 20).equals(transactionId)) return
      finish(parseMappedAddress(msg, transactionId))
    })
    socket.send(request, port, host, (err) => err && finish(null))
  })
}

/** IP com que a internet vê este PC. Com family 6, também confirma que o IPv6 funciona. */
export async function stunPublicIp(family: 4 | 6 = 4, timeoutMs = 1500): Promise<string | null> {
  for (const [host, port] of SERVERS) {
    const ip = await query(host, port, family, timeoutMs)
    if (ip) return ip
  }
  return null
}
