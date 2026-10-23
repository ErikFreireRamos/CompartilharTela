import { useEffect, useState } from 'react'
import type { ContentType, ScreenSource } from '../../../shared/api'
import type { QualityId } from '../../../shared/protocol'
import { api, errorMessage } from '../lib/api'
import { allowedPresets, clampQuality, hostUploadMbps, presetById } from '../lib/qualityPresets'
import { session } from '../lib/roomSession'
import AudioAppsList from './AudioAppsList'

export default function ShareDialog({ onClose }: { onClose: () => void }): React.JSX.Element {
  const settings = session.settings!
  const maxQuality = session.meta?.maxQuality ?? null
  const [sources, setSources] = useState<ScreenSource[]>([])
  const [sourceId, setSourceId] = useState<string>('')
  const [quality, setQuality] = useState<QualityId>(clampQuality(settings.quality, maxQuality))
  const [contentType, setContentType] = useState<ContentType>(settings.contentType)
  const [shareAudio, setShareAudio] = useState(settings.shareAudio)
  const [blocked, setBlocked] = useState(settings.blockedApps)
  const [sendVolume, setSendVolume] = useState(settings.sendVolume)
  const [starting, setStarting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    void api.screen.sources().then((list) => {
      setSources(list)
      setSourceId((current) => current || list[0]?.id || '')
    })
  }, [])

  const streamers = session.streams().filter((s) => !s.isLocal).length + 1
  const estimate = hostUploadMbps(presetById(quality), streamers)

  const start = async (): Promise<void> => {
    setStarting(true)
    setError(null)
    try {
      await session.updateSettings({ quality, contentType, shareAudio, blockedApps: blocked, sendVolume })
      await session.startShare({ sourceId, quality, contentType, audio: shareAudio, blocked, sendVolume })
      onClose()
    } catch (err) {
      setError(errorMessage(err))
      setStarting(false)
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>Compartilhar tela</h2>

        <div className="sources">
          {sources.map((s) => (
            <button
              key={s.id}
              className={`source ${s.id === sourceId ? 'selected' : ''}`}
              onClick={() => setSourceId(s.id)}
            >
              <img src={s.thumbnail} alt="" />
              <span>{s.name}</span>
            </button>
          ))}
        </div>

        <div className="form-row">
          <label className="field">
            <span>Qualidade</span>
            <select value={quality} onChange={(e) => setQuality(e.target.value as QualityId)}>
              {allowedPresets(maxQuality).map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>Conteúdo</span>
            <select value={contentType} onChange={(e) => setContentType(e.target.value as ContentType)}>
              <option value="motion">Jogo / vídeo (fluidez)</option>
              <option value="detail">Texto / código (nitidez)</option>
            </select>
          </label>
        </div>
        <p className="hint">
          Upload estimado do host com {streamers} {streamers === 1 ? 'transmissão' : 'transmissões'}: ~{estimate} Mbps
          {maxQuality && ` · teto definido pelo host: ${presetById(maxQuality).label}`}
        </p>

        <label className="check">
          <input type="checkbox" checked={shareAudio} onChange={(e) => setShareAudio(e.target.checked)} />
          Transmitir o áudio do PC
        </label>

        {shareAudio && (
          <>
            <AudioAppsList blocked={blocked} onChange={setBlocked} />
            <label className="field">
              <span>Volume da transmissão: {Math.round(sendVolume * 100)}%</span>
              <input
                type="range"
                min={0}
                max={2}
                step={0.05}
                value={sendVolume}
                onChange={(e) => setSendVolume(Number(e.target.value))}
              />
            </label>
          </>
        )}

        {error && <div className="banner error">{error}</div>}

        <div className="modal-actions">
          <button onClick={onClose}>Cancelar</button>
          <button className="primary" disabled={!sourceId || starting} onClick={() => void start()}>
            {starting ? 'Iniciando…' : 'Transmitir'}
          </button>
        </div>
      </div>
    </div>
  )
}
