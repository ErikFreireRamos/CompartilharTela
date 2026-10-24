import { randomUUID } from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import type { Settings } from '../shared/api'

export const DEFAULT_BLOCKED_APPS = ['discord.exe', 'discordptb.exe', 'discordcanary.exe', 'vesktop.exe']

const DEFAULTS: Omit<Settings, 'clientId'> = {
  nickname: '',
  blockedApps: DEFAULT_BLOCKED_APPS,
  volumes: {},
  quality: '1080p30',
  contentType: 'motion',
  shareAudio: true,
  audioScope: 'app',
  sendVolume: 1
}

export class SettingsStore {
  private data: Settings

  constructor(private readonly file: string) {
    let saved: Partial<Settings> = {}
    try {
      saved = JSON.parse(fs.readFileSync(file, 'utf8'))
    } catch {
      // primeira execução
    }
    this.data = { ...DEFAULTS, clientId: randomUUID(), ...saved }
    this.save()
  }

  get(): Settings {
    return this.data
  }

  set(patch: Partial<Settings>): Settings {
    const { clientId: _ignored, ...rest } = patch
    this.data = { ...this.data, ...rest }
    this.save()
    return this.data
  }

  private save(): void {
    fs.mkdirSync(path.dirname(this.file), { recursive: true })
    fs.writeFileSync(this.file, JSON.stringify(this.data, null, 2))
  }
}
