import { contextBridge, ipcRenderer, type IpcRendererEvent } from 'electron'
import type { Api, AudioStatus } from '../shared/api'

function on<T>(channel: string, cb: (value: T) => void): () => void {
  const listener = (_e: IpcRendererEvent, value: T): void => cb(value)
  ipcRenderer.on(channel, listener)
  return () => ipcRenderer.removeListener(channel, listener)
}

// A MessagePort do áudio não atravessa o contextBridge; vai por window.postMessage.
ipcRenderer.on('audio:port', (e) => window.postMessage({ type: 'audio:port' }, '*', e.ports))

const api: Api = {
  settings: {
    get: () => ipcRenderer.invoke('settings:get'),
    set: (patch) => ipcRenderer.invoke('settings:set', patch)
  },
  screen: {
    sources: () => ipcRenderer.invoke('screen:sources'),
    select: (sourceId, systemAudio) => ipcRenderer.invoke('screen:select', sourceId, systemAudio)
  },
  audio: {
    listApps: (blocked) => ipcRenderer.invoke('audio:list-apps', blocked),
    start: (blocked, only) => ipcRenderer.invoke('audio:start', blocked, only ?? []),
    resolveWindow: (sourceId) => ipcRenderer.invoke('audio:resolve-window', sourceId),
    setBlocked: (blocked) => ipcRenderer.invoke('audio:set-blocked', blocked),
    stop: () => ipcRenderer.invoke('audio:stop'),
    onStatus: (cb) => on<AudioStatus>('audio:status', cb)
  },
  room: {
    create: (nickname) => ipcRenderer.invoke('room:create', nickname),
    join: (code, nickname) => ipcRenderer.invoke('room:join', code, nickname),
    joinEndpoint: (endpoint, secret, nickname) => ipcRenderer.invoke('room:join-endpoint', endpoint, secret, nickname),
    becomeHost: (secret, epoch, nickname) => ipcRenderer.invoke('room:become-host', secret, epoch, nickname),
    prepareCandidate: () => ipcRenderer.invoke('room:prepare-candidate'),
    leave: () => ipcRenderer.invoke('room:leave')
  },
  host: {
    participantReady: (identity, publicIp, listening) =>
      ipcRenderer.invoke('host:participant-ready', identity, publicIp, listening),
    participantLeft: (identity) => ipcRenderer.invoke('host:participant-left', identity),
    setMaxQuality: (quality) => ipcRenderer.invoke('host:set-max-quality', quality)
  },
  app: {
    isElevated: () => ipcRenderer.invoke('app:is-elevated'),
    onCloseRequested: (cb) => on<void>('app:close-requested', cb),
    quitNow: () => ipcRenderer.invoke('app:quit-now'),
    clipboard: (text) => ipcRenderer.invoke('app:clipboard', text),
    readClipboard: () => ipcRenderer.invoke('app:read-clipboard')
  },
  net: {
    status: (refresh) => ipcRenderer.invoke('net:status', refresh ?? false)
  },
  popout: {
    open: (request) => ipcRenderer.invoke('popout:open', request),
    ready: () => ipcRenderer.send('popout:ready'),
    answer: (key, sdp) => ipcRenderer.send('popout:answer', { key, sdp }),
    close: (key) => ipcRenderer.send('popout:close', key),
    onOffer: (cb) => on('popout:offer', cb),
    onAnswer: (cb) => on('popout:answer', cb),
    onClosed: (cb) => on<{ key: string }>('popout:closed', (value) => cb(value.key))
  }
}

contextBridge.exposeInMainWorld('api', api)
