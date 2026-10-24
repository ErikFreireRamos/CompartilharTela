// "Destacar" uma transmissão numa janela própria (para pôr no segundo monitor).
//
// MediaStream não atravessa processos do Electron, então o vídeo chega na janela
// destacada por um RTCPeerConnection local: a janela do app manda a oferta e a
// destacada responde. Como as duas estão na mesma máquina, não há STUN/TURN.
import { BrowserWindow, ipcMain, screen, type WebContents } from 'electron'
import type { PopoutAnswer, PopoutRequest } from '../shared/api'

interface Entry {
  key: string
  title: string
  sdp: string
  win: BrowserWindow
  opener: WebContents
}

export interface PopoutPaths {
  /** URL do Vite em desenvolvimento; em produção usa-se o index.html do build. */
  devUrl: string | null
  indexHtml: string
  preload: string
}

/** key (identity de quem transmite) -> janela aberta */
const entries = new Map<string, Entry>()

function clampAspect(aspect: number): number {
  if (!Number.isFinite(aspect) || aspect <= 0) return 16 / 9
  return Math.min(Math.max(aspect, 0.4), 4)
}

function contentSize(aspect: number): { width: number; height: number } {
  let width = 960
  let height = Math.round(width / clampAspect(aspect))
  // retrato ou tela muito alta: limita pela altura
  if (height > 720) {
    height = 720
    width = Math.round(height * clampAspect(aspect))
  }
  return { width: Math.max(width, 320), height: Math.max(height, 180) }
}

/** Manda a janela para outra tela, se houver (preferindo a da direita). */
function moveToOtherDisplay(win: BrowserWindow, openerWin: BrowserWindow | null): void {
  const displays = screen.getAllDisplays()
  if (displays.length < 2) return
  const mainBounds = openerWin?.getBounds() ?? screen.getPrimaryDisplay().bounds
  const current = screen.getDisplayNearestPoint({
    x: mainBounds.x + Math.round(mainBounds.width / 2),
    y: mainBounds.y + Math.round(mainBounds.height / 2)
  })
  const others = displays.filter((d) => d.id !== current.id).sort((a, b) => a.bounds.x - b.bounds.x)
  if (!others.length) return
  const target = others.find((d) => d.bounds.x > current.bounds.x) ?? others[0]
  const area = target.workArea
  const { width, height } = win.getBounds()
  win.setPosition(
    Math.round(area.x + (area.width - width) / 2),
    Math.round(area.y + (area.height - height) / 2),
    false
  )
}

function createEntry(request: PopoutRequest, opener: WebContents, paths: PopoutPaths): Entry {
  const { width, height } = contentSize(request.aspect)
  const openerWin = BrowserWindow.fromWebContents(opener)
  const win = new BrowserWindow({
    width,
    height,
    useContentSize: true,
    minWidth: 320,
    minHeight: 180,
    title: request.title,
    backgroundColor: '#000000',
    autoHideMenuBar: true,
    show: false,
    webPreferences: {
      preload: paths.preload,
      sandbox: true,
      contextIsolation: true,
      // a janela fica num monitor que pode estar coberto: não dá para pausar o vídeo
      backgroundThrottling: false
    }
  })
  win.setMenuBarVisibility(false)

  if (paths.devUrl) {
    const url = new URL(paths.devUrl)
    url.searchParams.set('popout', request.key)
    void win.loadURL(url.toString())
  } else {
    void win.loadFile(paths.indexHtml, { query: { popout: request.key } })
  }

  win.once('ready-to-show', () => {
    moveToOtherDisplay(win, openerWin)
    win.show()
  })

  const entry: Entry = { key: request.key, title: request.title, sdp: request.sdp, win, opener }

  win.on('closed', () => {
    entries.delete(request.key)
    if (!opener.isDestroyed()) opener.send('popout:closed', { key: request.key })
  })
  // se a janela do app sumir, a destacada não faz sentido sozinha
  opener.once('destroyed', () => {
    if (!win.isDestroyed()) win.close()
  })

  return entry
}

function entryFor(sender: WebContents): Entry | undefined {
  for (const entry of entries.values()) if (entry.win.webContents === sender) return entry
  return undefined
}

/** Fecha todas: usado quando a janela principal fecha/sai. */
export function closeAllPopouts(): void {
  for (const entry of [...entries.values()]) {
    if (!entry.win.isDestroyed()) entry.win.close()
  }
}

export function registerPopoutIpc(paths: PopoutPaths): void {
  ipcMain.handle('popout:open', (e, request: PopoutRequest) => {
    const existing = entries.get(request.key)
    if (existing && !existing.win.isDestroyed()) {
      existing.win.focus()
      return { ok: true as const }
    }
    try {
      entries.set(request.key, createEntry(request, e.sender, paths))
      return { ok: true as const }
    } catch (err) {
      return { ok: false as const, error: err instanceof Error ? err.message : String(err) }
    }
  })

  // a janela destacada terminou de montar: manda a oferta
  ipcMain.on('popout:ready', (e) => {
    const entry = entryFor(e.sender)
    if (!entry) return
    e.sender.send('popout:offer', { key: entry.key, title: entry.title, sdp: entry.sdp })
  })

  ipcMain.on('popout:answer', (e, answer: PopoutAnswer) => {
    const entry = entries.get(answer.key)
    if (!entry || entry.win.webContents !== e.sender) return
    if (!entry.opener.isDestroyed()) entry.opener.send('popout:answer', answer)
  })

  ipcMain.on('popout:close', (e, key: string) => {
    const entry = entries.get(key)
    if (!entry || entry.win.isDestroyed()) return
    if (entry.opener !== e.sender && entry.win.webContents !== e.sender) return
    entry.win.close()
  })
}
