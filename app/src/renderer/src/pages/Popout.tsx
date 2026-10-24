// Janela destacada: só a transmissão escolhida, para pôr em outro monitor.
// Recebe a oferta pela IPC, responde e toca o vídeo — o áudio continua no app.
import { useEffect, useRef, useState } from 'react'
import { Maximize2 } from 'lucide-react'
import { api } from '../lib/api'
import { iceGathered, localPeer } from '../lib/localPeer'

type State = 'connecting' | 'live' | 'ended' | 'error'

export default function Popout(): React.JSX.Element {
  const videoRef = useRef<HTMLVideoElement>(null)
  const [title, setTitle] = useState('Transmissão')
  const [state, setState] = useState<State>('connecting')
  const [error, setError] = useState('')

  useEffect(() => {
    const key = new URLSearchParams(location.search).get('popout') ?? ''
    const video = videoRef.current
    const pc = localPeer()

    pc.ontrack = (event) => {
      const stream = event.streams[0] ?? new MediaStream([event.track])
      if (video) {
        video.srcObject = stream
        void video.play().catch(() => undefined)
      }
      setState('live')
    }
    pc.onconnectionstatechange = () => {
      if (pc.connectionState === 'failed' || pc.connectionState === 'disconnected') setState('ended')
    }

    const unsubscribe = api.popout.onOffer((offer) => {
      setTitle(offer.title)
      document.title = offer.title
      void (async () => {
        try {
          await pc.setRemoteDescription({ type: 'offer', sdp: offer.sdp })
          const answer = await pc.createAnswer()
          await pc.setLocalDescription(answer)
          await iceGathered(pc)
          api.popout.answer(key, pc.localDescription?.sdp ?? answer.sdp ?? '')
        } catch (err) {
          setState('error')
          setError(err instanceof Error ? err.message : String(err))
        }
      })()
    })
    // o main só manda a oferta depois deste aviso (a escuta já está pronta)
    api.popout.ready()

    return () => {
      unsubscribe()
      pc.close()
    }
  }, [])

  const fullscreen = (): void => {
    if (document.fullscreenElement) void document.exitFullscreen()
    else void document.documentElement.requestFullscreen()
  }

  return (
    <div className="popout" onDoubleClick={fullscreen}>
      <video ref={videoRef} autoPlay playsInline muted />
      {state !== 'live' && (
        <div className="popout-state">
          {state === 'connecting'
            ? 'Conectando à transmissão…'
            : state === 'ended'
              ? 'A transmissão terminou.'
              : `Não deu para abrir a transmissão: ${error}`}
        </div>
      )}
      <div className="popout-bar" onDoubleClick={(e) => e.stopPropagation()}>
        <span className="live-dot" />
        <span className="popout-title" title={title}>
          {title}
        </span>
        <div className="spacer" />
        <button className="icon" title="Tela cheia (duplo clique no vídeo)" onClick={fullscreen}>
          <Maximize2 size={15} />
        </button>
      </div>
    </div>
  )
}
