import type { RoomMeta } from '../../../shared/protocol'

/**
 * Quem pode assumir como host, em ordem. Todos os apps calculam a mesma lista
 * a partir dos metadados da sala, então chegam ao mesmo sucessor sem conversar.
 */
export function successorCandidates(meta: RoomMeta | null, present: Iterable<string>, exclude: string[] = []): string[] {
  if (!meta) return []
  const presentSet = new Set(present)
  const excluded = new Set([meta.hostId, ...exclude])
  return meta.queue.filter((id) => presentSet.has(id) && !excluded.has(id))
}
