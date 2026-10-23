import { useEffect, useState } from 'react'
import type { AudioApp } from '../../../shared/api'
import { api } from '../lib/api'

interface Props {
  blocked: string[]
  onChange: (blocked: string[]) => void
}

function displayName(exe: string): string {
  const base = exe.replace(/\.exe$/i, '')
  return base.charAt(0).toUpperCase() + base.slice(1)
}

/** Apps com áudio no PC e quais estão bloqueados na transmissão. */
export default function AudioAppsList({ blocked, onChange }: Props): React.JSX.Element {
  const [apps, setApps] = useState<AudioApp[]>([])

  const refresh = (): void => void api.audio.listApps(blocked).then(setApps)
  useEffect(refresh, [])

  // os bloqueados aparecem mesmo sem estar tocando agora
  const rows = [
    ...apps.map((a) => ({ exe: a.exe, active: a.active })),
    ...blocked.filter((exe) => !apps.some((a) => a.exe === exe)).map((exe) => ({ exe, active: false }))
  ]

  const toggle = (exe: string): void =>
    onChange(blocked.includes(exe) ? blocked.filter((b) => b !== exe) : [...blocked, exe])

  return (
    <div className="apps-list">
      <div className="apps-header">
        <span>Não transmitir o áudio de:</span>
        <button className="link" onClick={refresh}>
          Atualizar
        </button>
      </div>
      <ul>
        {rows.map((row) => (
          <li key={row.exe}>
            <label>
              <input type="checkbox" checked={blocked.includes(row.exe)} onChange={() => toggle(row.exe)} />
              {displayName(row.exe)}
              {row.active && <span className="tag">tocando</span>}
            </label>
          </li>
        ))}
      </ul>
    </div>
  )
}
