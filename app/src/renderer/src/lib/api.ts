import type { Api } from '../../../shared/api'

declare global {
  interface Window {
    api: Api
  }
}

export const api = window.api

/** Tira o prefixo que o Electron põe nos erros vindos do processo main. */
export function errorMessage(err: unknown): string {
  const msg = err instanceof Error ? err.message : String(err)
  return msg.replace(/^Error invoking remote method '[^']+': (?:\w*Error: )?/, '')
}

/** Próxima MessagePort de áudio entregue pelo preload. */
export function nextAudioPort(timeoutMs = 3000): Promise<MessagePort | null> {
  return new Promise((resolve) => {
    const finish = (port: MessagePort | null): void => {
      clearTimeout(timer)
      window.removeEventListener('message', listener)
      resolve(port)
    }
    const listener = (e: MessageEvent): void => {
      if (e.data?.type === 'audio:port' && e.ports[0]) finish(e.ports[0])
    }
    const timer = setTimeout(() => finish(null), timeoutMs)
    window.addEventListener('message', listener)
  })
}

export const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms))
