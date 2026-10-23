import { useState } from 'react'
import AdminWarning from '../components/AdminWarning'
import { session } from '../lib/roomSession'

/** Formata enquanto digita: XXXX-XXXX-XXXX (IPv4) ou 8 grupos (IPv6) */
function formatCode(raw: string): string {
  const clean = raw.toUpperCase().replace(/[^0-9A-Z]/g, '').slice(0, 32)
  return clean.match(/.{1,4}/g)?.join('-') ?? ''
}

export default function Home(): React.JSX.Element {
  const [nickname, setNickname] = useState(session.settings?.nickname ?? '')
  const [code, setCode] = useState('')
  const connecting = session.phase === 'connecting'
  const name = nickname.trim()
  const codeLength = code.replace(/-/g, '').length
  const codeComplete = codeLength === 12 || codeLength === 32

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

  return (
    <main className="home">
      <header className="home-header">
        <h1>CompartilhadorDeTela</h1>
        <p>Transmita sua tela com áudio para até 10 amigos.</p>
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
        <section className="card">
          <h2>Criar sala</h2>
          <p>Seu PC vira o host. Você recebe um código para mandar aos amigos.</p>
          <button className="primary" disabled={!name || connecting} onClick={create}>
            {connecting ? 'Conectando…' : 'Criar sala'}
          </button>
        </section>

        <form className="card" onSubmit={join}>
          <h2>Entrar em sala</h2>
          <input
            className={`code-input ${codeLength > 12 ? 'long' : ''}`}
            value={code}
            placeholder="XXXX-XXXX-XXXX"
            onChange={(e) => setCode(formatCode(e.target.value))}
            spellCheck={false}
          />
          <button className="primary" type="submit" disabled={!name || !codeComplete || connecting}>
            {connecting ? 'Conectando…' : 'Entrar'}
          </button>
        </form>
      </div>
    </main>
  )
}
