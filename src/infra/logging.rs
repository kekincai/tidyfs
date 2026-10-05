//! 诊断日志（tracing）。和“操作日志”（journal）分开：
//! - journal 记录每次实际移动/删除了什么，是给人核对用的审计记录；
//! - 这里记录扫描耗时、错误、调度情况，用来排查问题。
//!
//! 日志写到 `%LOCALAPPDATA%\tidyfs\logs`，按天滚动，保留 14 天。
//! `serve` 模式下 stdout 是 JSON 协议通道，所以日志永远不写 stdout。
//! 日志级别用环境变量 `TIDYFS_LOG` 控制，例如 `TIDYFS_LOG=debug`。

use std::path::PathBuf;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::fmt::time::LocalTime;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, fmt};

pub fn log_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("tidyfs")
        .join("logs")
}

/// 持有它直到程序退出，退出时会把缓冲区里的日志写完。
pub struct LogGuard {
    _file: Option<WorkerGuard>,
}

/// `component` 用作日志文件名前缀，例如 `engine`、`cli`。`verbose` 时同时输出到 stderr。
pub fn init(component: &str, verbose: bool) -> LogGuard {
    let filter = || {
        EnvFilter::try_from_env("TIDYFS_LOG")
            .unwrap_or_else(|_| EnvFilter::new(if verbose { "debug" } else { "info" }))
    };

    let dir = log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix(component)
        .filename_suffix("log")
        .max_log_files(14)
        .build(dir)
        .ok();

    let (file_layer, guard) = match appender {
        Some(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let layer = fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .with_timer(LocalTime::rfc_3339())
                .with_thread_names(true)
                .with_filter(filter());
            (Some(layer), Some(guard))
        }
        None => (None, None),
    };

    let stderr_layer = verbose.then(|| {
        fmt::layer()
            .with_writer(std::io::stderr)
            .with_timer(LocalTime::rfc_3339())
            .with_filter(filter())
    });

    let _ = tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .try_init();

    LogGuard { _file: guard }
}
