//! 一次遍历把目录结构读进内存，后续的空目录分析、拉平分析都只在内存里做。
//!
//! 每个目录只调用一次 `read_dir`，并且只使用目录项自带的类型信息（Windows 上来自
//! FindNextFile，不需要额外的 stat），这是扫描速度的关键。

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::domain::options::ScanOptions;
use crate::infra::fsutil::{ensure_directory, is_volume_root};

pub const ROOT: u32 = 0;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("已取消")
    }
}

impl std::error::Error for Cancelled {}

pub fn is_cancelled(error: &anyhow::Error) -> bool {
    error.is::<Cancelled>()
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: OsString,
    pub parent: u32,
    pub children: Vec<u32>,
    /// 普通文件数量（不含忽略名单里的噪音文件）。
    pub files: u32,
    /// 噪音文件（desktop.ini 等，以及默认情况下的隐藏文件）的文件名。
    pub ignored: Vec<OsString>,
    /// 目录里有我们不碰的东西：符号链接、Junction、被跳过的系统目录、读不了的内容。
    /// 这样的目录永远不会被删除，也不会被拉平。
    pub opaque: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanStats {
    pub dirs: u64,
    pub files: u64,
    pub skipped: u64,
    pub errors: u64,
}

/// 节点按发现顺序编号：父节点的编号总是小于子节点。
/// 所以正序遍历 = 先祖先后后代，倒序遍历 = 先后代后祖先。
#[derive(Debug, Clone)]
pub struct Tree {
    pub root: PathBuf,
    pub nodes: Vec<Node>,
    pub stats: ScanStats,
}

impl Tree {
    pub fn build(
        root: &Path,
        options: &ScanOptions,
        cancel: &AtomicBool,
        on_progress: &mut dyn FnMut(&ScanStats),
    ) -> Result<Tree> {
        ensure_directory(root)?;

        let mut tree = Tree {
            root: root.to_path_buf(),
            nodes: vec![Node::new(root.as_os_str().to_os_string(), u32::MAX)],
            stats: ScanStats::default(),
        };
        let root_is_volume = is_volume_root(root);
        let mut stack = vec![(ROOT, root.to_path_buf())];
        let mut last_report = Instant::now();
        let mut child_dirs = Vec::new();

        while let Some((index, dir)) = stack.pop() {
            if cancel.load(Ordering::Relaxed) {
                return Err(Cancelled.into());
            }
            tree.stats.dirs += 1;

            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(_) => {
                    tree.stats.errors += 1;
                    tree.nodes[index as usize].opaque = true;
                    continue;
                }
            };

            let parent_is_volume = index == ROOT && root_is_volume;
            let mut files = 0u32;
            let mut ignored = Vec::new();
            let mut opaque = false;

            for entry in entries {
                let Ok(entry) = entry else {
                    tree.stats.errors += 1;
                    opaque = true;
                    continue;
                };
                let Ok(file_type) = entry.file_type() else {
                    tree.stats.errors += 1;
                    opaque = true;
                    continue;
                };
                let name = entry.file_name();

                if file_type.is_dir() {
                    if options.is_skipped_dir(&name, parent_is_volume) || is_os_protected(&entry) {
                        tree.stats.skipped += 1;
                        opaque = true;
                    } else {
                        child_dirs.push(name);
                    }
                } else if file_type.is_file() {
                    tree.stats.files += 1;
                    if options.is_ignored_file(&name)
                        || (options.hidden_files_are_noise && is_hidden_file(&entry, &name))
                    {
                        ignored.push(name);
                    } else {
                        files += 1;
                    }
                } else {
                    // 符号链接、Junction 等：保留原样，永远不跟进去。
                    tree.stats.files += 1;
                    opaque = true;
                }
            }

            child_dirs.sort();
            ignored.sort();

            let first = tree.nodes.len() as u32;
            tree.nodes
                .extend(child_dirs.drain(..).map(|name| Node::new(name, index)));
            let end = tree.nodes.len() as u32;

            let node = &mut tree.nodes[index as usize];
            node.children = (first..end).collect();
            node.files = files;
            node.ignored = ignored;
            node.opaque |= opaque;

            // 倒序入栈，让第一个子目录最先被处理。
            for child in (first..end).rev() {
                let path = dir.join(&tree.nodes[child as usize].name);
                stack.push((child, path));
            }

            if last_report.elapsed() >= PROGRESS_INTERVAL {
                last_report = Instant::now();
                on_progress(&tree.stats);
            }
        }

        on_progress(&tree.stats);
        Ok(tree)
    }

    pub fn node(&self, index: u32) -> &Node {
        &self.nodes[index as usize]
    }

    pub fn path(&self, index: u32) -> PathBuf {
        let mut names = Vec::new();
        let mut current = index;
        while current != ROOT {
            let node = self.node(current);
            names.push(&node.name);
            current = node.parent;
        }
        let mut path = self.root.clone();
        path.extend(names.into_iter().rev());
        path
    }

    pub fn name(&self, index: u32) -> Option<&str> {
        if index == ROOT {
            self.root.file_name().and_then(|name| name.to_str())
        } else {
            self.node(index).name.to_str()
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

impl Node {
    fn new(name: OsString, parent: u32) -> Self {
        Self {
            name,
            parent,
            children: Vec::new(),
            files: 0,
            ignored: Vec::new(),
            opaque: false,
        }
    }
}

/// 同时带“隐藏 + 系统”属性的目录是 Windows 受保护的系统目录，不进入。
/// Windows 上 `DirEntry::metadata` 直接来自目录枚举数据，不会产生额外的系统调用。
#[cfg(windows)]
fn is_os_protected(entry: &fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    entry
        .metadata()
        .is_ok_and(|metadata| metadata.file_attributes() & (HIDDEN | SYSTEM) == HIDDEN | SYSTEM)
}

/// Windows 上看“隐藏”属性，其它系统看是不是以点开头。
#[cfg(windows)]
fn is_hidden_file(entry: &fs::DirEntry, _name: &std::ffi::OsStr) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    entry
        .metadata()
        .is_ok_and(|metadata| metadata.file_attributes() & HIDDEN != 0)
}

#[cfg(not(windows))]
fn is_hidden_file(_entry: &fs::DirEntry, name: &std::ffi::OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

#[cfg(not(windows))]
fn is_os_protected(_entry: &fs::DirEntry) -> bool {
    false
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use tempfile::tempdir;

    pub fn build(root: &Path, options: &ScanOptions) -> Tree {
        Tree::build(root, options, &AtomicBool::new(false), &mut |_| {}).unwrap()
    }

    #[test]
    fn builds_tree_with_counts_and_paths() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("a").join("b")).unwrap();
        fs::write(root.join("a").join("x.txt"), "x").unwrap();
        fs::write(root.join("a").join("desktop.ini"), "noise").unwrap();

        let tree = build(root, &ScanOptions::default());
        assert_eq!(tree.len(), 3);
        assert_eq!(tree.path(1), root.join("a"));
        assert_eq!(tree.path(2), root.join("a").join("b"));
        assert_eq!(tree.node(1).files, 1);
        assert_eq!(tree.node(1).ignored, vec![OsString::from("desktop.ini")]);
        assert_eq!(tree.stats.dirs, 3);
        assert_eq!(tree.stats.files, 2);
    }

    #[test]
    fn skipped_dirs_are_opaque_and_not_traversed() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("repo").join(".git").join("refs")).unwrap();

        let tree = build(root, &ScanOptions::default());
        assert_eq!(tree.len(), 2);
        assert!(tree.node(1).opaque);
        assert_eq!(tree.stats.skipped, 1);
    }

    /// 把文件设成隐藏：Windows 用 attrib +h，其它系统本来就用点开头的文件名。
    pub fn make_hidden(path: &Path) {
        #[cfg(windows)]
        {
            let status = std::process::Command::new("attrib")
                .arg("+h")
                .arg(path)
                .status()
                .unwrap();
            assert!(status.success());
        }
        #[cfg(not(windows))]
        let _ = path;
    }

    #[test]
    fn hidden_files_are_noise_by_default() {
        let temp = tempdir().unwrap();
        let dir = temp.path().join("a");
        fs::create_dir_all(&dir).unwrap();
        let hidden = dir.join(".hidden-note");
        fs::write(&hidden, "x").unwrap();
        make_hidden(&hidden);

        let tree = build(temp.path(), &ScanOptions::default());
        assert_eq!(tree.node(1).files, 0);
        assert_eq!(tree.node(1).ignored, vec![OsString::from(".hidden-note")]);

        let mut options = ScanOptions::default();
        options.hidden_files_are_noise = false;
        let tree = build(temp.path(), &options);
        assert_eq!(tree.node(1).files, 1);
    }

    #[test]
    fn cancel_stops_the_scan() {
        let temp = tempdir().unwrap();
        let error = Tree::build(
            temp.path(),
            &ScanOptions::default(),
            &AtomicBool::new(true),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(is_cancelled(&error));
    }
}
