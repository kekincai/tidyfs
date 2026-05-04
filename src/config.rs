use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::cli::FlattenMode as CliFlattenMode;
use crate::ops::{FlattenMode, ScanOptions};

pub const DEFAULT_CONFIG_FILE: &str = "tidyfs.toml";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub ignore_names: Vec<String>,
    pub default_ignored: Option<bool>,
    pub flatten_mode: Option<ConfigFlattenMode>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigFlattenMode {
    KeepEndpoints,
    OneLevel,
    CollapseChain,
}

#[derive(Debug, Clone, Default)]
pub struct LoadedConfig {
    pub path: Option<PathBuf>,
    pub config: AppConfig,
}

pub fn load_config(path: Option<&Path>) -> Result<LoadedConfig> {
    let config_path = match path {
        Some(path) => Some(path.to_path_buf()),
        None => {
            let candidate = PathBuf::from(DEFAULT_CONFIG_FILE);
            if candidate.exists() {
                Some(candidate)
            } else {
                None
            }
        }
    };

    let Some(path) = config_path else {
        return Ok(LoadedConfig::default());
    };

    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    let config: AppConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse config file {}", path.display()))?;

    Ok(LoadedConfig {
        path: Some(path),
        config,
    })
}

impl LoadedConfig {
    pub fn scan_options(
        &self,
        default_ignored: Option<bool>,
        extra_ignored_names: &[String],
    ) -> ScanOptions {
        let use_default_ignored = default_ignored
            .or(self.config.default_ignored)
            .unwrap_or(true);
        let mut options = if use_default_ignored {
            ScanOptions::default()
        } else {
            ScanOptions::from_names(Vec::<String>::new())
        };

        options.extend_names(&self.config.ignore_names);
        options.extend_names(extra_ignored_names);
        options
    }

    pub fn flatten_mode(&self, cli_mode: Option<CliFlattenMode>) -> FlattenMode {
        if let Some(mode) = cli_mode {
            return match mode {
                CliFlattenMode::KeepEndpoints => FlattenMode::KeepEndpoints,
                CliFlattenMode::OneLevel => FlattenMode::OneLevel,
                CliFlattenMode::CollapseChain => FlattenMode::CollapseChain,
            };
        }

        match self
            .config
            .flatten_mode
            .unwrap_or(ConfigFlattenMode::KeepEndpoints)
        {
            ConfigFlattenMode::KeepEndpoints => FlattenMode::KeepEndpoints,
            ConfigFlattenMode::OneLevel => FlattenMode::OneLevel,
            ConfigFlattenMode::CollapseChain => FlattenMode::CollapseChain,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_scan_options_merge_defaults_and_cli() {
        let loaded = LoadedConfig {
            path: None,
            config: AppConfig {
                ignore_names: vec!["foo.tmp".to_string()],
                default_ignored: Some(false),
                flatten_mode: Some(ConfigFlattenMode::CollapseChain),
            },
        };

        let options = loaded.scan_options(Some(true), &["bar.tmp".to_string()]);

        assert!(options.matches_name("desktop.ini"));
        assert!(options.matches_name("foo.tmp"));
        assert!(options.matches_name("bar.tmp"));
    }

    #[test]
    fn config_supplies_flatten_mode_when_cli_missing() {
        let loaded = LoadedConfig {
            path: None,
            config: AppConfig {
                ignore_names: Vec::new(),
                default_ignored: None,
                flatten_mode: Some(ConfigFlattenMode::CollapseChain),
            },
        };

        assert_eq!(loaded.flatten_mode(None), FlattenMode::CollapseChain);
        assert_eq!(
            loaded.flatten_mode(Some(CliFlattenMode::OneLevel)),
            FlattenMode::OneLevel
        );
        assert_eq!(
            LoadedConfig::default().flatten_mode(None),
            FlattenMode::KeepEndpoints
        );
    }
}
