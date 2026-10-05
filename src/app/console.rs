//! 纯命令行模式：多根目录并行扫描，每个根目录一行动态进度。

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::{Result, bail};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::domain::report::Report;
use crate::engine::{self, Event, Task};
use crate::infra::config::LoadedConfig;
use crate::infra::fsutil::normalize_roots;

use super::cli::TaskArgs;

pub fn run_task(config: &LoadedConfig, task: Task, args: &TaskArgs) -> Result<i32> {
    let options = config.scan_options(args.default_ignored, &args.ignore_names);
    let roots = normalize_roots(resolve_paths(&args.paths)?);
    let mut out = Output::default();

    if let Some(path) = &config.path {
        out.line(format!("[配置] {}", path.display()));
    }

    let multi = MultiProgress::new();
    let bars = roots
        .iter()
        .map(|root| {
            let bar = multi.add(ProgressBar::new_spinner());
            bar.set_style(spinner_style());
            bar.enable_steady_tick(Duration::from_millis(80));
            bar.set_prefix(root.display().to_string());
            bar.set_message("扫描中…");
            bar
        })
        .collect::<Vec<_>>();

    let cancel = AtomicBool::new(false);
    let outcomes = engine::scan(&roots, task, &options, &cancel, &|index, event| {
        if let Event::Scan { stats, elapsed } = event {
            bars[index].set_message(format!(
                "{} 个目录 · {} 个文件 · {:.1}s",
                stats.dirs,
                stats.files,
                elapsed.as_secs_f32()
            ));
        }
    });

    for (bar, outcome) in bars.iter().zip(&outcomes) {
        match outcome {
            Ok(outcome) => bar.finish_with_message(format!(
                "✓ {} 个目录，找到 {} 项，{:.1}s",
                outcome.stats.dirs,
                outcome.found.len(),
                outcome.elapsed.as_secs_f32()
            )),
            Err(error) => bar.finish_with_message(format!("✗ {error:#}")),
        }
    }
    multi.clear().ok();

    let mut failed = false;
    for (root, outcome) in roots.iter().zip(&outcomes) {
        out.line(String::new());
        match outcome {
            Ok(outcome) => {
                out.line(format!(
                    "== {}  扫描 {} 个目录，用时 {:.1}s，找到 {} 项",
                    root.display(),
                    outcome.stats.dirs,
                    outcome.elapsed.as_secs_f32(),
                    outcome.found.len()
                ));
                let prefix = if args.apply { "" } else { "[预览] " };
                for line in engine::describe(&outcome.found) {
                    out.line(format!("{prefix}{line}"));
                }
            }
            Err(error) => {
                failed = true;
                out.line(format!("== {}  失败: {error:#}", root.display()));
            }
        }
    }

    let ready = outcomes
        .iter()
        .filter_map(|outcome| outcome.as_ref().ok())
        .filter(|outcome| !outcome.found.is_empty())
        .collect::<Vec<_>>();

    if !args.apply || ready.is_empty() {
        if !args.apply && !ready.is_empty() {
            out.line(String::new());
            out.line("以上只是预览，加 --apply 才会真正执行。".to_string());
        }
        out.flush(args.log_file.as_ref())?;
        return Ok(if failed { 1 } else { 0 });
    }

    let multi = MultiProgress::new();
    let bars = ready
        .iter()
        .map(|outcome| {
            let bar = multi.add(ProgressBar::new(1));
            bar.set_style(bar_style());
            bar.set_prefix(outcome.root.display().to_string());
            bar
        })
        .collect::<Vec<_>>();
    let applied = engine::apply(&ready, &options, &cancel, &|index, event| {
        if let Event::Apply { current, total } = event {
            bars[index].set_length(total.max(1) as u64);
            bars[index].set_position(current as u64);
        }
    });
    multi.clear().ok();

    out.line(String::new());
    for outcome in &applied {
        failed |= !outcome.report.failures.is_empty();
        out.line(format!(
            "== {}  {}",
            outcome.root.display(),
            summarize(&outcome.report)
        ));
        for moved in &outcome.report.moves {
            out.line(format!(
                "[已移动] {} -> {}",
                moved.from.display(),
                moved.to.display()
            ));
        }
        for file in &outcome.report.removed_files {
            out.line(format!("[已删除文件] {}", file.display()));
        }
        for dir in &outcome.report.removed_dirs {
            out.line(format!("[已删除] {}", dir.display()));
        }
        for failure in &outcome.report.failures {
            out.line(format!(
                "[失败] {} | {}",
                failure.path.display(),
                failure.error
            ));
        }
        if let Some(journal) = &outcome.journal {
            out.line(format!("[操作日志] {}", journal.display()));
        }
    }
    if task == Task::Empty {
        out.line(
            "提示：如果资源管理器正停在已删除的文件夹里，回到上层目录或按 F5 刷新即可。"
                .to_string(),
        );
    }

    out.flush(args.log_file.as_ref())?;
    Ok(if failed { 1 } else { 0 })
}

fn summarize(report: &Report) -> String {
    let mut parts = Vec::new();
    if !report.moves.is_empty() {
        parts.push(format!("移动 {} 个文件", report.moves.len()));
    }
    parts.push(format!("删除 {} 个目录", report.removed_dirs.len()));
    if !report.failures.is_empty() {
        parts.push(format!("失败 {} 项", report.failures.len()));
    }
    if report.cancelled {
        parts.push("已取消".to_string());
    }
    parts.join("，")
}

fn resolve_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    if !paths.is_empty() {
        return Ok(paths.to_vec());
    }
    match rfd::FileDialog::new()
        .set_title("选择要处理的文件夹（可多选）")
        .pick_folders()
    {
        Some(paths) if !paths.is_empty() => Ok(paths),
        _ => bail!("没有选择文件夹"),
    }
}

#[derive(Default)]
struct Output {
    lines: Vec<String>,
}

impl Output {
    fn line(&mut self, line: String) {
        println!("{line}");
        self.lines.push(line);
    }

    fn flush(&self, path: Option<&PathBuf>) -> Result<()> {
        if let Some(path) = path {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, self.lines.join("\n") + "\n")?;
        }
        Ok(())
    }
}

fn spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.cyan} {prefix:.bold} {msg}")
        .unwrap_or_else(|_| ProgressStyle::default_spinner())
}

fn bar_style() -> ProgressStyle {
    ProgressStyle::with_template("{prefix:.bold} {bar:32.cyan/blue} {pos}/{len}")
        .unwrap_or_else(|_| ProgressStyle::default_bar())
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

pub fn pause(message: &str) {
    print!("{message}");
    let _ = io::stdout().flush();
    let _ = io::stdin().read_line(&mut String::new());
}
