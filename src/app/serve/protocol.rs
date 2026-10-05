//! 引擎和界面之间的 JSON Lines 协议。
//!
//! 界面 → 引擎：每行一个请求 `{"id": 1, "cmd": "scan", ...}`。
//! 引擎 → 界面：每行一个消息 `{"id": 1, "final": false, "type": "scan-progress", ...}`；
//! 一个请求的最后一条消息带 `"final": true`。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::flatten::FlattenMode;
use crate::domain::report::Report;
use crate::domain::tree::ScanStats;
use crate::engine::{Found, ScanOutcome};
use crate::infra::drives::DriveInfo;

/// 每个根目录在扫描结果里最多带多少条明细；完整清单通过 export 导出。
pub const PREVIEW_LIMIT: usize = 2000;
const FAILURE_LIMIT: usize = 200;

#[derive(Debug, Deserialize)]
pub struct Envelope {
    pub id: u64,
    #[serde(flatten)]
    pub request: Request,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TaskKind {
    Empty,
    Flatten,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "cmd",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Request {
    Hello,
    Drives,
    Pick,
    Scan {
        task: TaskKind,
        mode: Option<FlattenMode>,
        roots: Vec<PathBuf>,
    },
    Apply {
        scan: u64,
        roots: Vec<usize>,
    },
    Export {
        scan: u64,
        root: usize,
    },
    Cancel {
        job: u64,
    },
    Forget {
        scan: u64,
    },
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Message {
    Hello {
        version: &'static str,
        flatten_mode: &'static str,
        log_dir: String,
        journal_dir: String,
        config_path: Option<String>,
    },
    Drives {
        drives: Vec<DriveInfo>,
    },
    Picked {
        paths: Vec<String>,
    },
    ScanStarted {
        roots: Vec<String>,
    },
    ScanProgress {
        root: usize,
        stats: Stats,
        elapsed_ms: u64,
    },
    ScanRoot(RootScan),
    ScanDone {
        cancelled: bool,
    },
    ApplyStarted {
        roots: Vec<usize>,
    },
    ApplyProgress {
        root: usize,
        current: usize,
        total: usize,
    },
    ApplyRoot(RootApply),
    ApplyDone {
        cancelled: bool,
    },
    Exported {
        path: String,
    },
    Ok,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Stats {
    pub dirs: u64,
    pub files: u64,
    pub skipped: u64,
    pub errors: u64,
}

impl From<&ScanStats> for Stats {
    fn from(stats: &ScanStats) -> Self {
        Self {
            dirs: stats.dirs,
            files: stats.files,
            skipped: stats.skipped,
            errors: stats.errors,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootScan {
    pub root: usize,
    pub path: String,
    pub error: Option<String>,
    pub cancelled: bool,
    pub stats: Option<Stats>,
    pub elapsed_ms: u64,
    pub count: usize,
    pub ignored_files: usize,
    pub file_count: usize,
    pub items: Vec<Item>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Item {
    Empty {
        path: String,
    },
    Flatten {
        from: String,
        to: String,
        files: u32,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootApply {
    pub root: usize,
    pub path: String,
    pub moved: usize,
    pub removed_dirs: usize,
    pub removed_files: usize,
    pub failure_count: usize,
    pub failures: Vec<FailureItem>,
    pub journal: Option<String>,
    pub cancelled: bool,
    pub elapsed_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct FailureItem {
    pub action: &'static str,
    pub path: String,
    pub error: String,
}

pub fn text(path: &Path) -> String {
    path.display().to_string()
}

impl RootScan {
    pub fn from_outcome(root: usize, outcome: &ScanOutcome) -> Self {
        let items = match &outcome.found {
            Found::Empty(dirs) => dirs
                .iter()
                .take(PREVIEW_LIMIT)
                .map(|dir| Item::Empty {
                    path: text(&dir.path),
                })
                .collect(),
            Found::Flatten(plans) => plans
                .iter()
                .take(PREVIEW_LIMIT)
                .map(|plan| Item::Flatten {
                    from: text(&plan.source_dir),
                    to: text(&plan.target_dir),
                    files: plan.file_count,
                })
                .collect(),
        };
        Self {
            root,
            path: text(&outcome.root),
            error: None,
            cancelled: false,
            stats: Some((&outcome.stats).into()),
            elapsed_ms: outcome.elapsed.as_millis() as u64,
            count: outcome.found.len(),
            ignored_files: outcome.found.ignored_file_count(),
            file_count: outcome.found.file_count(),
            items,
        }
    }

    pub fn failed(root: usize, path: &Path, error: String, cancelled: bool) -> Self {
        Self {
            root,
            path: text(path),
            error: Some(error),
            cancelled,
            stats: None,
            elapsed_ms: 0,
            count: 0,
            ignored_files: 0,
            file_count: 0,
            items: Vec::new(),
        }
    }
}

impl RootApply {
    pub fn new(
        root: usize,
        path: &Path,
        report: &Report,
        journal: Option<&Path>,
        elapsed_ms: u64,
    ) -> Self {
        Self {
            root,
            path: text(path),
            moved: report.moves.len(),
            removed_dirs: report.removed_dirs.len(),
            removed_files: report.removed_files.len(),
            failure_count: report.failures.len(),
            failures: report
                .failures
                .iter()
                .take(FAILURE_LIMIT)
                .map(|failure| FailureItem {
                    action: failure.action.as_str(),
                    path: text(&failure.path),
                    error: failure.error.clone(),
                })
                .collect(),
            journal: journal.map(text),
            cancelled: report.cancelled,
            elapsed_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requests() {
        let envelope: Envelope = serde_json::from_str(
            r#"{"id":3,"cmd":"scan","task":"flatten","mode":"one-level","roots":["D:\\a"]}"#,
        )
        .unwrap();
        assert_eq!(envelope.id, 3);
        assert!(matches!(
            envelope.request,
            Request::Scan {
                task: TaskKind::Flatten,
                mode: Some(FlattenMode::OneLevel),
                ..
            }
        ));

        let envelope: Envelope =
            serde_json::from_str(r#"{"id":4,"cmd":"apply","scan":3,"roots":[0,1]}"#).unwrap();
        assert!(matches!(envelope.request, Request::Apply { scan: 3, .. }));
    }

    #[test]
    fn serializes_messages_with_type_tag() {
        let value = serde_json::to_value(Message::ScanProgress {
            root: 1,
            stats: Stats {
                dirs: 2,
                files: 3,
                skipped: 0,
                errors: 0,
            },
            elapsed_ms: 10,
        })
        .unwrap();
        assert_eq!(value["type"], "scan-progress");
        assert_eq!(value["elapsedMs"], 10);
        assert_eq!(value["stats"]["dirs"], 2);
    }
}
