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
  const [loading, setLoading] = useState(true)
  const [quality, setQuality] = useState<QualityId>(clampQuality(settings.quality, maxQuality))
  const [contentType, setContentType] = useState<ContentType>(settings.contentType)
  const [shareAudio, setShareAudio] = useState(settings.shareAudio)
  const [audioScope, setAudioScope] = useState<'app' | 'pc'>(settings.audioScope)
  const [windowApp, setWindowApp] = useState<string>('')
  const [blocked, setBlocked] = useState(settings.blockedApps)
  const [sendVolume, setSendVolume] = useState(settings.sendVolume)
  const [starting, setStarting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const loadSources = async (): Promise<void> => {
    setLoading(true)
    const list = await api.screen.sources().catch(() => [] as ScreenSource[])
    setSources(list)
    // mantém a escolha anterior se ela ainda existir (janela pode ter fechado)
    setSourceId((current) => (list.some((s) => s.id === current) ? current : (list[0]?.id ?? '')))
    setLoading(false)
  }

  useEffect(() => {
    void loadSources()
  }, [])

  const streamers = session.streams().filter((s) => !s.isLocal).length + 1
  const estimate = hostUploadMbps(presetById(quality), streamers)
  const screens = sources.filter((s) => s.kind === 'screen')
  const windows = sources.filter((s) => s.kind === 'window')
  const selectedIsWindow = sources.find((s) => s.id === sourceId)?.kind === 'window'

  // app dono da janela escolhida: é o que será capturado no modo "só este app"
  useEffect(() => {
    if (!selectedIsWindow) {
      setWindowApp('')
      return
    }
    let active = true
    void api.audio.resolveWindow(sourceId).then((owner) => {
      if (active) setWindowApp(owner.exe)
    })
    return () => {
      active = false
    }
  }, [sourceId, selectedIsWindow])

  const appScope = shareAudio && selectedIsWindow && audioScope === 'app' && !!windowApp

  const start = async (): Promise<void> => {
    setStarting(true)
    setError(null)
    try {
      await session.updateSettings({ quality, contentType, shareAudio, audioScope, blockedApps: blocked, sendVolume })
      await session.startShare({ sourceId, quality, contentType, audio: shareAudio, audioScope, blocked, sendVolume })
      onClose()
    } catch (err) {
      setError(errorMessage(err))
      setStarting(false)
    }
  }

  const renderSource = (s: ScreenSource): React.JSX.Element => (
    <button
      key={s.id}
      className={`source ${s.kind === 'window' ? 'window' : ''} ${s.id === sourceId ? 'selected' : ''}`}
      onClick={() => setSourceId(s.id)}
      title={s.name}
    >
      <img src={s.thumbnail} alt="" />
      <span>{s.name}</span>
    </button>
  )

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h2>Compartilhar tela ou janela</h2>
          <button className="link" onClick={() => void loadSources()} disabled={loading}>
            {loading ? 'Carregando…' : 'Atualizar'}
          </button>
        </div>

        <div className="modal-body">
          {windows.length > 0 && (
            <>
              <h3 className="sources-title">Janelas</h3>
              <div className="sources windows">{windows.map(renderSource)}</div>
            </>
          )}
          {screens.length > 0 && (
            <>
              <h3 className="sources-title">Telas</h3>
              <div className="sources">{screens.map(renderSource)}</div>
            </>
          )}
          {!loading && sources.length === 0 && (
            <p className="hint">
              Nenhuma tela ou janela encontrada. Abra a janela que quer transmitir e clique em "Atualizar".
            </p>
          )}
          {!loading && sources.length > 0 && windows.length === 0 && (
            <p className="hint">
              O Windows não devolveu nenhuma janela — abra a que quer transmitir e clique em "Atualizar". Enquanto isso,
              dá para compartilhar a tela inteira.
            </p>
          )}

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

          {shareAudio && selectedIsWindow && (
            <div className="audio-scope">
              <label className="check">
                <input
                  type="radio"
                  name="audio-scope"
                  checked={appScope}
                  disabled={!windowApp}
                  onChange={() => setAudioScope('app')}
                />
                Só o áudio deste app{windowApp ? ` (${windowApp})` : ' — não identificado'}
              </label>
              <label className="check">
                <input type="radio" name="audio-scope" checked={!appScope} onChange={() => setAudioScope('pc')} />
                Áudio de todo o PC
              </label>
              {!windowApp && (
                <p className="hint">Não consegui identificar o app desta janela; dá para usar o áudio do PC todo.</p>
              )}
            </div>
          )}

          {shareAudio && !appScope && <AudioAppsList blocked={blocked} onChange={setBlocked} />}

          {shareAudio && (
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
          )}

          {error && <div className="banner error">{error}</div>}
        </div>

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
