use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::ops::{FlattenResult, MoveAction};

#[derive(Debug, Clone, Default)]
pub struct Journal {
    entries: Vec<JournalEntry>,
}

#[derive(Debug, Clone)]
enum JournalEntry {
    Move { from: PathBuf, to: PathBuf },
    RemoveFile { path: PathBuf },
    RemoveDir { path: PathBuf },
}

impl Journal {
    pub fn record_removed_dirs<I>(&mut self, paths: I)
    where
        I: IntoIterator<Item = PathBuf>,
    {
        self.entries.extend(
            paths
                .into_iter()
                .map(|path| JournalEntry::RemoveDir { path }),
        );
    }

    pub fn record_flatten_result(&mut self, result: &FlattenResult) {
        self.entries.extend(
            result
                .moves
                .iter()
                .cloned()
                .map(|MoveAction { from, to }| JournalEntry::Move { from, to }),
        );
        self.entries.extend(
            result
                .removed_files
                .iter()
                .cloned()
                .map(|path| JournalEntry::RemoveFile { path }),
        );
        self.entries.extend(
            result
                .removed_dirs
                .iter()
                .cloned()
                .map(|path| JournalEntry::RemoveDir { path }),
        );
    }

    pub fn record_flatten_results_ordered(&mut self, results: &[FlattenResult]) {
        for result in results {
            self.entries.extend(
                result
                    .moves
                    .iter()
                    .cloned()
                    .map(|MoveAction { from, to }| JournalEntry::Move { from, to }),
            );
        }
        for result in results {
            self.entries.extend(
                result
                    .removed_files
                    .iter()
                    .cloned()
                    .map(|path| JournalEntry::RemoveFile { path }),
            );
        }
        let mut removed_dirs = results
            .iter()
            .flat_map(|result| result.removed_dirs.iter().cloned())
            .collect::<Vec<_>>();
        removed_dirs.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        self.entries.extend(
            removed_dirs
                .into_iter()
                .map(|path| JournalEntry::RemoveDir { path }),
        );
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn write_to_root(&self, root: &Path) -> Result<Option<PathBuf>> {
        if self.entries.is_empty() {
            return Ok(None);
        }

        let dir = root.join(".tidyfs-journals");
        fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create journal directory {}", dir.display()))?;

        let timestamp = timestamp_for_filename();
        let path = dir.join(format!("tidyfs-{timestamp}.log"));
        fs::write(&path, self.render())
            .with_context(|| format!("failed to write journal {}", path.display()))?;

        Ok(Some(path))
    }

    fn render(&self) -> String {
        let mut lines = Vec::with_capacity(self.entries.len() + 2);
        lines.push("# tidyfs journal".to_string());
        lines.push(format!("created_at = {}", timestamp_for_log()));

        for entry in &self.entries {
            match entry {
                JournalEntry::Move { from, to } => {
                    lines.push(format!("MOVE\t{}\t{}", from.display(), to.display()));
                }
                JournalEntry::RemoveFile { path } => {
                    lines.push(format!("REMOVE_FILE\t{}", path.display()));
                }
                JournalEntry::RemoveDir { path } => {
                    lines.push(format!("REMOVE_DIR\t{}", path.display()));
                }
            }
        }

        lines.push(String::new());
        lines.join("\n")
    }
}

fn timestamp_for_filename() -> String {
    timestamp_for_log().replace(':', "-").replace('.', "-")
}

fn timestamp_for_log() -> String {
    OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown-time".to_string())
}
