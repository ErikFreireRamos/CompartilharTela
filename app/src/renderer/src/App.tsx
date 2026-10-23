import { useEffect, useSyncExternalStore } from 'react'
import { api } from './lib/api'
import { session } from './lib/roomSession'
import Home from './pages/Home'
import RoomPage from './pages/Room'

export default function App(): React.JSX.Element {
  useSyncExternalStore(session.subscribe, session.getVersion)

  useEffect(() => {
    void session.loadSettings()
    // fechar a janela dentro de uma sala: passa o host adiante antes
    return api.app.onCloseRequested(async () => {
      await session.leave()
      await api.app.quitNow()
    })
  }, [])

  if (!session.settings) return <div className="splash">Carregando…</div>
  const inRoom = session.phase === 'connected' || session.phase === 'migrating'
  return (
    <>
      {inRoom ? <RoomPage /> : <Home />}
      <div className="credit">by Erik Freire Ramos</div>
    </>
  )
}
