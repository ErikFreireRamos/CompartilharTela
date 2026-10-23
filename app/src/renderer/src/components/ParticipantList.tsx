import { MAX_PARTICIPANTS } from '../../../shared/protocol'
import type { PeerView } from '../lib/roomSession'

export default function ParticipantList({ peers }: { peers: PeerView[] }): React.JSX.Element {
  return (
    <aside className="participants">
      <h3>
        Na sala <span className="muted">{peers.length}/{MAX_PARTICIPANTS}</span>
      </h3>
      <ul>
        {peers.map((p) => (
          <li key={p.identity}>
            <span className="peer-name">
              {p.name}
              {p.isLocal && <span className="muted"> (você)</span>}
            </span>
            <span className="badges">
              {p.isHost && <span title="Host da sala">👑</span>}
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
    </aside>
  )
}
