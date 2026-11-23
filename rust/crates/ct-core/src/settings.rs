use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::protocol::{
    AudioQualityId, ContentType, QualityId, RECOMMENDED_AUDIO_QUALITY, RECOMMENDED_FPS,
    RECOMMENDED_RESOLUTION,
};

pub const DEFAULT_BLOCKED_APPS: &[&str] = &[
    "discord.exe",
    "discordptb.exe",
    "discordcanary.exe",
    "vesktop.exe",
];

pub const DEFAULT_CUSTOM_ACCENT: [u8; 3] = [56, 189, 248];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeId {
    #[default]
    Red,
    MarathonGreen,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub client_id: String,
    pub nickname: String,
    pub blocked_apps: Vec<String>,
    pub volumes: HashMap<String, f32>,
    pub quality: QualityId,
    pub fps: u32,
    pub audio_quality: AudioQualityId,
    pub gpu_adapter_index: Option<u32>,
    pub theme: ThemeId,
    pub custom_accent: [u8; 3],
    pub content_type: ContentType,
    pub share_audio: bool,
    pub send_volume: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            client_id: Uuid::new_v4().to_string(),
            nickname: String::new(),
            blocked_apps: DEFAULT_BLOCKED_APPS.iter().map(|s| (*s).to_string()).collect(),
            volumes: HashMap::new(),
            quality: RECOMMENDED_RESOLUTION.to_quality_id(RECOMMENDED_FPS),
            fps: RECOMMENDED_FPS,
            audio_quality: RECOMMENDED_AUDIO_QUALITY,
            gpu_adapter_index: None,
            theme: ThemeId::Red,
            custom_accent: DEFAULT_CUSTOM_ACCENT,
            content_type: ContentType::Motion,
            share_audio: true,
            send_volume: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    file: PathBuf,
    data: Settings,
}

impl SettingsStore {
    pub fn default_data_dir(profile: Option<&str>) -> PathBuf {
        let base = directories::BaseDirs::new()
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        match profile {
            Some(p) if !p.is_empty() => base.join(format!("CompartilhadorDeTela-{p}")),
            _ => base.join("CompartilhadorDeTela"),
        }
    }

    pub fn load(file: impl Into<PathBuf>) -> Self {
        let file = file.into();
        let data = fs::read_to_string(&file)
            .ok()
            .and_then(|raw| serde_json::from_str::<Settings>(&raw).ok())
            .unwrap_or_default();
        let store = Self { file, data };
        let _ = store.save();
        store
    }

    pub fn get(&self) -> &Settings {
        &self.data
    }

    pub fn update<F>(&mut self, mutate: F) -> &Settings
    where
        F: FnOnce(&mut Settings),
    {
        let original_id = self.data.client_id.clone();
        mutate(&mut self.data);
        self.data.client_id = original_id;
        let _ = self.save();
        &self.data
    }

    pub fn path(&self) -> &Path {
        &self.file
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent)?;
        }
        let payload = serde_json::to_string_pretty(&self.data)?;
        let tmp = self.file.with_extension("json.tmp");
        if fs::write(&tmp, &payload).is_ok() && fs::rename(&tmp, &self.file).is_ok() {
            return Ok(());
        }
        // Fallback no Windows caso o rename atômico falhe por lock temporário
        let _ = fs::remove_file(&tmp);
        fs::write(&self.file, payload)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_theme_and_custom_rgb_color() {
        let dir = std::env::temp_dir().join(format!("pit-settings-test-{}", Uuid::new_v4()));
        let path = dir.join("settings.json");

        let mut store = SettingsStore::load(&path);
        assert_eq!(store.get().theme, ThemeId::Red);

        store.update(|s| {
            s.theme = ThemeId::Custom;
            s.custom_accent = [255, 110, 40];
        });

        let reloaded = SettingsStore::load(&path);
        assert_eq!(reloaded.get().theme, ThemeId::Custom);
        assert_eq!(reloaded.get().custom_accent, [255, 110, 40]);

        let _ = fs::remove_dir_all(&dir);
    }
}
