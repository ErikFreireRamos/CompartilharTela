import { ChevronLeft, ChevronRight, Crown } from 'lucide-react'
import { MAX_PARTICIPANTS } from '../../../shared/protocol'
import type { PeerView } from '../lib/roomSession'

export default function ParticipantList({
  peers,
  hidden = false,
  onHide,
  onShow
}: {
  peers: PeerView[]
  /** A lista fica recolhida na barra fininha (o painel anima a largura). */
  hidden?: boolean
  /** Some a lista para a tela compartilhada ganhar espaço. */
  onHide?: () => void
  onShow?: () => void
}): React.JSX.Element {
  return (
    <aside className={`participants${hidden ? ' collapsed' : ''}`}>
      <div className="panel-full">
        <div className="panel-head">
          <h3>
            Na sala <span className="muted">{peers.length}/{MAX_PARTICIPANTS}</span>
          </h3>
          <div className="spacer" />
          {onHide && (
            <button className="icon" title="Esconder a lista de pessoas" onClick={onHide}>
              <ChevronRight size={15} />
            </button>
          )}
        </div>
        <ul>
          {peers.map((p) => (
            <li key={p.identity}>
              <span className="peer-name">
                {p.name}
                {p.isLocal && <span className="muted"> (você)</span>}
              </span>
              <span className="badges">
                {p.isHost && (
                  <span className="badge-icon" title="Host da sala">
                    <Crown size={12} />
                  </span>
                )}
                {p.sharing && <span title="Transmitindo" className="live-dot" />}
                {!p.isHost && p.canHost === true && (
                  <span className="tag" title="Pode assumir como host se o atual sair">
                    reserva
                  </span>
                )}
              </span>
            </li>
          ))}
        </ul>
      </div>

      {onShow && (
        <button
          className="rail"
          title="Mostrar a lista de pessoas"
          aria-label="Mostrar a lista de pessoas"
          tabIndex={hidden ? 0 : -1}
          onClick={onShow}
        >
          <ChevronLeft size={14} />
          <span className="rail-label">Pessoas</span>
        </button>
      )}
    </aside>
  )
}
