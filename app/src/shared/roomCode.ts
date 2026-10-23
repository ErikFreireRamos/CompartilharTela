// Código da sala: IPv4 do host (32 bits) + segredo da sala (23 bits) em 11
// caracteres base32 Crockford, seguidos de 1 caractere de verificação.
// Formato exibido: XXXX-XXXX-XXXX

const ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ'
const SECRET_BITS = 23
const DATA_CHARS = 11
export const ROOM_SECRET_MAX = 2 ** SECRET_BITS

export interface RoomAddress {
  ip: string
  secret: number
}

export class RoomCodeError extends Error {}

function ipToInt(ip: string): number {
  const parts = ip.split('.')
  if (parts.length !== 4) throw new RoomCodeError(`IP inválido: ${ip}`)
  let n = 0
  for (const part of parts) {
    if (!/^\d{1,3}$/.test(part) || Number(part) > 255) throw new RoomCodeError(`IP inválido: ${ip}`)
    n = n * 256 + Number(part)
  }
  return n
}

function intToIp(n: number): string {
  return [24, 16, 8, 0].map((shift) => Math.floor(n / 2 ** shift) % 256).join('.')
}

// Soma ponderada mod 31 (primo): pega qualquer caractere trocado e
// qualquer par de caracteres vizinhos invertido.
function checksum(values: number[]): number {
  return values.reduce((sum, v, i) => sum + (i + 1) * v, 0) % 31
}

export function encodeRoomCode({ ip, secret }: RoomAddress): string {
  if (!Number.isInteger(secret) || secret < 0 || secret >= ROOM_SECRET_MAX) {
    throw new RoomCodeError('Segredo da sala inválido')
  }
  let bits = (BigInt(ipToInt(ip)) << BigInt(SECRET_BITS)) | BigInt(secret)
  const values: number[] = []
  for (let i = 0; i < DATA_CHARS; i++) {
    values.unshift(Number(bits & 31n))
    bits >>= 5n
  }
  values.push(checksum(values))
  const chars = values.map((v) => ALPHABET[v]).join('')
  return `${chars.slice(0, 4)}-${chars.slice(4, 8)}-${chars.slice(8)}`
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
  if (clean.length !== DATA_CHARS + 1) throw new RoomCodeError('O código tem 12 caracteres')
  const values = [...clean].map((c) => {
    const v = ALPHABET.indexOf(c)
    if (v < 0) throw new RoomCodeError(`Caractere inválido no código: ${c}`)
    return v
  })
  const check = values.pop()!
  if (checksum(values) !== check) throw new RoomCodeError('Código incorreto, confira se digitou certo')
  let bits = 0n
  for (const v of values) bits = (bits << 5n) | BigInt(v)
  return {
    ip: intToIp(Number(bits >> BigInt(SECRET_BITS))),
    secret: Number(bits & BigInt(ROOM_SECRET_MAX - 1))
  }
}

export function generateRoomSecret(): number {
  const buf = new Uint32Array(1)
  crypto.getRandomValues(buf)
  return buf[0] % ROOM_SECRET_MAX
}
