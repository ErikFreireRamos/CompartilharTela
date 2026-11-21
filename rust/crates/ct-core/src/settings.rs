use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::protocol::{ContentType, QualityId};

pub const DEFAULT_BLOCKED_APPS: &[&str] = &[
    "discord.exe",
    "discordptb.exe",
    "discordcanary.exe",
    "vesktop.exe",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub client_id: String,
    pub nickname: String,
    pub blocked_apps: Vec<String>,
    pub volumes: HashMap<String, f32>,
    pub quality: QualityId,
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
            quality: QualityId::P1080F30,
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
        let tmp = self.file.with_extension("json.tmp");
        let payload = serde_json::to_string_pretty(&self.data)?;
        fs::write(&tmp, payload)?;
        fs::rename(tmp, &self.file)?;
        Ok(())
    }
}
