// Retornos rápidos (código copiado, destaque aberto, erro…). Avisos que
// precisam continuar na tela (CGNAT, UPnP) ficam nos banners da sala.

export type ToastKind = 'info' | 'success' | 'error'

export interface ToastItem {
  id: number
  message: string
  kind: ToastKind
  /** Em saída: some da lista depois da animação. */
  leaving?: boolean
}

let items: ToastItem[] = []
let seq = 0
const listeners = new Set<() => void>()

function emit(): void {
  for (const listener of listeners) listener()
}

export function dismissToast(id: number): void {
  const item = items.find((t) => t.id === id)
  if (!item || item.leaving) return
  items = items.map((t) => (t.id === id ? { ...t, leaving: true } : t))
  emit()
  setTimeout(() => {
    items = items.filter((t) => t.id !== id)
    emit()
  }, 160)
}

/** `error` fica mais tempo na tela: dá para ler a mensagem inteira. */
export function toast(message: string, kind: ToastKind = 'info'): void {
  const item: ToastItem = { id: ++seq, message, kind }
  // no máximo 3 pilhas: o mais antigo sai para o novo aparecer
  const visible = items.filter((t) => !t.leaving)
  if (visible.length >= 3) dismissToast(visible[0].id)
  items = [...items, item]
  emit()
  setTimeout(() => dismissToast(item.id), kind === 'error' ? 5200 : 3000)
}

export const toasts = {
  subscribe(cb: () => void): () => void {
    listeners.add(cb)
    return () => listeners.delete(cb)
  },
  getSnapshot: (): ToastItem[] => items
}
