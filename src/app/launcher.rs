//! 双击 exe 时启动 Node.js 界面，并告诉界面引擎在哪里。

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};

const UI_SCRIPT: &str = "ui/dist/cli.js";

pub fn launch() -> Result<i32> {
    let exe = std::env::current_exe().context("无法确定程序路径")?;
    let script = find_ui_script(&exe).ok_or_else(|| {
        anyhow!("找不到界面文件 {UI_SCRIPT}。请先在 ui 目录执行 npm install && npm run build")
    })?;
    tracing::info!(script = %script.display(), "launching ui");

    let status = Command::new("node")
        .arg(&script)
        .env("TIDYFS_ENGINE", &exe)
        .status()
        .context("无法启动 Node.js，请确认已安装 Node.js 22 或更新版本")?;
    Ok(status.code().unwrap_or(1))
}

/// 依次查找：环境变量 TIDYFS_UI，exe 所在目录及其上几级目录（兼容 target/release 布局）。
fn find_ui_script(exe: &Path) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("TIDYFS_UI").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    exe.ancestors()
        .skip(1)
        .take(4)
        .map(|dir| dir.join(UI_SCRIPT))
        .find(|candidate| candidate.is_file())
}
