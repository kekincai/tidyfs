use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::domain::report::{Action, Report};
use crate::domain::tree::Tree;
use crate::infra::fsutil::{remove_dir_allow_readonly, remove_file_allow_readonly};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyDir {
    pub path: PathBuf,
    pub ignored_files: Vec<PathBuf>,
}

/// 找出所有空目录：没有普通文件，子目录也全部是空目录。结果按“先深后浅”排列，可以直接按顺序删除。
/// 扫描根目录本身永远不会出现在结果里。
pub fn find(tree: &Tree) -> Vec<EmptyDir> {
    let mut empty = vec![false; tree.len()];
    let mut found = Vec::new();

    for index in (1..tree.len()).rev() {
        let node = &tree.nodes[index];
        if node.files == 0
            && !node.opaque
            && node.children.iter().all(|child| empty[*child as usize])
        {
            empty[index] = true;
            let path = tree.path(index as u32);
            found.push(EmptyDir {
                ignored_files: node.ignored.iter().map(|name| path.join(name)).collect(),
                path,
            });
        }
    }

    found
}

/// 依次删除空目录。单个失败（权限、占用等）只记录，不中断后面的目录。
pub fn remove(
    dirs: &[EmptyDir],
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(usize, usize),
) -> Report {
    let mut report = Report::default();
    let total = dirs.len();

    for (index, dir) in dirs.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }

        let mut blocked = false;
        for file in &dir.ignored_files {
            match remove_file_allow_readonly(file) {
                Ok(()) => report.removed_files.push(file.clone()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    blocked = true;
                    report.fail(Action::RemoveFile, file, error);
                }
            }
        }

        if !blocked {
            match remove_dir_allow_readonly(&dir.path) {
                Ok(()) => report.removed_dirs.push(dir.path.clone()),
                Err(error) => report.fail(Action::RemoveDir, &dir.path, error),
            }
        }

        on_progress(index + 1, total);
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::options::ScanOptions;
    use crate::domain::tree::tests::build;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn collects_empty_dirs_from_deep_to_shallow() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let abc = root.join("a").join("b").join("c");
        let keep_sub = root.join("keep").join("sub");
        fs::create_dir_all(&abc).unwrap();
        fs::create_dir_all(&keep_sub).unwrap();
        fs::write(keep_sub.join("file.txt"), "hello").unwrap();

        let paths = find(&build(root, &ScanOptions::default()))
            .into_iter()
            .map(|item| item.path)
            .collect::<Vec<_>>();

        assert_eq!(paths, vec![abc, root.join("a").join("b"), root.join("a")]);
    }

    #[test]
    fn ignored_junk_files_still_count_as_empty() {
        let temp = tempdir().unwrap();
        let folder = temp.path().join("album");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("desktop.ini"), "noise").unwrap();

        let found = find(&build(temp.path(), &ScanOptions::default()));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].ignored_files, vec![folder.join("desktop.ini")]);

        let report = remove(&found, &AtomicBool::new(false), &mut |_, _| {});
        assert_eq!(report.removed_files, vec![folder.join("desktop.ini")]);
        assert!(!folder.exists());
    }

    #[test]
    fn dir_with_only_hidden_files_is_empty_and_removed() {
        let temp = tempdir().unwrap();
        let folder = temp.path().join("only-hidden");
        fs::create_dir_all(&folder).unwrap();
        let hidden = folder.join(".cache-marker");
        fs::write(&hidden, "x").unwrap();
        crate::domain::tree::tests::make_hidden(&hidden);

        let found = find(&build(temp.path(), &ScanOptions::default()));
        assert_eq!(found.len(), 1);
        let report = remove(&found, &AtomicBool::new(false), &mut |_, _| {});
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert!(!folder.exists());
    }

    #[test]
    fn parent_of_skipped_dir_is_not_empty() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("project").join(".git").join("refs")).unwrap();
        fs::create_dir_all(temp.path().join("project").join("empty")).unwrap();

        let found = find(&build(temp.path(), &ScanOptions::default()));
        assert_eq!(
            found.into_iter().map(|item| item.path).collect::<Vec<_>>(),
            vec![temp.path().join("project").join("empty")]
        );
    }

    #[test]
    fn remove_reports_failures_and_keeps_going() {
        let temp = tempdir().unwrap();
        let not_empty = temp.path().join("not_empty");
        let empty = temp.path().join("empty");
        fs::create_dir_all(&not_empty).unwrap();
        fs::create_dir_all(&empty).unwrap();
        fs::write(not_empty.join("file.txt"), "still here").unwrap();

        let dirs = [
            EmptyDir {
                path: not_empty.clone(),
                ignored_files: Vec::new(),
            },
            EmptyDir {
                path: empty.clone(),
                ignored_files: Vec::new(),
            },
        ];
        let report = remove(&dirs, &AtomicBool::new(false), &mut |_, _| {});

        assert_eq!(report.removed_dirs, vec![empty.clone()]);
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].path, not_empty);
        assert_eq!(report.failures[0].action, Action::RemoveDir);
        assert!(!empty.exists());
    }
}
