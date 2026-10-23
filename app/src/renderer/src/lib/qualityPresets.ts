import { VideoPreset } from 'livekit-client'
import type { ContentType } from '../../../shared/api'
import type { QualityId } from '../../../shared/protocol'

export interface QualityPreset {
  id: QualityId
  label: string
  width: number
  height: number
  fps: number
  /** bits/s da camada principal */
  bitrate: number
}

export const QUALITY_PRESETS: QualityPreset[] = [
  { id: '480p30', label: '480p · 30 fps', width: 854, height: 480, fps: 30, bitrate: 1_000_000 },
  { id: '480p60', label: '480p · 60 fps', width: 854, height: 480, fps: 60, bitrate: 1_500_000 },
  { id: '720p30', label: '720p · 30 fps', width: 1280, height: 720, fps: 30, bitrate: 2_000_000 },
  { id: '720p60', label: '720p · 60 fps', width: 1280, height: 720, fps: 60, bitrate: 3_000_000 },
  { id: '1080p30', label: '1080p · 30 fps', width: 1920, height: 1080, fps: 30, bitrate: 4_000_000 },
  { id: '1080p60', label: '1080p · 60 fps', width: 1920, height: 1080, fps: 60, bitrate: 6_000_000 },
  { id: '1440p60', label: '1440p · 60 fps', width: 2560, height: 1440, fps: 60, bitrate: 10_000_000 }
]

export function presetById(id: QualityId): QualityPreset {
  return QUALITY_PRESETS.find((p) => p.id === id) ?? QUALITY_PRESETS[4]
}

/** Presets permitidos pelo teto de qualidade do host. */
export function allowedPresets(max: QualityId | null): QualityPreset[] {
  if (!max) return QUALITY_PRESETS
  const limit = QUALITY_PRESETS.findIndex((p) => p.id === max)
  return QUALITY_PRESETS.slice(0, limit + 1)
}

export function clampQuality(id: QualityId, max: QualityId | null): QualityId {
  const allowed = allowedPresets(max)
  return allowed.some((p) => p.id === id) ? id : allowed[allowed.length - 1].id
}

/**
 * Camadas extras do simulcast (da menor para a maior). Quem vê em miniatura ou
 * tem pouca banda recebe uma delas em vez da principal.
 */
export function simulcastLayers(preset: QualityPreset): VideoPreset[] {
  if (preset.height <= 480) return [new VideoPreset(480, 270, 200_000, 15)]
  const layers = [new VideoPreset(640, 360, 400_000, 15)]
  if (preset.height >= 1080) layers.push(new VideoPreset(1280, 720, 1_500_000, 30))
  return layers
}

export function contentSettings(type: ContentType): { hint: string; degradation: RTCDegradationPreference } {
  return type === 'motion'
    ? { hint: 'motion', degradation: 'maintain-framerate' }
    : { hint: 'detail', degradation: 'maintain-resolution' }
}

/** Upload aproximado que o host precisa: 9 pessoas vendo, cada uma com 1 tela grande. */
export function hostUploadMbps(preset: QualityPreset, streamers: number): number {
  const thumbs = Math.max(0, streamers - 1) * 0.4
  return Math.round(9 * (preset.bitrate / 1_000_000 + thumbs + 0.13 * streamers))
}
