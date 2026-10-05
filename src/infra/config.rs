use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::domain::flatten::FlattenMode;
use crate::domain::options::ScanOptions;

pub const DEFAULT_CONFIG_FILE: &str = "tidyfs.toml";

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    #[serde(default)]
    pub ignore_names: Vec<String>,
    #[serde(default)]
    pub skip_dirs: Vec<String>,
    pub default_ignored: Option<bool>,
    pub hidden_files_are_noise: Option<bool>,
    pub flatten_mode: Option<FlattenMode>,
}

#[derive(Debug, Clone, Default)]
pub struct LoadedConfig {
    pub path: Option<PathBuf>,
    pub config: AppConfig,
}

/// 读取配置文件。未显式指定时，依次尝试当前目录、程序所在目录下的 tidyfs.toml。
pub fn load_config(path: Option<&Path>) -> Result<LoadedConfig> {
    let path = match path {
        Some(path) => Some(path.to_path_buf()),
        None => default_candidates()
            .into_iter()
            .find(|candidate| candidate.is_file()),
    };
    let Some(path) = path else {
        return Ok(LoadedConfig::default());
    };

    let content = fs::read_to_string(&path)
        .with_context(|| format!("无法读取配置文件 {}", path.display()))?;
    let config =
        toml::from_str(&content).with_context(|| format!("配置文件格式错误 {}", path.display()))?;
    Ok(LoadedConfig {
        path: Some(path),
        config,
    })
}

fn default_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![PathBuf::from(DEFAULT_CONFIG_FILE)];
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        candidates.push(dir.join(DEFAULT_CONFIG_FILE));
    }
    candidates
}

impl LoadedConfig {
    /// 命令行参数优先，其次配置文件，最后是内置默认值。忽略名单是合并关系。
    pub fn scan_options(
        &self,
        default_ignored: Option<bool>,
        extra_ignored: &[String],
    ) -> ScanOptions {
        let use_default = default_ignored
            .or(self.config.default_ignored)
            .unwrap_or(true);
        let mut options = ScanOptions::new(
            use_default,
            &self.config.ignore_names,
            &self.config.skip_dirs,
        );
        options.add_ignored(extra_ignored);
        options.hidden_files_are_noise = self.config.hidden_files_are_noise.unwrap_or(true);
        options
    }

    pub fn flatten_mode(&self, cli_mode: Option<FlattenMode>) -> FlattenMode {
        cli_mode.or(self.config.flatten_mode).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn scan_options_merge_config_and_cli() {
        let loaded = LoadedConfig {
            path: None,
            config: AppConfig {
                ignore_names: vec!["foo.tmp".to_string()],
                skip_dirs: vec!["cache".to_string()],
                default_ignored: Some(false),
                hidden_files_are_noise: None,
                flatten_mode: None,
            },
        };

        let options = loaded.scan_options(Some(true), &["bar.tmp".to_string()]);
        assert!(options.is_ignored_file(OsStr::new("desktop.ini")));
        assert!(options.is_ignored_file(OsStr::new("foo.tmp")));
        assert!(options.is_ignored_file(OsStr::new("bar.tmp")));
        assert!(options.is_skipped_dir(OsStr::new("cache"), false));

        let options = loaded.scan_options(None, &[]);
        assert!(!options.is_ignored_file(OsStr::new("desktop.ini")));
    }

    #[test]
    fn flatten_mode_precedence() {
        let loaded = LoadedConfig {
            path: None,
            config: toml::from_str("flatten_mode = \"collapse-chain\"").unwrap(),
        };
        assert_eq!(loaded.flatten_mode(None), FlattenMode::CollapseChain);
        assert_eq!(
            loaded.flatten_mode(Some(FlattenMode::OneLevel)),
            FlattenMode::OneLevel
        );
        assert_eq!(
            LoadedConfig::default().flatten_mode(None),
            FlattenMode::KeepEndpoints
        );
    }
}
