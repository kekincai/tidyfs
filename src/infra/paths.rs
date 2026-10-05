//! 程序自己的数据目录。日志一律写在部署目录（tidyfs.exe 所在目录）下，
//! 绝不写进被扫描、被处理的文件夹。
//!
//! 查找顺序：环境变量 `TIDYFS_HOME` → exe 所在目录 → `%LOCALAPPDATA%\tidyfs`（部署目录不可写时，
//! 比如装在 Program Files 下）。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 部署目录：`TIDYFS_HOME` 或 exe 所在目录。
pub fn install_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("TIDYFS_HOME") {
        return Some(PathBuf::from(home));
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
}

/// 诊断日志目录。
pub fn log_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| writable_subdir("logs")).clone()
}

/// 操作日志目录。
pub fn journal_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("TIDYFS_JOURNAL_DIR") {
        return PathBuf::from(dir);
    }
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| writable_subdir("journals")).clone()
}

fn writable_subdir(name: &str) -> PathBuf {
    if let Some(dir) = install_dir().map(|dir| dir.join(name))
        && is_writable(&dir)
    {
        return dir;
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("tidyfs")
        .join(name)
}

fn is_writable(dir: &Path) -> bool {
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".tidyfs-write-test");
    let ok = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writable_check() {
        let temp = tempdir().unwrap();
        assert!(is_writable(&temp.path().join("logs")));
        assert!(temp.path().join("logs").is_dir());
        assert!(!temp.path().join("logs").join(".tidyfs-write-test").exists());
    }
}
