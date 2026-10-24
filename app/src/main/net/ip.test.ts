import { describe, expect, it } from 'vitest'
import { isCgnatIPv4, isPrivateIPv4 } from './ip'

describe('faixas de IPv4', () => {
  it('reconhece o CGNAT (100.64/10)', () => {
    expect(isCgnatIPv4('100.64.0.1')).toBe(true)
    expect(isCgnatIPv4('100.68.157.232')).toBe(true)
    expect(isCgnatIPv4('100.127.255.255')).toBe(true)
    expect(isCgnatIPv4('100.63.255.255')).toBe(false)
    expect(isCgnatIPv4('100.128.0.0')).toBe(false)
    expect(isCgnatIPv4('177.37.143.208')).toBe(false)
  })

  it('CGNAT também é uma faixa não alcançável de fora', () => {
    expect(isPrivateIPv4('100.68.157.232')).toBe(true)
    expect(isPrivateIPv4('177.37.143.208')).toBe(false)
  })
})
