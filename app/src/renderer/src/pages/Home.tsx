import { useEffect, useState } from 'react'
import { ArrowRight, Loader2, LogIn, RefreshCw, ScreenShare, ShieldCheck, X } from 'lucide-react'
import type { NetworkStatus } from '../../../shared/api'
import { extractRoomCode } from '../../../shared/roomCode'
import AdminWarning from '../components/AdminWarning'
import Logo from '../components/Logo'
import { api } from '../lib/api'
import { session } from '../lib/roomSession'
import { toast } from '../lib/toast'

/** Formata enquanto digita: XXXX-XXXX-XXXX (IPv4) ou 8 grupos (IPv6) */
function formatCode(raw: string): string {
  const clean = raw.toUpperCase().replace(/[^0-9A-Z]/g, '').slice(0, 32)
  return clean.match(/.{1,4}/g)?.join('-') ?? ''
}

function networkSummary(net: NetworkStatus | null): { kind: 'ok' | 'warn' | 'bad'; label: string; title: string } {
  if (!net) {
    return {
      kind: 'bad',
      label: 'Não consegui verificar a rede',
      title: 'A checagem usa um servidor STUN. Sem ela, ainda dá para criar uma sala na rede local.'
    }
  }
  if (net.ipv6) {
    return {
      kind: 'ok',
      label: net.cgnat ? 'IPv6 disponível · IPv4 com CGNAT' : 'IPv6 disponível',
      title: net.cgnat
        ? 'Sua internet tem CGNAT no IPv4, mas o IPv6 funciona: dá para criar sala na internet. Quem for entrar também precisa de IPv6.'
        : 'Dá para criar sala na internet, inclusive pelo IPv6.'
    }
  }
  if (net.publicIp && !net.cgnat) {
    return {
      kind: 'ok',
      label: 'IPv4 público disponível',
      title: 'Dá para criar sala na internet. O app tenta abrir as portas no roteador (UPnP).'
    }
  }
  if (net.cgnat) {
    return {
      kind: 'bad',
      label: 'CGNAT sem IPv6: sala só na rede local',
      title:
        'A operadora divide o mesmo IP entre vários clientes e não há IPv6. Ninguém de fora alcança este PC; peça para um amigo sem CGNAT criar a sala.'
    }
  }
  return {
    kind: 'warn',
    label: 'Sem IP público: sala só na rede local',
    title: 'Não encontrei um endereço público. A sala funciona para quem está na mesma rede.'
  }
}

export default function Home(): React.JSX.Element {
  const [nickname, setNickname] = useState(session.settings?.nickname ?? '')
  const [code, setCode] = useState('')
  const [net, setNet] = useState<NetworkStatus | null>(null)
  const [checkingNet, setCheckingNet] = useState(true)
  const connecting = session.phase === 'connecting'
  const name = nickname.trim()
  const codeLength = code.replace(/-/g, '').length
  const codeComplete = codeLength === 12 || codeLength === 32
  const summary = networkSummary(net)

  const checkNet = (refresh = false): void => {
    setCheckingNet(true)
    void api.net
      .status(refresh)
      .then(setNet)
      .catch(() => setNet(null))
      .finally(() => setCheckingNet(false))
  }

  useEffect(() => {
    checkNet()
  }, [])

  const saveNickname = (): void => {
    if (name !== session.settings?.nickname) void session.updateSettings({ nickname: name })
  }

  const create = (): void => {
    saveNickname()
    void session.create(name)
  }

  const join = (e: React.FormEvent): void => {
    e.preventDefault()
    saveNickname()
    void session.join(code, name)
  }

  /** Ao focar o campo vazio, aproveita um código que esteja no clipboard. */
  const pasteFromClipboard = (): void => {
    if (code) return
    void api.app
      .readClipboard()
      .then((text) => {
        const found = extractRoomCode(text)
        if (!found) return
        setCode(formatCode(found))
        toast('Código colado da área de transferência')
      })
      .catch(() => undefined)
  }

  return (
    <main className="home">
      <header className="home-hero">
        <Logo size={54} />
        <div>
          <h1>CompartilhadorDeTela</h1>
          <p>Sua tela e o áudio do PC, direto para até 10 amigos.</p>
          <span className="home-scope">
            <span className="home-scope-dot" /> Sem voz e sem chat: só a tela
          </span>
        </div>
      </header>

      <label className="field nickname">
        <span>Seu apelido</span>
        <input
          value={nickname}
          maxLength={32}
          placeholder="Como os amigos vão te ver"
          onChange={(e) => setNickname(e.target.value)}
          onBlur={saveNickname}
          autoFocus
        />
      </label>

      <AdminWarning />
      {session.error && <div className="banner error">{session.error}</div>}

      <div className="home-cards">
        <section className="card card-primary">
          <div className="card-icon">
            <ScreenShare size={21} />
          </div>
          <h2>Criar sala</h2>
          <p>Seu PC vira o host. Você recebe um código para mandar aos amigos.</p>
          <button className="primary big" disabled={!name || connecting} onClick={create}>
            <ScreenShare size={16} />
            {connecting ? 'Conectando…' : 'Criar sala'}
          </button>
        </section>

        <form className="card" onSubmit={join}>
          <div className="card-icon">
            <LogIn size={20} />
          </div>
          <h2>Entrar em sala</h2>
          <p>Cole o código que o host mandou — ele cola sozinho quando você clica no campo.</p>
          <div className="code-wrap">
            <input
              className={`code-input ${codeLength > 12 ? 'long' : ''}`}
              value={code}
              placeholder="XXXX-XXXX-XXXX"
              onChange={(e) => setCode(formatCode(e.target.value))}
              onFocus={pasteFromClipboard}
              spellCheck={false}
            />
            {code && (
              <button type="button" className="icon code-clear" title="Limpar" onClick={() => setCode('')}>
                <X size={14} />
              </button>
            )}
          </div>
          <button className="primary big" type="submit" disabled={!name || !codeComplete || connecting}>
            <ArrowRight size={16} />
            {connecting ? 'Conectando…' : 'Entrar'}
          </button>
        </form>
      </div>

      <footer className="home-foot">
        <span className={`net-status ${summary.kind}`} title={summary.title}>
          {checkingNet ? <Loader2 size={13} className="spin" /> : <span className="net-dot" />}
          <span>{checkingNet ? 'Verificando a rede…' : summary.label}</span>
          <button
            className="icon"
            title="Verificar a rede de novo"
            onClick={() => checkNet(true)}
            disabled={checkingNet}
          >
            <RefreshCw size={12} />
          </button>
        </span>
        <span className="home-privacy">
          <ShieldCheck size={13} /> Conexão direta entre vocês, sem servidores no meio
        </span>
      </footer>
    </main>
  )
}
