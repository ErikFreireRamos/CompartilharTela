// Baixa o livekit-server.exe oficial (GitHub releases), confere o SHA-256 e
// coloca em app/resources/bin. Uso: node scripts/download-livekit.mjs [--force]
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const VERSION = '1.13.10'
const ASSET = `livekit_${VERSION}_windows_amd64.zip`
const BASE = `https://github.com/livekit/livekit/releases/download/v${VERSION}`

const here = path.dirname(fileURLToPath(import.meta.url))
const binDir = path.join(here, '..', 'app', 'resources', 'bin')
const target = path.join(binDir, 'livekit-server.exe')

if (fs.existsSync(target) && !process.argv.includes('--force')) {
  console.log(`livekit-server.exe já existe em ${target} (use --force para baixar de novo)`)
  process.exit(0)
}

console.log(`Baixando LiveKit ${VERSION}...`)
const [zipRes, sumsRes] = await Promise.all([fetch(`${BASE}/${ASSET}`), fetch(`${BASE}/checksums.txt`)])
if (!zipRes.ok || !sumsRes.ok) throw new Error(`Falha no download (${zipRes.status} / ${sumsRes.status})`)
const zip = Buffer.from(await zipRes.arrayBuffer())
const expected = (await sumsRes.text())
  .split('\n')
  .find((line) => line.trim().endsWith(ASSET))
  ?.split(/\s+/)[0]
const actual = createHash('sha256').update(zip).digest('hex')
if (!expected || expected !== actual) {
  throw new Error(`SHA-256 não confere para ${ASSET}: esperado ${expected}, obtido ${actual}`)
}

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'livekit-'))
try {
  const zipPath = path.join(tmp, ASSET)
  const extractDir = path.join(tmp, 'extract')
  fs.writeFileSync(zipPath, zip)
  execFileSync('powershell', [
    '-NoProfile',
    '-Command',
    `Expand-Archive -LiteralPath '${zipPath}' -DestinationPath '${extractDir}' -Force`
  ])
  const exe = fs
    .readdirSync(extractDir, { recursive: true })
    .map(String)
    .find((f) => path.basename(f).toLowerCase() === 'livekit-server.exe')
  if (!exe) throw new Error('livekit-server.exe não encontrado dentro do zip')
  fs.mkdirSync(binDir, { recursive: true })
  fs.copyFileSync(path.join(extractDir, exe), target)
  console.log(`OK: ${target}`)
} finally {
  fs.rmSync(tmp, { recursive: true, force: true })
}
