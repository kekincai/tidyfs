//! 操作日志。写到部署目录下的 `journals`（见 `paths`），绝不写进被处理的文件夹：
//! 不污染用户的目录，磁盘根目录通常也没有写权限，日志也不该出现在下一次扫描结果里。

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::domain::report::Report;

pub use super::paths::journal_dir;

/// 写入一次执行的日志。没有任何实际操作时不写文件。
pub fn write(root: &Path, task: &str, report: &Report) -> Result<Option<PathBuf>> {
    if report.is_empty() {
        return Ok(None);
    }

    let dir = journal_dir();
    fs::create_dir_all(&dir).with_context(|| format!("无法创建日志目录 {}", dir.display()))?;

    let now = timestamp();
    let file_name = format!(
        "tidyfs-{}-{}-{}.log",
        now.replace([':', '.'], "-"),
        task,
        sanitize(root)
    );
    let path = dir.join(file_name);
    fs::write(&path, render(root, task, &now, report))
        .with_context(|| format!("无法写入日志 {}", path.display()))?;
    Ok(Some(path))
}

fn render(root: &Path, task: &str, now: &str, report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# tidyfs journal");
    let _ = writeln!(out, "created_at = {now}");
    let _ = writeln!(out, "task = {task}");
    let _ = writeln!(out, "root = {}", root.display());
    if report.cancelled {
        let _ = writeln!(out, "cancelled = true");
    }
    for moved in &report.moves {
        let _ = writeln!(
            out,
            "MOVE\t{}\t{}",
            moved.from.display(),
            moved.to.display()
        );
    }
    for file in &report.removed_files {
        let _ = writeln!(out, "REMOVE_FILE\t{}", file.display());
    }
    for dir in &report.removed_dirs {
        let _ = writeln!(out, "REMOVE_DIR\t{}", dir.display());
    }
    for failure in &report.failures {
        let _ = writeln!(
            out,
            "FAILED\t{}\t{}\t{}",
            failure.action.as_str(),
            failure.path.display(),
            failure.error
        );
    }
    out
}

fn sanitize(root: &Path) -> String {
    let name: String = root
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let name = name.trim_matches('_');
    let name: String = name
        .chars()
        .rev()
        .take(40)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if name.is_empty() {
        "root".to_string()
    } else {
        name
    }
}

fn timestamp() -> String {
    OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown-time".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_readable_tail() {
        assert_eq!(sanitize(Path::new(r"C:\")), "C");
        assert_eq!(sanitize(Path::new(r"D:\照片\2025")), "D__照片_2025");
    }
}
