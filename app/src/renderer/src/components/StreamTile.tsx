import { useEffect, useRef, useState } from 'react'
import { session, type ReceiveQuality, type StreamView } from '../lib/roomSession'

interface Props {
  stream: StreamView
  large: boolean
  onFocus?: () => void
}

export default function StreamTile({ stream, large, onFocus }: Props): React.JSX.Element {
  const containerRef = useRef<HTMLDivElement>(null)
  const videoRef = useRef<HTMLVideoElement>(null)
  const audioRef = useRef<HTMLAudioElement>(null)
  const [mutedFrom, setMutedFrom] = useState<number | null>(null)
  const videoTrack = stream.video?.track
  const audioTrack = stream.isLocal ? undefined : stream.audio?.track
  const volume = session.volumeOf(stream.name)

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
                {volume > 0 ? '🔊' : '🔇'}
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
          <button className="icon" title="Tela cheia" onClick={fullscreen}>
            ⛶
          </button>
        </div>
      )}
    </div>
  )
}
