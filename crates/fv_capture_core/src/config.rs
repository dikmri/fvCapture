use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::{CaptureConfig, EncoderConfig, OverlaySettings};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    pub capture: CaptureConfig,
    pub overlay: OverlaySettings,
    pub encoder: EncoderConfig,
    pub include_recording_panel: bool,
}

impl AppConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read config: {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("failed to parse config: {}", path.display()))
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create config dir: {}", parent.display()))?;
        }

        let content = toml::to_string_pretty(self).context("failed to serialize config")?;
        fs::write(path, content)
            .with_context(|| format!("failed to write config: {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_through_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = AppConfig::default();
        config.capture.fps = 15;
        config.overlay.show_mouse = false;
        config.encoder.trim_start_ms = 250;

        config.save(&path).unwrap();
        let loaded = AppConfig::load(&path).unwrap();

        assert_eq!(loaded, config);
    }
}
