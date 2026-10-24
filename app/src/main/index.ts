import { execFile } from 'node:child_process'
import path from 'node:path'
import { app, BrowserWindow, clipboard, desktopCapturer, ipcMain, session } from 'electron'
import type { CandidateInfo, JoinInfo, Settings } from '../shared/api'
import { PUBLIC_PORT, type QualityId } from '../shared/protocol'
import { decodeRoomCode, encodeRoomCode, generateRoomSecret } from '../shared/roomCode'
import { AudioCaptureService } from './audio/audioCapture'
import { HostController } from './host/hostController'
import { PublicServer } from './host/publicServer'
import { urlHost } from './net/ip'
import { findOnLan, LanResponder } from './net/lanDiscovery'
import { NetworkManager } from './net/network'
import { closeAllPopouts, registerPopoutIpc } from './popout'
import { SettingsStore } from './settings'

// Perfil separado para testar duas cópias no mesmo PC: electron . --profile=b
const profileArg = process.argv.find((a) => a.startsWith('--profile='))
if (profileArg) {
  app.setPath('userData', path.join(app.getPath('appData'), `CompartilhadorDeTela-${profileArg.slice('--profile='.length)}`))
}
app.commandLine.appendSwitch('autoplay-policy', 'no-user-gesture-required')
// Por padrão o Chromium codifica compartilhamento de tela por software. Isto libera
// o H.264 por hardware (Quick Sync / NVENC / AMF), que pesa bem menos para quem joga;
// sem hardware disponível ele volta sozinho para o OpenH264.
app.commandLine.appendSwitch('enable-features', 'PlatformH264CbpEncoding')
app.commandLine.appendSwitch('disable-features', 'WebRtcScreenshareSwEncoding')

const binDir = app.isPackaged ? path.join(process.resourcesPath, 'bin') : path.join(app.getAppPath(), 'resources', 'bin')

const JOIN_ERRORS: Record<string, string> = {
  'wrong-code': 'Código incorreto ou sala encerrada.',
  'room-full': 'A sala está cheia (máximo de 10 pessoas).',
  'too-many-attempts': 'Muitas tentativas com código errado. Espere alguns minutos.',
  'not-host': 'Esse PC não está mais hospedando a sala.'
}

let settings: SettingsStore
let win: BrowserWindow | null = null
let publicServer: PublicServer
let audio: AudioCaptureService
const network = new NetworkManager()
let host: HostController | null = null
let lanResponder: LanResponder | null = null
let pendingShare: { sourceId: string; systemAudio: boolean } | null = null
let inRoom = false
let quitting = false
let cleanedUp = false

function clientId(): string {
  return settings.get().clientId
}

/** "window:327750:0" -> 327750 (o HWND que o Windows usa para achar o processo). */
function windowHandleOf(sourceId: string): number {
  const match = /^window:(\d+)/.exec(sourceId)
  return match ? Number(match[1]) : 0
}

let elevated: Promise<boolean> | null = null
/** Nível de integridade alto (S-1-16-12288) ou de sistema = rodando como administrador. */
function isElevated(): Promise<boolean> {
  // caminho completo: no PATH pode haver outro whoami (ex.: o do Git)
  const whoami = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'whoami.exe')
  elevated ??= new Promise((resolve) =>
    execFile(whoami, ['/groups'], { windowsHide: true }, (err, stdout) =>
      resolve(!err && /S-1-16-(12288|16384)\b/.test(stdout))
    )
  )
  return elevated
}

function createWindow(): void {
  win = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 960,
    minHeight: 600,
    backgroundColor: '#101216',
    title: 'CompartilhadorDeTela',
    autoHideMenuBar: true,
    webPreferences: {
      preload: path.join(__dirname, '../preload/index.js'),
      sandbox: true,
      contextIsolation: true,
      // quem transmite costuma minimizar o app ou jogar em tela cheia por cima
      backgroundThrottling: false
    }
  })
  win.on('close', (e) => {
    if (!inRoom || quitting) return
    // deixa a interface passar o host para outra pessoa antes de fechar
    e.preventDefault()
    win?.webContents.send('app:close-requested')
    setTimeout(() => {
      quitting = true
      app.quit()
    }, 8000)
  })
  win.on('closed', () => {
    win = null
    closeAllPopouts()
  })
  win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
  if (process.env.ELECTRON_RENDERER_URL) void win.loadURL(process.env.ELECTRON_RENDERER_URL)
  else void win.loadFile(path.join(__dirname, '../renderer/index.html'))
}

async function stopHosting(): Promise<void> {
  lanResponder?.close()
  lanResponder = null
  publicServer.host = null
  host?.stop()
  host = null
}

async function becomeHost(secret: number, epoch: number, nickname: string): Promise<JoinInfo> {
  await stopHosting()
  publicServer.stopUdpProbe() // o LiveKit precisa da porta UDP
  if (!(await publicServer.listen())) {
    throw new Error(`A porta ${PUBLIC_PORT} já está em uso. Há outra cópia do app aberta?`)
  }
  const netInfo = await network.resolve()
  // IPv4 público quando dá; com CGNAT, IPv6 (o CGNAT só existe no IPv4); senão, só rede local
  const viaIpv4 = !!netInfo.publicIp && !netInfo.cgnat
  const publicIp = viaIpv4 ? netInfo.publicIp : netInfo.ipv6
  const portsOpen = viaIpv4 ? await network.openPorts() : false
  const ip = publicIp ?? netInfo.lanIps[0] ?? '127.0.0.1'
  const code = encodeRoomCode({ ip, secret })
  const ctl = new HostController({
    exe: path.join(binDir, 'livekit-server.exe'),
    workDir: path.join(app.getPath('userData'), 'livekit')
  })
  await ctl.start({
    secret,
    epoch,
    hostId: clientId(),
    hostName: nickname,
    publicIp,
    nodeIp: viaIpv4 ? netInfo.publicIp : null,
    code
  })
  host = ctl
  publicServer.host = ctl
  lanResponder = await LanResponder.start(secret)
  inRoom = true
  return {
    url: `ws://127.0.0.1:${PUBLIC_PORT}`,
    token: await ctl.mintToken(clientId(), nickname),
    identity: clientId(),
    endpoint: ip,
    code,
    secret,
    isHost: true,
    portsOpen,
    cgnat: netInfo.cgnat,
    scope: ctl.roomMeta.scope
  }
}

async function unreachableHostError(ip: string): Promise<Error> {
  if (!ip.includes(':')) {
    return new Error('Não consegui falar com o host. Ele pode ter saído ou estar com as portas fechadas.')
  }
  const { ipv6 } = await network.resolve()
  return new Error(
    ipv6
      ? 'Não consegui falar com o host pelo IPv6. Ele pode ter saído, ou o roteador dele está bloqueando conexões IPv6 de entrada (procure "firewall IPv6" nas configurações do roteador).'
      : 'Esta sala usa IPv6 (a internet do host tem CGNAT), mas a sua internet não tem IPv6. Peça para outra pessoa criar a sala.'
  )
}

async function joinTarget(ip: string, secret: number, nickname: string): Promise<JoinInfo> {
  // na mesma rede local o IP público pode não funcionar (NAT loopback)
  const target = (await findOnLan(secret)) ?? ip
  let res: Response
  try {
    res = await fetch(`http://${urlHost(target)}:${PUBLIC_PORT}/api/join`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ secret, identity: clientId(), name: nickname }),
      signal: AbortSignal.timeout(6000)
    })
  } catch {
    throw await unreachableHostError(target)
  }
  const body = (await res.json().catch(() => ({}))) as { error?: string; token?: string; code?: string }
  if (!res.ok || !body.token) {
    throw new Error(JOIN_ERRORS[body.error ?? ''] ?? `O host recusou a entrada (${res.status}).`)
  }
  inRoom = true
  return {
    url: `ws://${urlHost(target)}:${PUBLIC_PORT}`,
    token: body.token,
    identity: clientId(),
    endpoint: ip,
    code: body.code ?? encodeRoomCode({ ip, secret }),
    secret,
    isHost: false
  }
}

async function prepareCandidate(): Promise<CandidateInfo> {
  const listening = (await publicServer.listen()) && (await publicServer.startUdpProbe())
  if (!listening) return { listening: false, publicIp: null, portsOpen: false }
  const netInfo = await network.resolve()
  if (netInfo.publicIp && !netInfo.cgnat) {
    return { listening, publicIp: netInfo.publicIp, portsOpen: await network.openPorts() }
  }
  // atrás de CGNAT, só dá para ser host pelo IPv6
  if (netInfo.ipv6) return { listening, publicIp: netInfo.ipv6, portsOpen: false }
  return { listening: false, publicIp: null, portsOpen: false }
}

async function leaveRoom(): Promise<void> {
  inRoom = false
  await stopHosting()
  publicServer.close()
  audio.stop()
  await network.closePorts()
}

function registerIpc(): void {
  ipcMain.handle('settings:get', () => settings.get())
  ipcMain.handle('settings:set', (_e, patch: Partial<Settings>) => settings.set(patch))

  ipcMain.handle('screen:sources', async () => {
    const sources = await desktopCapturer.getSources({
      types: ['screen', 'window'],
      thumbnailSize: { width: 320, height: 180 }
    })
    return sources
      .filter((s) => s.name.trim())
      .map((s) => ({
        id: s.id,
        name: s.name,
        kind: (s.id.startsWith('window:') ? 'window' : 'screen') as 'window' | 'screen',
        thumbnail: s.thumbnail.toDataURL()
      }))
  })
  ipcMain.handle('screen:select', (_e, sourceId: string, systemAudio: boolean) => {
    pendingShare = { sourceId, systemAudio }
  })

  ipcMain.handle('audio:list-apps', (_e, blocked: string[]) => audio.listApps(blocked))
  ipcMain.handle('audio:resolve-window', (_e, sourceId: string) => audio.resolveWindow(windowHandleOf(sourceId)))
  ipcMain.handle('audio:start', (e, blocked: string[], only: string[]) =>
    audio.start(blocked, only ?? [], process.pid, e.sender)
  )
  ipcMain.handle('audio:set-blocked', (_e, blocked: string[]) => audio.setBlocked(blocked))
  ipcMain.handle('audio:stop', () => audio.stop())

  ipcMain.handle('room:create', (_e, nickname: string) => becomeHost(generateRoomSecret(), 1, nickname))
  ipcMain.handle('room:join', (_e, code: string, nickname: string) => {
    const { ip, secret } = decodeRoomCode(code)
    return joinTarget(ip, secret, nickname)
  })
  ipcMain.handle('room:join-endpoint', (_e, endpoint: string, secret: number, nickname: string) =>
    joinTarget(endpoint, secret, nickname)
  )
  ipcMain.handle('room:become-host', (_e, secret: number, epoch: number, nickname: string) =>
    becomeHost(secret, epoch, nickname)
  )
  ipcMain.handle('room:prepare-candidate', () => prepareCandidate())
  ipcMain.handle('room:leave', () => leaveRoom())

  ipcMain.handle('host:participant-ready', (_e, identity: string, publicIp: string | null, listening: boolean) =>
    host?.participantReady(identity, publicIp, listening)
  )
  ipcMain.handle('host:participant-left', (_e, identity: string) => host?.participantLeft(identity))
  ipcMain.handle('host:set-max-quality', (_e, quality: QualityId | null) => host?.setMaxQuality(quality))

  ipcMain.handle('app:is-elevated', () => isElevated())
  ipcMain.handle('app:quit-now', () => {
    quitting = true
    app.quit()
  })
  ipcMain.handle('app:clipboard', (_e, text: string) => clipboard.writeText(text))
  ipcMain.handle('app:read-clipboard', () => clipboard.readText())

  ipcMain.handle('net:status', (_e, refresh: boolean) => {
    if (refresh) network.invalidate()
    return network.resolve()
  })
}

app.whenReady().then(() => {
  settings = new SettingsStore(path.join(app.getPath('userData'), 'settings.json'))
  publicServer = new PublicServer(clientId)
  audio = new AudioCaptureService(path.join(binDir, 'audio-capture.exe'))
  audio.on('status', (status) => win?.webContents.send('audio:status', status))

  session.defaultSession.setDisplayMediaRequestHandler(async (_request, callback) => {
    // telas e janelas: o sourceId escolhido na interface pode ser dos dois tipos
    const sources = await desktopCapturer.getSources({ types: ['screen', 'window'] })
    const source = sources.find((s) => s.id === pendingShare?.sourceId) ?? sources[0]
    if (!source) return callback({})
    callback(pendingShare?.systemAudio ? { video: source, audio: 'loopback' } : { video: source })
  })

  registerIpc()
  registerPopoutIpc({
    devUrl: process.env.ELECTRON_RENDERER_URL ?? null,
    indexHtml: path.join(__dirname, '../renderer/index.html'),
    preload: path.join(__dirname, '../preload/index.js')
  })
  createWindow()
})

app.on('window-all-closed', () => app.quit())

app.on('will-quit', (e) => {
  if (cleanedUp) return
  e.preventDefault()
  audio?.dispose()
  void Promise.race([leaveRoom(), new Promise((r) => setTimeout(r, 2500))]).finally(() => {
    cleanedUp = true
    app.quit()
  })
})
