// Código da sala: IP do host + segredo da sala (23 bits) em base32 Crockford,
// seguidos de 1 caractere de verificação.
//   IPv4 (32 bits):  11 + 1 = 12 caracteres  XXXX-XXXX-XXXX
//   IPv6 (128 bits): 31 + 1 = 32 caracteres  XXXX-XXXX-...-XXXX (8 grupos)
// O IPv6 é usado quando o host está atrás de CGNAT no IPv4.

const ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ'
const SECRET_BITS = 23n
const V4_DATA_CHARS = 11 // 55 bits = 32 + 23
const V6_DATA_CHARS = 31 // 155 bits >= 128 + 23
export const ROOM_SECRET_MAX = 2 ** Number(SECRET_BITS)

export interface RoomAddress {
  ip: string
  secret: number
}

export class RoomCodeError extends Error {}

function parseIPv4(ip: string): bigint {
  const parts = ip.split('.')
  if (parts.length !== 4) throw new RoomCodeError(`IP inválido: ${ip}`)
  let n = 0n
  for (const part of parts) {
    if (!/^\d{1,3}$/.test(part) || Number(part) > 255) throw new RoomCodeError(`IP inválido: ${ip}`)
    n = (n << 8n) | BigInt(part)
  }
  return n
}

function formatIPv4(n: bigint): string {
  return [24n, 16n, 8n, 0n].map((shift) => String((n >> shift) & 255n)).join('.')
}

function parseIPv6(ip: string): bigint {
  const halves = ip.toLowerCase().split('::')
  if (halves.length > 2) throw new RoomCodeError(`IP inválido: ${ip}`)
  const head = halves[0] ? halves[0].split(':') : []
  const tail = halves.length === 2 && halves[1] ? halves[1].split(':') : []
  const missing = 8 - head.length - tail.length
  if (halves.length === 1 ? head.length !== 8 : missing < 1) throw new RoomCodeError(`IP inválido: ${ip}`)
  const groups = [...head, ...Array<string>(halves.length === 2 ? missing : 0).fill('0'), ...tail]
  let n = 0n
  for (const group of groups) {
    if (!/^[0-9a-f]{1,4}$/.test(group)) throw new RoomCodeError(`IP inválido: ${ip}`)
    n = (n << 16n) | BigInt(parseInt(group, 16))
  }
  return n
}

/** Forma curta (RFC 5952): sem zeros à esquerda e a maior sequência de zeros vira "::". */
function formatIPv6(n: bigint): string {
  const groups = Array.from({ length: 8 }, (_, i) => Number((n >> BigInt(112 - 16 * i)) & 0xffffn))
  let bestStart = -1
  let bestLength = 1
  for (let i = 0; i < 8; i++) {
    let length = 0
    while (i + length < 8 && groups[i + length] === 0) length++
    if (length > bestLength) {
      bestStart = i
      bestLength = length
    }
  }
  const hex = groups.map((g) => g.toString(16))
  if (bestStart < 0) return hex.join(':')
  return `${hex.slice(0, bestStart).join(':')}::${hex.slice(bestStart + bestLength).join(':')}`
}

// Soma ponderada mod 31 (primo), com pesos 1..30 (nunca múltiplos de 31): pega
// qualquer caractere trocado e qualquer par de caracteres vizinhos invertido.
function checksum(values: number[]): number {
  return values.reduce((sum, v, i) => sum + ((i % 30) + 1) * v, 0) % 31
}

export function encodeRoomCode({ ip, secret }: RoomAddress): string {
  if (!Number.isInteger(secret) || secret < 0 || secret >= ROOM_SECRET_MAX) {
    throw new RoomCodeError('Segredo da sala inválido')
  }
  const v6 = ip.includes(':')
  let bits = ((v6 ? parseIPv6(ip) : parseIPv4(ip)) << SECRET_BITS) | BigInt(secret)
  const values: number[] = []
  for (let i = 0; i < (v6 ? V6_DATA_CHARS : V4_DATA_CHARS); i++) {
    values.unshift(Number(bits & 31n))
    bits >>= 5n
  }
  values.push(checksum(values))
  return values
    .map((v) => ALPHABET[v])
    .join('')
    .match(/.{4}/g)!
    .join('-')
}

/** Remove separadores e corrige letras que costumam ser confundidas. */
export function normalizeRoomCode(code: string): string {
  return code
    .toUpperCase()
    .replace(/[\s-]/g, '')
    .replace(/O/g, '0')
    .replace(/[IL]/g, '1')
}

export function decodeRoomCode(code: string): RoomAddress {
  const clean = normalizeRoomCode(code)
  const v6 = clean.length === V6_DATA_CHARS + 1
  if (!v6 && clean.length !== V4_DATA_CHARS + 1) throw new RoomCodeError('O código tem 12 ou 32 caracteres')
  const values = [...clean].map((c) => {
    const v = ALPHABET.indexOf(c)
    if (v < 0) throw new RoomCodeError(`Caractere inválido no código: ${c}`)
    return v
  })
  const check = values.pop()!
  if (checksum(values) !== check) throw new RoomCodeError('Código incorreto, confira se digitou certo')
  let bits = 0n
  for (const v of values) bits = (bits << 5n) | BigInt(v)
  const ipBits = bits >> SECRET_BITS
  if (v6 && ipBits >> 128n !== 0n) throw new RoomCodeError('Código incorreto, confira se digitou certo')
  return {
    ip: v6 ? formatIPv6(ipBits) : formatIPv4(ipBits),
    secret: Number(bits & (BigInt(ROOM_SECRET_MAX) - 1n))
  }
}

export function generateRoomSecret(): number {
  const buf = new Uint32Array(1)
  crypto.getRandomValues(buf)
  return buf[0] % ROOM_SECRET_MAX
}
