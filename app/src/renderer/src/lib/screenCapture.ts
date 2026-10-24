import type { ContentType } from '../../../shared/api'
import type { QualityId } from '../../../shared/protocol'
import { SendAudio } from '../audio/sendAudio'
import { api, nextAudioPort } from './api'
import { contentSettings, presetById, type QualityPreset } from './qualityPresets'

export interface ShareOptions {
  sourceId: string
  quality: QualityId
  contentType: ContentType
  audio: boolean
  /** Com janela escolhida: 'app' = só o áudio do app dela; 'pc' = todos os apps. */
  audioScope: 'app' | 'pc'
  blocked: string[]
  sendVolume: number
}

export interface LocalShare {
  video: MediaStreamTrack
  audio: SendAudio | null
  /** native = por app (com bloqueio), system = áudio do PC inteiro (fallback) */
  audioMode: 'native' | 'system' | 'none'
  preset: QualityPreset
  contentType: ContentType
  warning: string | null
  stop(): void
}

function describeCaptureError(err: unknown): Error {
  const name = err instanceof DOMException ? err.name : ''
  if (name === 'NotReadableError') {
    return new Error(
      'O Windows não deixou capturar a tela. Isso acontece quando o app está rodando como administrador: feche e abra o app normalmente.'
    )
  }
  if (name === 'NotAllowedError') return new Error('A captura de tela foi recusada.')
  return err instanceof Error ? err : new Error(String(err))
}

export async function startScreenShare(opts: ShareOptions): Promise<LocalShare> {
  const preset = presetById(opts.quality)
  let audio: SendAudio | null = null
  let audioMode: LocalShare['audioMode'] = 'none'
  let warning: string | null = null
  // "só este app" só faz sentido para janela: o Windows captura áudio por processo
  const appScope = opts.audio && opts.audioScope === 'app' && opts.sourceId.startsWith('window:')

  if (opts.audio) {
    const port = nextAudioPort()
    let only: string[] = []
    if (appScope) {
      const owner = await api.audio.resolveWindow(opts.sourceId)
      if (owner.exe) {
        only = [owner.exe]
      } else {
        warning =
          'Não consegui descobrir o app desta janela. O áudio de todos os apps (menos os bloqueados) será transmitido.'
      }
    }
    const result = await api.audio.start(opts.blocked, only)
    if (result.ok) {
      const p = await port
      if (p) {
        audio = await SendAudio.fromPort(p, opts.sendVolume)
        audioMode = 'native'
      } else {
        await api.audio.stop()
      }
    } else {
      warning = `Bloqueio de apps indisponível (${result.error}). O áudio de todo o PC será transmitido.`
    }
  }

  const systemAudio = opts.audio && audioMode !== 'native'
  await api.screen.select(opts.sourceId, systemAudio)
  let stream: MediaStream
  try {
    stream = await navigator.mediaDevices.getDisplayMedia({
      video: {
        width: { max: preset.width },
        height: { max: preset.height },
        frameRate: { ideal: preset.fps, max: preset.fps }
      },
      audio: systemAudio
    })
  } catch (err) {
    audio?.close()
    if (audioMode === 'native') await api.audio.stop()
    throw describeCaptureError(err)
  }

  const video = stream.getVideoTracks()[0]
  video.contentHint = contentSettings(opts.contentType).hint
  if (systemAudio) {
    const track = stream.getAudioTracks()[0]
    if (track) {
      audio = SendAudio.fromTrack(track, opts.sendVolume)
      audioMode = 'system'
    }
  }

  return {
    video,
    audio,
    audioMode,
    preset,
    contentType: opts.contentType,
    warning,
    stop() {
      video.stop()
      audio?.close()
      if (audioMode === 'native') void api.audio.stop()
    }
  }
}
