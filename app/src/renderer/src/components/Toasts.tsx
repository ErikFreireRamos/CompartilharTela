import { useSyncExternalStore } from 'react'
import { CheckCircle2, CircleAlert, Info, X } from 'lucide-react'
import { dismissToast, toasts } from '../lib/toast'

export default function Toasts(): React.JSX.Element {
  const items = useSyncExternalStore(toasts.subscribe, toasts.getSnapshot)

  return (
    <div className="toasts" aria-live="polite" aria-atomic="false">
      {items.map((t) => (
        <div key={t.id} className={`toast ${t.kind}${t.leaving ? ' leaving' : ''}`}>
          {t.kind === 'success' ? (
            <CheckCircle2 size={16} />
          ) : t.kind === 'error' ? (
            <CircleAlert size={16} />
          ) : (
            <Info size={16} />
          )}
          <span>{t.message}</span>
          <button className="icon" title="Fechar" onClick={() => dismissToast(t.id)}>
            <X size={13} />
          </button>
        </div>
      ))}
    </div>
  )
}
