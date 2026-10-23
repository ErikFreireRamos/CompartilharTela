// Teste de integração com o livekit-server.exe de verdade.
// Rode com: INTEGRATION=1 npx vitest run src/main/host (usa as portas 7880–7882 e 7890)
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { PUBLIC_PORT, ROOM_NAME } from '../../shared/protocol'
import { HostController } from './hostController'
import { PublicServer } from './publicServer'

const exe = path.join(__dirname, '..', '..', '..', 'resources', 'bin', 'livekit-server.exe')
const enabled = process.env.INTEGRATION === '1' && fs.existsSync(exe)
const base = `http://127.0.0.1:${PUBLIC_PORT}`

async function join(body: Record<string, unknown>): Promise<{ status: number; body: Record<string, unknown> }> {
  const res = await fetch(`${base}/api/join`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body)
  })
  return { status: res.status, body: (await res.json()) as Record<string, unknown> }
}

describe.skipIf(!enabled)('host com LiveKit real', () => {
  const workDir = fs.mkdtempSync(path.join(os.tmpdir(), 'ct-host-'))
  const host = new HostController({ exe, workDir })
  const server = new PublicServer(() => 'host-id')

  beforeAll(async () => {
    await host.start({ secret: 4242, epoch: 1, hostId: 'host-id', hostName: 'Host', publicIp: null, nodeIp: null, code: 'TEST' })
    expect(await server.listen()).toBe(true)
    server.host = host
  }, 30_000)

  afterAll(() => {
    server.close()
    host.stop()
  })

  it('responde ao teste de alcance', async () => {
    const res = await fetch(`${base}/probe?nonce=abc`)
    expect(await res.json()).toEqual({ nonce: 'abc', id: 'host-id' })
  })

  it('recusa código errado', async () => {
    const res = await join({ secret: 1, identity: 'ana', name: 'Ana' })
    expect(res.status).toBe(403)
  })

  it('emite token e o WebSocket passa pelo proxy até o LiveKit', async () => {
    const res = await join({ secret: 4242, identity: 'ana', name: 'Ana' })
    expect(res.status).toBe(200)
    expect(res.body.code).toBe('TEST')
    const token = String(res.body.token)

    // a raiz HTTP também é repassada ao LiveKit
    expect((await fetch(`${base}/`)).status).toBe(200)

    const ws = new WebSocket(`ws://127.0.0.1:${PUBLIC_PORT}/rtc?access_token=${token}&auto_subscribe=1&protocol=16`)
    ws.binaryType = 'arraybuffer'
    const firstMessage = await new Promise<ArrayBuffer>((resolve, reject) => {
      ws.onmessage = (e) => resolve(e.data as ArrayBuffer)
      ws.onerror = () => reject(new Error('websocket falhou'))
      setTimeout(() => reject(new Error('sem resposta do LiveKit')), 10_000)
    })
    // a primeira mensagem é o JoinResponse (protobuf) com o nome da sala
    expect(Buffer.from(firstMessage).includes(Buffer.from(ROOM_NAME))).toBe(true)
    ws.close()
  }, 20_000)
})
