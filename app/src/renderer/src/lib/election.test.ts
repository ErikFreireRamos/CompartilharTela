import { describe, expect, it } from 'vitest'
import type { RoomMeta } from '../../../shared/protocol'
import { successorCandidates } from './election'

const meta: RoomMeta = {
  v: 1,
  epoch: 3,
  hostId: 'host',
  hostName: 'Host',
  code: 'XXXX-XXXX-XXXX',
  scope: 'internet',
  queue: ['ana', 'bia', 'caio'],
  maxQuality: null
}

describe('successorCandidates', () => {
  it('segue a ordem da fila e ignora quem saiu', () => {
    expect(successorCandidates(meta, ['caio', 'ana', 'host', 'dani'])).toEqual(['ana', 'caio'])
  })

  it('nunca escolhe o host atual nem os excluídos', () => {
    const withHost = { ...meta, queue: ['host', 'ana', 'bia'] }
    expect(successorCandidates(withHost, ['host', 'ana', 'bia'], ['ana'])).toEqual(['bia'])
  })

  it('sem metadados não há candidatos', () => {
    expect(successorCandidates(null, ['ana'])).toEqual([])
  })
})
