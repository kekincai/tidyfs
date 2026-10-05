//! 多根目录 / 多磁盘调度。
//!
//! 同一块磁盘上的根目录放在同一个线程里顺序处理：单块磁盘上多线程读大量小目录反而更慢；
//! 不同磁盘各开一个线程并行，互不影响。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use tracing::{info, info_span, warn};

use crate::domain::empty::{self, EmptyDir};
use crate::domain::flatten::{self, FlattenMode, FlattenPlan};
use crate::domain::options::ScanOptions;
use crate::domain::report::Report;
use crate::domain::tree::{ScanStats, Tree, is_cancelled};
use crate::infra::fsutil::{is_volume_root, volume_key};
use crate::infra::journal;

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    Empty,
    Flatten(FlattenMode),
}

impl Task {
    pub fn name(self) -> &'static str {
        match self {
            Task::Empty => "empty",
            Task::Flatten(_) => "flatten",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Found {
    Empty(Vec<EmptyDir>),
    Flatten(Vec<FlattenPlan>),
}

impl Found {
    pub fn len(&self) -> usize {
        match self {
            Found::Empty(dirs) => dirs.len(),
            Found::Flatten(plans) => plans.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn ignored_file_count(&self) -> usize {
        match self {
            Found::Empty(dirs) => dirs.iter().map(|dir| dir.ignored_files.len()).sum(),
            Found::Flatten(plans) => plans.iter().map(|plan| plan.ignored_files.len()).sum(),
        }
    }

    pub fn file_count(&self) -> usize {
        match self {
            Found::Empty(_) => 0,
            Found::Flatten(plans) => plans.iter().map(|plan| plan.file_count as usize).sum(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScanOutcome {
    pub root: PathBuf,
    pub task: Task,
    pub stats: ScanStats,
    pub elapsed: Duration,
    pub found: Found,
}

#[derive(Debug, Clone)]
pub struct ApplyOutcome {
    pub root: PathBuf,
    pub report: Report,
    pub journal: Option<PathBuf>,
    pub elapsed: Duration,
}

pub enum Event<'a> {
    Scan {
        stats: &'a ScanStats,
        elapsed: Duration,
    },
    Apply {
        current: usize,
        total: usize,
    },
}

pub type EventSink<'a> = &'a (dyn Fn(usize, Event<'_>) + Sync);

pub fn scan(
    roots: &[PathBuf],
    task: Task,
    options: &ScanOptions,
    cancel: &AtomicBool,
    on_event: EventSink<'_>,
) -> Vec<Result<ScanOutcome>> {
    per_volume(roots, |index| {
        scan_root(index, &roots[index], task, options, cancel, on_event)
    })
}

/// 扫描单个根目录。`index` 只用于标记进度事件属于哪个根目录。
pub fn scan_root(
    index: usize,
    root: &Path,
    task: Task,
    options: &ScanOptions,
    cancel: &AtomicBool,
    on_event: EventSink<'_>,
) -> Result<ScanOutcome> {
    let _span = info_span!("scan", root = %root.display(), task = task.name()).entered();
    if matches!(task, Task::Flatten(_)) && is_volume_root(root) {
        bail!("拉平不能直接作用于整个磁盘，请选择具体的文件夹");
    }

    let started = Instant::now();
    let tree = Tree::build(root, options, cancel, &mut |stats| {
        on_event(
            index,
            Event::Scan {
                stats,
                elapsed: started.elapsed(),
            },
        );
    });
    let tree = match tree {
        Ok(tree) => tree,
        Err(error) => {
            if is_cancelled(&error) {
                info!("scan cancelled");
            } else {
                warn!(error = %format!("{error:#}"), "scan failed");
            }
            return Err(error);
        }
    };

    let found = match task {
        Task::Empty => Found::Empty(empty::find(&tree)),
        Task::Flatten(mode) => Found::Flatten(flatten::plan(&tree, mode)),
    };
    let elapsed = started.elapsed();
    info!(
        dirs = tree.stats.dirs,
        files = tree.stats.files,
        skipped = tree.stats.skipped,
        errors = tree.stats.errors,
        found = found.len(),
        elapsed_ms = elapsed.as_millis() as u64,
        "scan finished"
    );

    Ok(ScanOutcome {
        root: root.to_path_buf(),
        task,
        stats: tree.stats,
        elapsed,
        found,
    })
}

pub fn apply(
    outcomes: &[&ScanOutcome],
    options: &ScanOptions,
    cancel: &AtomicBool,
    on_event: EventSink<'_>,
) -> Vec<ApplyOutcome> {
    let roots = outcomes
        .iter()
        .map(|outcome| outcome.root.clone())
        .collect::<Vec<_>>();
    per_volume(&roots, |index| {
        apply_root(index, outcomes[index], options, cancel, on_event)
    })
}

/// 执行单个根目录的扫描结果，并写入操作日志。
pub fn apply_root(
    index: usize,
    outcome: &ScanOutcome,
    options: &ScanOptions,
    cancel: &AtomicBool,
    on_event: EventSink<'_>,
) -> ApplyOutcome {
    let _span =
        info_span!("apply", root = %outcome.root.display(), task = outcome.task.name()).entered();
    let started = Instant::now();
    let mut last = Instant::now() - PROGRESS_INTERVAL;
    let mut on_progress = |current: usize, total: usize| {
        if current >= total || last.elapsed() >= PROGRESS_INTERVAL {
            last = Instant::now();
            on_event(index, Event::Apply { current, total });
        }
    };

    let report = match &outcome.found {
        Found::Empty(dirs) => empty::remove(dirs, cancel, &mut on_progress),
        Found::Flatten(plans) => flatten::apply(plans, options, cancel, &mut on_progress),
    };

    for failure in &report.failures {
        warn!(action = failure.action.as_str(), path = %failure.path.display(), error = %failure.error, "operation failed");
    }

    let journal =
        journal::write(&outcome.root, outcome.task.name(), &report).unwrap_or_else(|error| {
            warn!(error = %format!("{error:#}"), "failed to write journal");
            None
        });

    let elapsed = started.elapsed();
    info!(
        moved = report.moves.len(),
        removed_dirs = report.removed_dirs.len(),
        removed_files = report.removed_files.len(),
        failures = report.failures.len(),
        cancelled = report.cancelled,
        elapsed_ms = elapsed.as_millis() as u64,
        "apply finished"
    );

    ApplyOutcome {
        root: outcome.root.clone(),
        report,
        journal,
        elapsed,
    }
}

/// 按磁盘分组：每块磁盘一个线程，组内按原顺序依次执行。返回值顺序与 `roots` 一致。
pub fn per_volume<R, F>(roots: &[PathBuf], work: F) -> Vec<R>
where
    R: Send,
    F: Fn(usize) -> R + Sync,
{
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for (index, root) in roots.iter().enumerate() {
        let key = volume_key(root);
        let slot = *by_key.entry(key.clone()).or_insert_with(|| {
            groups.push((key, Vec::new()));
            groups.len() - 1
        });
        groups[slot].1.push(index);
    }

    let mut results: Vec<Option<R>> = (0..roots.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles = groups
            .iter()
            .map(|(key, indices)| {
                let work = &work;
                std::thread::Builder::new()
                    .name(format!("volume {key}"))
                    .spawn_scoped(scope, move || {
                        indices
                            .iter()
                            .map(|index| (*index, work(*index)))
                            .collect::<Vec<_>>()
                    })
                    .expect("failed to spawn worker thread")
            })
            .collect::<Vec<_>>();
        for handle in handles {
            for (index, result) in handle.join().expect("worker thread panicked") {
                results[index] = Some(result);
            }
        }
    });
    results
        .into_iter()
        .map(|result| result.expect("every root produces a result"))
        .collect()
}

/// 把结果展开成一行一项的文字，用于控制台输出和导出完整清单。
pub fn describe(found: &Found) -> Vec<String> {
    match found {
        Found::Empty(dirs) => dirs
            .iter()
            .map(|dir| dir.path.display().to_string())
            .collect(),
        Found::Flatten(plans) => plans
            .iter()
            .map(|plan| {
                format!(
                    "{} -> {}  ({} 个文件)",
                    plan.source_dir.display(),
                    plan.target_dir.display(),
                    plan.file_count
                )
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicBool;
    use tempfile::tempdir;

    #[test]
    fn per_volume_keeps_result_order() {
        let roots = (0..5)
            .map(|i| PathBuf::from(format!("dir{i}")))
            .collect::<Vec<_>>();
        assert_eq!(
            per_volume(&roots, |index| index * 10),
            vec![0, 10, 20, 30, 40]
        );
    }

    #[test]
    fn scans_and_applies_multiple_roots() {
        let temp = tempdir().unwrap();
        let a = temp.path().join("a");
        let b = temp.path().join("b");
        fs::create_dir_all(a.join("x").join("y")).unwrap();
        fs::create_dir_all(b.join("z")).unwrap();
        fs::write(b.join("keep.txt"), "keep").unwrap();
        let missing = temp.path().join("missing");

        let roots = vec![a.clone(), missing, b.clone()];
        let cancel = AtomicBool::new(false);
        let options = ScanOptions::default();
        let outcomes = scan(&roots, Task::Empty, &options, &cancel, &|_, _| {});

        assert_eq!(outcomes[0].as_ref().unwrap().found.len(), 2);
        assert!(outcomes[1].is_err());
        assert_eq!(outcomes[2].as_ref().unwrap().found.len(), 1);

        let ok = outcomes
            .iter()
            .filter_map(|o| o.as_ref().ok())
            .collect::<Vec<_>>();
        let journal_dir = temp.path().join("journals");
        // SAFETY: 单个测试内设置，测试只读取这个变量。
        unsafe { std::env::set_var("TIDYFS_JOURNAL_DIR", &journal_dir) };
        let applied = apply(&ok, &options, &cancel, &|_, _| {});

        assert_eq!(applied[0].report.removed_dirs.len(), 2);
        assert_eq!(applied[1].report.removed_dirs, vec![b.join("z")]);
        assert!(!a.join("x").exists());
        assert!(b.join("keep.txt").exists());
    }

    #[test]
    fn flatten_refuses_whole_volume() {
        let root = if cfg!(windows) {
            PathBuf::from(r"C:\")
        } else {
            PathBuf::from("/")
        };
        let outcomes = scan(
            &[root],
            Task::Flatten(FlattenMode::KeepEndpoints),
            &ScanOptions::default(),
            &AtomicBool::new(false),
            &|_, _| {},
        );
        assert!(outcomes[0].is_err());
    }
}
