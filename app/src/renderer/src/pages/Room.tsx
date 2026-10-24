import { useEffect, useRef, useState } from 'react'
import {
  Ban,
  Check,
  ChevronDown,
  ChevronUp,
  CircleStop,
  Copy,
  Crown,
  LayoutGrid,
  LogOut,
  MoreHorizontal,
  ScreenShare,
  SlidersHorizontal,
  Square,
  X
} from 'lucide-react'
import type { QualityId } from '../../../shared/protocol'
import AdminWarning from '../components/AdminWarning'
import AudioAppsList from '../components/AudioAppsList'
import Logo from '../components/Logo'
import ParticipantList from '../components/ParticipantList'
import ShareDialog from '../components/ShareDialog'
import StreamTile from '../components/StreamTile'
import { api } from '../lib/api'
import { closeAllPopouts, closePopout, poppedKeys } from '../lib/popout'
import { QUALITY_PRESETS } from '../lib/qualityPresets'
import { session } from '../lib/roomSession'
import { toast } from '../lib/toast'

export default function RoomPage(): React.JSX.Element {
  const [showShare, setShowShare] = useState(false)
  const [showAudio, setShowAudio] = useState(false)
  const [focusedId, setFocusedId] = useState<string | null>(null)
  const [grid, setGrid] = useState(false)
  const [copied, setCopied] = useState(false)
  const [menuOpen, setMenuOpen] = useState(false)
  /** Painéis que podem ser escondidos para a transmissão ganhar espaço. */
  const [peopleHidden, setPeopleHidden] = useState(false)
  const [thumbsHidden, setThumbsHidden] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)

  const streams = session.streams()
  // transmissões que ainda fazem sentido numa janela destacada
  const activeKey = streams
    .filter((s) => s.video?.track && s.receive !== 'off')
    .map((s) => s.identity)
    .join('|')

  // transmissão que sumiu (parou de compartilhar, saiu da sala): fecha a janela
  useEffect(() => {
    const ids = new Set(activeKey ? activeKey.split('|') : [])
    for (const key of poppedKeys()) if (!ids.has(key)) closePopout(key)
  }, [activeKey])

  // saindo da sala: as janelas destacadas não fazem mais sentido
  useEffect(() => () => closeAllPopouts(), [])

  // menu "⋯": fecha ao clicar fora ou com Esc
  useEffect(() => {
    if (!menuOpen) return
    const onDown = (e: PointerEvent): void => {
      if (!menuRef.current?.contains(e.target as Node)) setMenuOpen(false)
    }
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') setMenuOpen(false)
    }
    document.addEventListener('pointerdown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('pointerdown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [menuOpen])

  const peers = session.peerList()
  const meta = session.meta
  const share = session.share
  const settings = session.settings!
  const focused = streams.find((s) => s.identity === focusedId) ?? streams.find((s) => !s.isLocal) ?? streams[0]
  const others = streams.filter((s) => s !== focused)

  const connection = session.reconnecting
    ? { kind: 'warn', label: 'Reconectando' }
    : session.phase === 'connected'
      ? { kind: 'ok', label: 'Conectado' }
      : session.phase === 'migrating'
        ? { kind: 'warn', label: 'Trocando de host' }
        : session.phase === 'connecting'
          ? { kind: 'warn', label: 'Conectando' }
          : { kind: 'bad', label: 'Desconectado' }
  const linkName = session.info?.endpoint.includes(':') ? 'IPv6' : 'IPv4'
  const connectionTitle = session.reconnecting
    ? 'A conexão caiu e o app está tentando voltar. Se não voltar, a sala troca de host.'
    : session.phase === 'connected'
      ? `Conectado por ${linkName}`
      : session.phase === 'migrating'
        ? 'O host saiu: a sala está escolhendo outra pessoa para hospedar.'
        : session.phase === 'connecting'
          ? 'Falando com o host…'
          : 'Sem conexão com a sala.'

  const copyCode = (): void => {
    void api.app.clipboard(session.code)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
    toast('Código copiado', 'success')
  }

  const setBlocked = (blocked: string[]): void => {
    void session.updateSettings({ blockedApps: blocked })
    void api.audio.setBlocked(blocked)
  }

  const setSendVolume = (v: number): void => {
    session.setSendVolume(v)
    void session.updateSettings({ sendVolume: v })
  }

  const onPopoutError = (message: string): void => toast(message, 'error')

  const stopShare = (): void => {
    session.stopShare()
    setShowAudio(false)
    toast('Transmissão encerrada')
  }

  // a transmissão acabou por fora (janela fechada, track encerrada): some o popover
  useEffect(() => {
    if (share?.audioMode !== 'native') setShowAudio(false)
  }, [share?.audioMode])

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
      <header className="appbar">
        <div className="brand" title="CompartilhadorDeTela">
          <Logo size={26} />
        </div>

        <button
          className={`code-pill${copied ? ' copied' : ''}${session.code.length > 20 ? ' long' : ''}`}
          onClick={copyCode}
          title="Copiar o código da sala"
        >
          <span className="code-label">Sala</span>
          <span className="code-value">{session.code}</span>
          {copied ? <Check size={14} /> : <Copy size={14} />}
        </button>

        <span className="host-chip" title="Quem está hospedando a sala">
          <Crown size={13} />
          <span>{meta?.hostName ?? '…'}</span>
        </span>

        <div className="spacer" />

        <span className={`conn ${connection.kind}`} title={connectionTitle}>
          <span className="conn-dot" />
          {connection.label}
        </span>

        {streams.length > 1 && (
          <div className="segmented" role="group" aria-label="Modo de visualização">
            <button className={!grid ? 'on' : ''} aria-pressed={!grid} onClick={() => setGrid(false)}>
              <Square size={12} /> Destaque
            </button>
            <button className={grid ? 'on' : ''} aria-pressed={grid} onClick={() => setGrid(true)}>
              <LayoutGrid size={12} /> Grade
            </button>
          </div>
        )}

        <div className="menu-wrap" ref={menuRef}>
          <button
            className={`icon menu-btn${menuOpen ? ' on' : ''}`}
            title="Mais opções"
            aria-haspopup="true"
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((open) => !open)}
          >
            <MoreHorizontal size={18} />
          </button>

          {menuOpen && (
            <div className="menu" aria-label="Mais opções">
              {session.isHost && (
                <label className="menu-field">
                  <span>
                    <SlidersHorizontal size={13} /> Teto de qualidade
                  </span>
                  <select
                    value={meta?.maxQuality ?? ''}
                    title="Limite de qualidade para todos (protege seu upload)"
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

              {share?.audioMode === 'native' && (
                <button
                  className="menu-item"
                  onClick={() => {
                    setMenuOpen(false)
                    setShowAudio(true)
                  }}
                >
                  <Ban size={15} /> Apps bloqueados
                </button>
              )}

              <div className="menu-sep" />

              <button
                className="menu-item danger"
                onClick={() => {
                  setMenuOpen(false)
                  void session.leave()
                }}
              >
                <LogOut size={15} /> Sair da sala
              </button>
            </div>
          )}
        </div>
      </header>

      {session.notice && <div className="banner info">{session.notice}</div>}
      <AdminWarning />
      {portsWarning && <div className="banner warn">{portsWarning}</div>}
      {share?.warning && <div className="banner warn">{share.warning}</div>}

      <div className="room-body">
        <section className="stage">
          {streams.length === 0 ? (
            <div className="empty">
              <div className="empty-icon">
                <ScreenShare size={26} />
              </div>
              <p>Ninguém está transmitindo ainda.</p>
              <button className="primary" onClick={() => setShowShare(true)}>
                <ScreenShare size={15} />
                Compartilhar minha tela
              </button>
            </div>
          ) : grid ? (
            <div className={`grid grid-${Math.min(streams.length, 4)}`}>
              {streams.map((s) => (
                <StreamTile key={s.identity} stream={s} large onError={onPopoutError} />
              ))}
            </div>
          ) : (
            <>
              {focused && <StreamTile key={focused.identity} stream={focused} large onError={onPopoutError} />}
              {others.length > 0 && (
                <div className={`thumbs${thumbsHidden ? ' collapsed' : ''}`}>
                  <div className="thumbs-strip">
                    {others.map((s) => (
                      <StreamTile
                        key={s.identity}
                        stream={s}
                        large={false}
                        onFocus={() => setFocusedId(s.identity)}
                        onError={onPopoutError}
                      />
                    ))}
                    <button
                      className="thumbs-hide"
                      title="Esconder as miniaturas"
                      tabIndex={thumbsHidden ? -1 : 0}
                      onClick={() => setThumbsHidden(true)}
                    >
                      <ChevronDown size={14} />
                    </button>
                  </div>
                  <button
                    className="thumbs-bar"
                    title="Mostrar as miniaturas"
                    tabIndex={thumbsHidden ? 0 : -1}
                    onClick={() => setThumbsHidden(false)}
                  >
                    <ChevronUp size={13} />
                    Mostrar miniaturas ({others.length})
                  </button>
                </div>
              )}
            </>
          )}
        </section>

        <ParticipantList
          peers={peers}
          hidden={peopleHidden}
          onHide={() => setPeopleHidden(true)}
          onShow={() => setPeopleHidden(false)}
        />
      </div>

      <footer className="room-footer">
        {share ? (
          <>
            <span className="live-badge">
              <span className="live-dot" />
              <strong>Transmitindo</strong>
              <span className="muted">
                {share.preset.label}
                {share.audioMode === 'native' && ' · áudio por app'}
                {share.audioMode === 'system' && ' · áudio do PC inteiro'}
              </span>
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
            <div className="spacer" />
            <button className="danger" onClick={stopShare}>
              <CircleStop size={14} />
              Parar transmissão
            </button>
          </>
        ) : (
          <>
            <span className="muted footer-hint">
              {streams.length > 0 ? 'Você está assistindo' : 'Sala aberta — compartilhe a sua tela quando quiser'}
            </span>
            <div className="spacer" />
            <button className="primary" onClick={() => setShowShare(true)}>
              <ScreenShare size={15} />
              Compartilhar tela
            </button>
          </>
        )}
      </footer>

      {showAudio && share?.audioMode === 'native' && (
        <div className="popover">
          <div className="popover-head">
            <h3>Apps bloqueados</h3>
            <button className="icon" title="Fechar" onClick={() => setShowAudio(false)}>
              <X size={15} />
            </button>
          </div>
          <AudioAppsList blocked={settings.blockedApps} onChange={setBlocked} />
        </div>
      )}
      {showShare && <ShareDialog onClose={() => setShowShare(false)} />}
    </div>
  )
}
