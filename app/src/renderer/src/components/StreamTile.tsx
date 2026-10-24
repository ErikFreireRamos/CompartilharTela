import { useEffect, useRef, useState } from 'react'
import { Maximize2, PictureInPicture2, Volume2, VolumeX } from 'lucide-react'
import { errorMessage } from '../lib/api'
import { closePopout, isPopped, onPopoutChange, popOut } from '../lib/popout'
import { session, type ReceiveQuality, type StreamView } from '../lib/roomSession'
import { toast } from '../lib/toast'

interface Props {
  stream: StreamView
  large: boolean
  onFocus?: () => void
  /** Erro ao abrir a janela destacada (mostrado como aviso na sala). */
  onError?: (message: string) => void
}

export default function StreamTile({ stream, large, onFocus, onError }: Props): React.JSX.Element {
  const containerRef = useRef<HTMLDivElement>(null)
  const videoRef = useRef<HTMLVideoElement>(null)
  const audioRef = useRef<HTMLAudioElement>(null)
  const [mutedFrom, setMutedFrom] = useState<number | null>(null)
  const [popped, setPopped] = useState(() => isPopped(stream.identity))
  const videoTrack = stream.video?.track
  const audioTrack = stream.isLocal ? undefined : stream.audio?.track
  const volume = session.volumeOf(stream.name)

  useEffect(() => onPopoutChange(() => setPopped(isPopped(stream.identity))), [stream.identity])

  useEffect(() => {
    const el = videoRef.current
    if (!videoTrack || !el) return
    videoTrack.attach(el)
    return () => void videoTrack.detach(el)
  }, [videoTrack])

  useEffect(() => {
    const el = audioRef.current
    if (!audioTrack || !el) return
    audioTrack.attach(el)
    return () => void audioTrack.detach(el)
  }, [audioTrack])

  const setVolume = (v: number): void => session.setVolume(stream.identity, stream.name, v)

  const toggleMute = (): void => {
    if (volume > 0) {
      setMutedFrom(volume)
      setVolume(0)
    } else {
      setVolume(mutedFrom || 1)
      setMutedFrom(null)
    }
  }

  const fullscreen = (): void => {
    if (document.fullscreenElement) void document.exitFullscreen()
    else void containerRef.current?.requestFullscreen()
  }

  const togglePopout = (): void => {
    if (popped) {
      closePopout(stream.identity)
      return
    }
    void popOut(stream)
      .then(() => toast('Transmissão destacada em outra janela', 'success'))
      .catch((err) => onError?.(errorMessage(err)))
  }

  const popoutButton = (
    <button
      className={`icon${popped ? ' on' : ''}`}
      title={popped ? 'Trazer a transmissão de volta' : 'Destacar em outra janela (outro monitor)'}
      onClick={togglePopout}
    >
      <PictureInPicture2 size={15} />
    </button>
  )

  const off = stream.receive === 'off'

  return (
    <div className={`tile ${large ? 'large' : 'small'}`} ref={containerRef} onDoubleClick={fullscreen}>
      {off ? (
        <button className="tile-off" onClick={() => session.setReceive(stream.identity, 'auto')}>
          Transmissão desligada. Clique para ligar.
        </button>
      ) : (
        <video ref={videoRef} autoPlay playsInline muted onClick={large ? undefined : onFocus} />
      )}
      <audio ref={audioRef} autoPlay />

      <div className="tile-label">
        <span className="live-dot" /> {stream.isLocal ? 'Você' : stream.name}
      </div>

      {!stream.isLocal && (
        <div className="tile-controls" onDoubleClick={(e) => e.stopPropagation()}>
          {stream.audio && (
            <>
              <button className="icon" title={volume > 0 ? 'Silenciar' : 'Ativar som'} onClick={toggleMute}>
                {volume > 0 ? <Volume2 size={15} /> : <VolumeX size={15} />}
              </button>
              <input
                type="range"
                min={0}
                max={2}
                step={0.05}
                value={volume}
                onChange={(e) => setVolume(Number(e.target.value))}
                title={`Volume: ${Math.round(volume * 100)}%`}
              />
              <span className="volume-value">{Math.round(volume * 100)}%</span>
            </>
          )}
          <select
            value={stream.receive}
            onChange={(e) => session.setReceive(stream.identity, e.target.value as ReceiveQuality)}
            title="Qualidade que você recebe"
          >
            <option value="auto">Auto</option>
            <option value="low">Baixa</option>
            <option value="off">Desligar</option>
          </select>
          {videoTrack && !off && popoutButton}
          <button className="icon" title="Tela cheia" onClick={fullscreen}>
            <Maximize2 size={15} />
          </button>
        </div>
      )}

      {stream.isLocal && videoTrack && !off && (
        <div className="tile-controls" onDoubleClick={(e) => e.stopPropagation()}>
          {popoutButton}
        </div>
      )}
    </div>
  )
}
