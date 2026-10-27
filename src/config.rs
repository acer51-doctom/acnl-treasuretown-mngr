use crate::storage;
use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub config_file: PathBuf,
    pub towns_dir: PathBuf,
    pub templates_dir: PathBuf,
}

impl AppPaths {
    pub fn detect() -> Result<Self> {
        let base = BaseDirs::new().context("unable to discover user home directory")?;
        let root = base.home_dir().join(".local/share/acnl-treasuretown-mngr");
        Ok(Self {
            config_file: root.join("config.toml"),
            towns_dir: root.join("towns"),
            templates_dir: root.join("templates"),
            root,
        })
    }

    pub fn ensure_layout(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::create_dir_all(&self.towns_dir)?;
        std::fs::create_dir_all(&self.templates_dir)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_max_active_towns")]
    pub max_active_towns: usize,
    #[serde(default = "default_session_limit_minutes")]
    pub default_session_limit_minutes: u64,
}

const fn default_max_active_towns() -> usize {
    1
}

const fn default_session_limit_minutes() -> u64 {
    45
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            max_active_towns: default_max_active_towns(),
            default_session_limit_minutes: default_session_limit_minutes(),
        }
    }
}

pub struct ConfigStore {
    paths: AppPaths,
}

impl ConfigStore {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub fn load_or_create(&self) -> Result<AppConfig> {
        self.paths.ensure_layout()?;
        if !self.paths.config_file.exists() {
            let cfg = AppConfig::default();
            self.save(&cfg)?;
            return Ok(cfg);
        }

        let raw = std::fs::read_to_string(&self.paths.config_file)
            .with_context(|| format!("failed to read {}", self.paths.config_file.display()))?;
        let cfg = toml::from_str::<AppConfig>(&raw).context("failed to parse config.toml")?;
        Ok(cfg)
    }

    pub fn save(&self, cfg: &AppConfig) -> Result<()> {
        let data = toml::to_string_pretty(cfg)?;
        storage::atomic_write(&self.paths.config_file, data.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn creates_default_config() {
        let tmp = TempDir::new().expect("tempdir");
        let paths = AppPaths {
            root: tmp.path().join("data"),
            config_file: tmp.path().join("data/config.toml"),
            towns_dir: tmp.path().join("data/towns"),
            templates_dir: tmp.path().join("data/templates"),
        };

        let store = ConfigStore::new(paths.clone());
        let cfg = store.load_or_create().expect("load config");

        assert_eq!(cfg.default_session_limit_minutes, 45);
        assert!(paths.config_file.exists());
        assert!(paths.towns_dir.exists());
        assert!(paths.templates_dir.exists());
    }
}
