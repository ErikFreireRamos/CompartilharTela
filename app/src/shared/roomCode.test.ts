import { describe, expect, it } from 'vitest'
import { decodeRoomCode, encodeRoomCode, generateRoomSecret, normalizeRoomCode, ROOM_SECRET_MAX } from './roomCode'

describe('roomCode', () => {
  it('ida e volta preserva IP e segredo', () => {
    const cases = [
      { ip: '0.0.0.0', secret: 0 },
      { ip: '255.255.255.255', secret: ROOM_SECRET_MAX - 1 },
      { ip: '189.45.7.200', secret: 123456 },
      { ip: '192.168.0.10', secret: 1 }
    ]
    for (const c of cases) {
      const code = encodeRoomCode(c)
      expect(code).toMatch(/^[0-9A-Z]{4}-[0-9A-Z]{4}-[0-9A-Z]{4}$/)
      expect(decodeRoomCode(code)).toEqual(c)
    }
  })

  it('aceita minúsculas, sem traços e letras confundíveis', () => {
    const code = encodeRoomCode({ ip: '10.0.0.1', secret: 42 })
    const messy = code.toLowerCase().replace(/-/g, ' ').replace(/0/g, 'o').replace(/1/g, 'l')
    expect(decodeRoomCode(messy)).toEqual({ ip: '10.0.0.1', secret: 42 })
  })

  it('detecta qualquer caractere trocado', () => {
    const code = normalizeRoomCode(encodeRoomCode({ ip: '177.12.99.3', secret: 7654321 }))
    const alphabet = '0123456789ABCDEFGHJKMNPQRSTVWXYZ'
    for (let i = 0; i < code.length; i++) {
      for (const c of alphabet) {
        if (c === code[i]) continue
        const typo = code.slice(0, i) + c + code.slice(i + 1)
        // o único caso não detectável é trocar valores que diferem em 31 (0 <-> Z)
        const diff = Math.abs(alphabet.indexOf(c) - alphabet.indexOf(code[i]))
        if (diff === 31) continue
        expect(() => decodeRoomCode(typo)).toThrow()
      }
    }
  })

  it('detecta caracteres vizinhos invertidos', () => {
    const code = normalizeRoomCode(encodeRoomCode({ ip: '200.100.50.25', secret: 99999 }))
    for (let i = 0; i < code.length - 1; i++) {
      if (code[i] === code[i + 1]) continue
      const swapped = code.slice(0, i) + code[i + 1] + code[i] + code.slice(i + 2)
      expect(() => decodeRoomCode(swapped)).toThrow()
    }
  })

  it('rejeita tamanho errado', () => {
    expect(() => decodeRoomCode('ABCD-EFGH')).toThrow(/12 caracteres/)
  })

  it('gera segredo dentro do limite', () => {
    for (let i = 0; i < 100; i++) {
      const s = generateRoomSecret()
      expect(s).toBeGreaterThanOrEqual(0)
      expect(s).toBeLessThan(ROOM_SECRET_MAX)
    }
  })
})
