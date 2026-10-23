import { useState } from 'react'
import type { QualityId } from '../../../shared/protocol'
import AdminWarning from '../components/AdminWarning'
import AudioAppsList from '../components/AudioAppsList'
import ParticipantList from '../components/ParticipantList'
import ShareDialog from '../components/ShareDialog'
import StreamTile from '../components/StreamTile'
import { api } from '../lib/api'
import { QUALITY_PRESETS } from '../lib/qualityPresets'
import { session } from '../lib/roomSession'

export default function RoomPage(): React.JSX.Element {
  const [showShare, setShowShare] = useState(false)
  const [showAudio, setShowAudio] = useState(false)
  const [focusedId, setFocusedId] = useState<string | null>(null)
  const [grid, setGrid] = useState(false)
  const [copied, setCopied] = useState(false)

  const streams = session.streams()
  const peers = session.peerList()
  const meta = session.meta
  const share = session.share
  const settings = session.settings!
  const focused = streams.find((s) => s.identity === focusedId) ?? streams.find((s) => !s.isLocal) ?? streams[0]
  const others = streams.filter((s) => s !== focused)

  const copyCode = (): void => {
    void api.app.clipboard(session.code)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }

  const setBlocked = (blocked: string[]): void => {
    void session.updateSettings({ blockedApps: blocked })
    void api.audio.setBlocked(blocked)
  }

  const setSendVolume = (v: number): void => {
    session.setSendVolume(v)
    void session.updateSettings({ sendVolume: v })
  }

  const viaIpv6 = !!session.info?.endpoint.includes(':')
  const portsWarning = !session.isHost
    ? null
    : session.info?.cgnat && viaIpv6
      ? 'Sua internet tem CGNAT, então esta sala foi criada pelo IPv6. Quem for entrar precisa ter IPv6 na internet. Se ninguém conseguir entrar, libere conexões IPv6 de entrada no firewall do roteador (TCP 7880–7881 e UDP 7882).'
      : session.info?.cgnat
        ? 'Sua internet usa CGNAT: a operadora divide o mesmo IP público entre vários clientes, então amigos de fora não conseguem chegar no seu PC, nem com as portas abertas. Peça para um amigo sem CGNAT criar a sala, ou peça um IP público à operadora.'
        : meta?.scope === 'internet' && !viaIpv6 && session.info?.portsOpen === false
          ? 'Não consegui abrir as portas no roteador (UPnP). Se você não abriu manualmente TCP 7880–7881 e UDP 7882, amigos de fora não vão conseguir entrar.'
          : meta?.scope === 'lan'
            ? 'Sem IP público: esta sala só funciona para quem está na mesma rede local.'
            : null

  return (
    <div className="room">
      <header className="room-header">
        <button className="code-chip" onClick={copyCode} title="Copiar código">
          <span className="muted">Código</span> <strong>{session.code}</strong> <span>{copied ? '✓ copiado' : '⧉'}</span>
        </button>
        <span className="muted">Host: {meta?.hostName ?? '…'} 👑</span>
        <div className="spacer" />
        {session.isHost && (
          <label className="inline-field" title="Limite de qualidade para todos (protege seu upload)">
            Teto de qualidade
            <select
              value={meta?.maxQuality ?? ''}
              onChange={(e) => void session.setMaxQuality((e.target.value || null) as QualityId | null)}
            >
              <option value="">Sem limite</option>
              {QUALITY_PRESETS.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </select>
          </label>
        )}
        {streams.length > 1 && <button onClick={() => setGrid(!grid)}>{grid ? 'Destaque' : 'Grade'}</button>}
        <button className="danger" onClick={() => void session.leave()}>
          Sair
        </button>
      </header>

      {session.notice && <div className="banner info">{session.notice}</div>}
      <AdminWarning />
      {portsWarning && <div className="banner warn">{portsWarning}</div>}
      {share?.warning && <div className="banner warn">{share.warning}</div>}

      <div className="room-body">
        <section className="stage">
          {streams.length === 0 ? (
            <div className="empty">
              <p>Ninguém está transmitindo ainda.</p>
              <button className="primary" onClick={() => setShowShare(true)}>
                Compartilhar minha tela
              </button>
            </div>
          ) : grid ? (
            <div className={`grid grid-${Math.min(streams.length, 4)}`}>
              {streams.map((s) => (
                <StreamTile key={s.identity} stream={s} large />
              ))}
            </div>
          ) : (
            <>
              {focused && <StreamTile key={focused.identity} stream={focused} large />}
              {others.length > 0 && (
                <div className="thumbs">
                  {others.map((s) => (
                    <StreamTile key={s.identity} stream={s} large={false} onFocus={() => setFocusedId(s.identity)} />
                  ))}
                </div>
              )}
            </>
          )}
        </section>
        <ParticipantList peers={peers} />
      </div>

      <footer className="room-footer">
        {share ? (
          <>
            <span className="live-badge">
              <span className="live-dot" /> Transmitindo · {share.preset.label}
              {share.audioMode === 'native' && ' · áudio por app'}
              {share.audioMode === 'system' && ' · áudio do PC inteiro'}
            </span>
            {share.audio && (
              <label className="inline-field">
                Volume enviado {Math.round(settings.sendVolume * 100)}%
                <input
                  type="range"
                  min={0}
                  max={2}
                  step={0.05}
                  value={settings.sendVolume}
                  onChange={(e) => setSendVolume(Number(e.target.value))}
                />
              </label>
            )}
            {share.audioMode === 'native' && <button onClick={() => setShowAudio(!showAudio)}>Apps bloqueados</button>}
            <div className="spacer" />
            <button className="danger" onClick={() => session.stopShare()}>
              Parar transmissão
            </button>
          </>
        ) : (
          <>
            <div className="spacer" />
            <button className="primary" onClick={() => setShowShare(true)}>
              Compartilhar tela
            </button>
          </>
        )}
      </footer>

      {showAudio && share?.audioMode === 'native' && (
        <div className="popover">
          <AudioAppsList blocked={settings.blockedApps} onChange={setBlocked} />
          <button onClick={() => setShowAudio(false)}>Fechar</button>
        </div>
      )}
      {showShare && <ShareDialog onClose={() => setShowShare(false)} />}
    </div>
  )
}
