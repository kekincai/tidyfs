use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;

use crate::domain::options::ScanOptions;
use crate::domain::report::{Action, MoveAction, Report};
use crate::domain::tree::{ROOT, Tree};
use crate::infra::fsutil::{
    is_dir_empty, remove_dir_allow_readonly, remove_file_allow_readonly, unique_destination,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum FlattenMode {
    /// 按日期目录分组，保留日期目录内第一层，去掉更深的壳目录。
    #[default]
    KeepEndpoints,
    /// 只把最内层的文件提升一层。
    OneLevel,
    /// 一路压平到单链的起点。
    CollapseChain,
}

impl FlattenMode {
    pub fn as_str(self) -> &'static str {
        match self {
            FlattenMode::KeepEndpoints => "keep-endpoints",
            FlattenMode::OneLevel => "one-level",
            FlattenMode::CollapseChain => "collapse-chain",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlattenPlan {
    /// 被保留的那一层目录。
    pub root: PathBuf,
    /// 文件当前所在的最内层目录。
    pub source_dir: PathBuf,
    /// 文件要移动到的目录。
    pub target_dir: PathBuf,
    pub file_count: u32,
    /// 被删除的壳目录里的噪音文件。
    pub ignored_files: Vec<PathBuf>,
    /// 移动完成后尝试删除的壳目录，先深后浅。
    pub removed_dirs: Vec<PathBuf>,
}

pub fn plan(tree: &Tree, mode: FlattenMode) -> Vec<FlattenPlan> {
    match mode {
        FlattenMode::KeepEndpoints => plan_keep_endpoints(tree),
        FlattenMode::OneLevel | FlattenMode::CollapseChain => plan_single_chains(tree, mode),
    }
}

/// 估算执行步骤数，用于进度条。
pub fn step_count(plans: &[FlattenPlan]) -> usize {
    plans
        .iter()
        .map(|plan| plan.file_count as usize + plan.ignored_files.len() + plan.removed_dirs.len())
        .sum()
}

fn plan_keep_endpoints(tree: &Tree) -> Vec<FlattenPlan> {
    let mut plans = Vec::new();

    if tree.name(ROOT).is_some_and(is_date_name) {
        // 直接选中了日期目录：它的每个子目录就是要保留的第一层。
        for first_level in &tree.node(ROOT).children {
            collect_leaves_under(tree, *first_level, &mut plans);
        }
    } else {
        let has_date_child = tree
            .node(ROOT)
            .children
            .iter()
            .any(|child| tree.name(*child).is_some_and(is_date_name));
        if !has_date_child {
            // 选中的目录本身就是“第一层”。
            collect_leaves_under(tree, ROOT, &mut plans);
        }
        if plans.is_empty() {
            // 选中目录下面是一组日期目录。
            for anchor in &tree.node(ROOT).children {
                for first_level in &tree.node(*anchor).children {
                    collect_leaves_under(tree, *first_level, &mut plans);
                }
            }
        }
        if plans.is_empty() {
            for first_level in &tree.node(ROOT).children {
                collect_leaves_under(tree, *first_level, &mut plans);
            }
        }
    }

    preserve_parallel_leaf_dirs(&mut plans);
    plans.retain(|plan| plan.target_dir != plan.source_dir);
    plans
}

fn collect_leaves_under(tree: &Tree, keep: u32, plans: &mut Vec<FlattenPlan>) {
    let mut chain = vec![keep];
    for child in &tree.node(keep).children {
        chain.push(*child);
        collect_leaves(tree, &mut chain, plans);
        chain.pop();
    }
}

/// 沿着没有普通文件的壳目录往下走，直到遇到“有文件且没有子目录”的最内层目录。
fn collect_leaves(tree: &Tree, chain: &mut Vec<u32>, plans: &mut Vec<FlattenPlan>) {
    let current = *chain.last().expect("chain is never empty");
    let node = tree.node(current);
    if node.opaque {
        return;
    }
    if node.files > 0 {
        if node.children.is_empty() {
            let target = tree.path(chain[0]);
            plans.push(make_plan(tree, chain, target));
        }
        return;
    }
    for child in &node.children {
        chain.push(*child);
        collect_leaves(tree, chain, plans);
        chain.pop();
    }
}

/// 同一个壳目录下有多个末端目录时，保留末端目录名，避免把不同来源的文件混在一起。
fn preserve_parallel_leaf_dirs(plans: &mut [FlattenPlan]) {
    let mut sibling_counts: HashMap<PathBuf, usize> = HashMap::new();
    for plan in plans.iter() {
        if let Some(parent) = plan.source_dir.parent() {
            *sibling_counts.entry(parent.to_path_buf()).or_default() += 1;
        }
    }

    for plan in plans {
        let shared = plan
            .source_dir
            .parent()
            .is_some_and(|parent| sibling_counts.get(parent).copied().unwrap_or(0) >= 2);
        if let (true, Some(leaf_name)) = (shared, plan.source_dir.file_name()) {
            plan.target_dir = plan.root.join(leaf_name);
        }
    }
}

fn plan_single_chains(tree: &Tree, mode: FlattenMode) -> Vec<FlattenPlan> {
    let mut covered = vec![false; tree.len()];
    let mut plans = Vec::new();

    // 编号正序 = 祖先先于后代，所以总是先找到最长的链。
    for start in 1..tree.len() as u32 {
        if covered[start as usize] {
            continue;
        }
        let Some(chain) = single_chain_from(tree, start) else {
            continue;
        };
        for index in &chain {
            covered[*index as usize] = true;
        }
        let plan = match mode {
            FlattenMode::OneLevel => {
                let parent = chain[chain.len() - 2];
                let deepest = chain[chain.len() - 1];
                let mut plan = make_plan(tree, &[parent, deepest], tree.path(parent));
                plan.root = tree.path(start);
                plan
            }
            _ => make_plan(tree, &chain, tree.path(start)),
        };
        plans.push(plan);
    }

    plans
}

/// `start` 起每一层都只有一个子目录且没有普通文件，最内层有文件且没有子目录。
fn single_chain_from(tree: &Tree, start: u32) -> Option<Vec<u32>> {
    let mut chain = vec![start];
    let mut current = start;
    loop {
        let node = tree.node(current);
        if node.files > 0 || node.opaque || node.children.len() != 1 {
            return None;
        }
        current = node.children[0];
        chain.push(current);

        let next = tree.node(current);
        if next.opaque {
            return None;
        }
        if next.files > 0 {
            return next.children.is_empty().then_some(chain);
        }
    }
}

/// `chain[0]` 被保留，`chain[1..]` 是要删除的目录，最后一个是文件所在目录。
fn make_plan(tree: &Tree, chain: &[u32], target_dir: PathBuf) -> FlattenPlan {
    let deepest = *chain.last().expect("chain has a leaf");
    let mut ignored_files = Vec::new();
    let mut removed_dirs = Vec::new();
    for index in chain[1..].iter().rev() {
        let path = tree.path(*index);
        ignored_files.extend(tree.node(*index).ignored.iter().map(|name| path.join(name)));
        removed_dirs.push(path);
    }

    FlattenPlan {
        root: tree.path(chain[0]),
        source_dir: tree.path(deepest),
        target_dir,
        file_count: tree.node(deepest).files,
        ignored_files,
        removed_dirs,
    }
}

fn is_date_name(name: &str) -> bool {
    name.len() == 8 && name.starts_with("20") && name.bytes().all(|byte| byte.is_ascii_digit())
}

/// 执行拉平。单个文件失败只记录，不中断其它计划；某个计划有文件没移走时，不会删除它的噪音文件。
/// 所有计划处理完后，统一从深到浅删除已经空掉的壳目录。
pub fn apply(
    plans: &[FlattenPlan],
    options: &ScanOptions,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(usize, usize),
) -> Report {
    let mut report = Report::default();
    let total = step_count(plans);
    let mut done = 0usize;
    let mut tick = || {
        done += 1;
        on_progress(done.min(total), total);
    };

    for plan in plans {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            return report;
        }

        let moved_all = move_files(plan, options, &mut report, &mut tick);
        for file in &plan.ignored_files {
            if moved_all {
                match remove_file_allow_readonly(file) {
                    Ok(()) => report.removed_files.push(file.clone()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => report.fail(Action::RemoveFile, file, error),
                }
            }
            tick();
        }
    }

    let mut dirs = plans
        .iter()
        .flat_map(|plan| plan.removed_dirs.iter().cloned())
        .collect::<Vec<_>>();
    dirs.sort_by(|a, b| {
        b.components()
            .count()
            .cmp(&a.components().count())
            .then_with(|| a.cmp(b))
    });
    dirs.dedup();

    for dir in dirs {
        if is_dir_empty(&dir).unwrap_or(false) {
            match remove_dir_allow_readonly(&dir) {
                Ok(()) => report.removed_dirs.push(dir),
                Err(error) => report.fail(Action::RemoveDir, dir, error),
            }
        }
        tick();
    }

    report
}

fn move_files(
    plan: &FlattenPlan,
    options: &ScanOptions,
    report: &mut Report,
    tick: &mut dyn FnMut(),
) -> bool {
    if let Err(error) = fs::create_dir_all(&plan.target_dir) {
        report.fail(Action::CreateDir, &plan.target_dir, error);
        return false;
    }

    let files = match list_regular_files(&plan.source_dir, options) {
        Ok(files) => files,
        Err(error) => {
            report.fail(Action::Move, &plan.source_dir, error);
            return false;
        }
    };

    let mut moved_all = true;
    for name in files {
        let source = plan.source_dir.join(&name);
        let destination = unique_destination(&plan.target_dir, &name);
        match fs::rename(&source, &destination) {
            Ok(()) => report.moves.push(MoveAction {
                from: source,
                to: destination,
            }),
            Err(error) => {
                moved_all = false;
                report.fail(Action::Move, source, error);
            }
        }
        tick();
    }
    moved_all
}

/// 最内层目录里要移动的文件。隐藏文件也一起移动（不删），只有忽略名单里的噪音文件留下删除。
fn list_regular_files(dir: &Path, options: &ScanOptions) -> std::io::Result<Vec<OsString>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if entry.file_type()?.is_file() && !options.is_ignored_file(&name) {
            files.push(name);
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tree::tests::build;
    use tempfile::tempdir;

    fn plans_for(root: &Path, mode: FlattenMode) -> Vec<FlattenPlan> {
        let mut plans = plan(&build(root, &ScanOptions::default()), mode);
        plans.sort_by(|a, b| a.source_dir.cmp(&b.source_dir));
        plans
    }

    fn run(plans: &[FlattenPlan]) -> Report {
        apply(
            plans,
            &ScanOptions::default(),
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
    }

    #[test]
    fn collapse_chain_detects_single_chain_that_ends_with_files() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let outer = root.join("outer");
        let deep = outer.join("inner").join("deep");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("x.txt"), "x").unwrap();
        fs::write(deep.join("y.txt"), "y").unwrap();

        let plans = plans_for(root, FlattenMode::CollapseChain);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].root, outer);
        assert_eq!(plans[0].target_dir, outer);
        assert_eq!(plans[0].source_dir, deep);
        assert_eq!(plans[0].file_count, 2);
        assert_eq!(
            plans[0].removed_dirs,
            vec![deep.clone(), outer.join("inner")]
        );

        let report = run(&plans);
        assert_eq!(report.moves.len(), 2);
        assert!(outer.join("x.txt").exists());
        assert!(!outer.join("inner").exists());
    }

    #[test]
    fn skips_chain_when_intermediate_dir_contains_file() {
        let temp = tempdir().unwrap();
        let inner = temp.path().join("outer").join("inner");
        fs::create_dir_all(&inner).unwrap();
        fs::write(temp.path().join("outer").join("note.txt"), "keep").unwrap();
        fs::write(inner.join("x.txt"), "x").unwrap();

        assert!(plans_for(temp.path(), FlattenMode::CollapseChain).is_empty());
    }

    #[test]
    fn one_level_mode_moves_to_parent_only() {
        let temp = tempdir().unwrap();
        let outer = temp.path().join("outer");
        let deep = outer.join("inner").join("deep");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("x.txt"), "x").unwrap();

        let plans = plans_for(temp.path(), FlattenMode::OneLevel);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].root, outer);
        assert_eq!(plans[0].target_dir, outer.join("inner"));
        assert_eq!(plans[0].removed_dirs, vec![deep]);
    }

    #[test]
    fn one_level_removes_ignored_files_in_deepest_dir() {
        let temp = tempdir().unwrap();
        let outer = temp.path().join("outer");
        let deep = outer.join("inner");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("x.txt"), "x").unwrap();
        fs::write(deep.join("desktop.ini"), "noise").unwrap();

        let report = run(&plans_for(temp.path(), FlattenMode::OneLevel));
        assert_eq!(report.removed_files, vec![deep.join("desktop.ini")]);
        assert!(outer.join("x.txt").exists());
        assert!(!deep.exists());
    }

    #[test]
    fn keep_endpoints_keeps_first_folder_inside_date_anchor() {
        let temp = tempdir().unwrap();
        let anchor = temp.path().join("20250203").join("a");
        let deep = anchor.join("b").join("c");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("x.txt"), "x").unwrap();

        let plans = plans_for(temp.path(), FlattenMode::KeepEndpoints);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].root, anchor);
        assert_eq!(plans[0].source_dir, deep);
        assert_eq!(plans[0].target_dir, anchor);
        assert_eq!(plans[0].removed_dirs, vec![deep, anchor.join("b")]);
    }

    #[test]
    fn keep_endpoints_removes_shared_middle_folder_for_multiple_leaves() {
        let temp = tempdir().unwrap();
        let anchor = temp.path().join("20250203").join("a");
        let c = anchor.join("keep").join("b").join("c");
        let d = anchor.join("keep").join("b").join("d");
        fs::create_dir_all(&c).unwrap();
        fs::create_dir_all(&d).unwrap();
        fs::write(c.join("c.txt"), "c").unwrap();
        fs::write(d.join("d.txt"), "d").unwrap();

        let plans = plans_for(temp.path(), FlattenMode::KeepEndpoints);
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].target_dir, anchor.join("c"));
        assert_eq!(plans[1].target_dir, anchor.join("d"));

        // 分开执行也必须正确：第一次执行时共享的壳目录还不空，不能删。
        run(&plans[..1]);
        run(&plans[1..]);
        assert!(anchor.join("c").join("c.txt").exists());
        assert!(anchor.join("d").join("d.txt").exists());
        assert!(!anchor.join("keep").exists());
    }

    #[test]
    fn keep_endpoints_removes_all_middle_layers() {
        let temp = tempdir().unwrap();
        let anchor = temp.path().join("20250203").join("a");
        let deep = anchor.join("keep").join("b").join("x").join("y").join("c");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("photo.jpg"), "photo").unwrap();

        let plans = plans_for(temp.path(), FlattenMode::KeepEndpoints);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].target_dir, anchor);

        run(&plans);
        assert!(anchor.join("photo.jpg").exists());
        assert!(!anchor.join("keep").exists());
    }

    #[test]
    fn keep_endpoints_keeps_first_folder_and_removes_title_shell() {
        let temp = tempdir().unwrap();
        let date = temp.path().join("20250203");
        let first = date.join("project_alpha");
        fs::create_dir_all(first.join("archive_shell")).unwrap();
        fs::write(date.join("already-here.txt"), "keep").unwrap();
        fs::write(first.join("archive_shell").join("video.mp4"), "video").unwrap();

        let plans = plans_for(temp.path(), FlattenMode::KeepEndpoints);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].root, first);
        assert_eq!(plans[0].target_dir, first);

        run(&plans);
        assert!(date.join("already-here.txt").exists());
        assert!(first.join("video.mp4").exists());
        assert!(!first.join("archive_shell").exists());
    }

    #[test]
    fn keep_endpoints_can_scan_selected_date_folder_directly() {
        let temp = tempdir().unwrap();
        let date = temp.path().join("20250203");
        let first = date.join("project_alpha");
        let leaf = first.join("long_title_shell").join("p (1)");
        fs::create_dir_all(&leaf).unwrap();
        fs::write(leaf.join("image.jpg"), "image").unwrap();

        let plans = plans_for(&date, FlattenMode::KeepEndpoints);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].root, first);
        assert_eq!(plans[0].target_dir, first);

        run(&plans);
        assert!(first.join("image.jpg").exists());
        assert!(!first.join("long_title_shell").exists());
    }

    #[test]
    fn keep_endpoints_can_scan_selected_first_level_folder_directly() {
        let temp = tempdir().unwrap();
        let selected = temp.path().join("project_alpha");
        let view = selected.join("project_alpha_copy").join("578").join("view");
        fs::create_dir_all(view.join("v")).unwrap();
        fs::create_dir_all(view.join("p")).unwrap();
        fs::write(view.join("v").join("v.mp4"), "v").unwrap();
        fs::write(view.join("p").join("p.jpg"), "p").unwrap();

        let plans = plans_for(&selected, FlattenMode::KeepEndpoints);
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].target_dir, selected.join("p"));
        assert_eq!(plans[1].target_dir, selected.join("v"));

        run(&plans);
        assert!(selected.join("v").join("v.mp4").exists());
        assert!(selected.join("p").join("p.jpg").exists());
        assert!(!selected.join("project_alpha_copy").exists());
    }

    #[test]
    fn keep_endpoints_reuses_existing_parallel_leaf_folder() {
        let temp = tempdir().unwrap();
        let selected = temp.path().join("project_alpha");
        let view = selected.join("project_alpha_copy").join("578").join("view");
        fs::create_dir_all(view.join("p")).unwrap();
        fs::create_dir_all(view.join("v")).unwrap();
        fs::create_dir_all(selected.join("p")).unwrap();
        fs::write(view.join("p").join("p.jpg"), "p").unwrap();
        fs::write(view.join("v").join("v.mp4"), "v").unwrap();

        let plans = plans_for(&selected, FlattenMode::KeepEndpoints);
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].target_dir, selected.join("p"));
        assert_eq!(plans[1].target_dir, selected.join("v"));

        run(&plans);
        assert!(selected.join("p").join("p.jpg").exists());
        assert!(selected.join("v").join("v.mp4").exists());
        assert!(!selected.join("p (1)").exists());
    }

    #[test]
    fn keep_endpoints_skips_already_flat_parallel_leaf_folders() {
        let temp = tempdir().unwrap();
        let first = temp.path().join("20250203").join("project_alpha");
        fs::create_dir_all(first.join("p")).unwrap();
        fs::create_dir_all(first.join("v")).unwrap();
        fs::write(first.join("p").join("p.jpg"), "p").unwrap();
        fs::write(first.join("v").join("v.mp4"), "v").unwrap();

        assert!(plans_for(&temp.path().join("20250203"), FlattenMode::KeepEndpoints).is_empty());
    }

    #[test]
    fn keep_endpoints_preserves_copied_parallel_leaf_folder_names() {
        let temp = tempdir().unwrap();
        let date = temp.path().join("20250203");
        let first = date.join("project_alpha");
        let shell = first.join("long_title_shell");
        fs::create_dir_all(shell.join("p (1)")).unwrap();
        fs::create_dir_all(shell.join("v (1)")).unwrap();
        fs::write(shell.join("p (1)").join("p.jpg"), "p").unwrap();
        fs::write(shell.join("v (1)").join("v.mp4"), "v").unwrap();

        let plans = plans_for(&date, FlattenMode::KeepEndpoints);
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].target_dir, first.join("p (1)"));
        assert_eq!(plans[1].target_dir, first.join("v (1)"));

        run(&plans);
        assert!(first.join("p (1)").join("p.jpg").exists());
        assert!(first.join("v (1)").join("v.mp4").exists());
        assert!(!shell.exists());
    }

    #[test]
    fn keep_endpoints_does_not_touch_ignored_files_of_kept_folder() {
        let temp = tempdir().unwrap();
        let first = temp.path().join("20250203").join("album");
        fs::create_dir_all(first.join("shell")).unwrap();
        fs::write(first.join("desktop.ini"), "folder icon").unwrap();
        fs::write(first.join("shell").join("desktop.ini"), "noise").unwrap();
        fs::write(first.join("shell").join("a.jpg"), "a").unwrap();

        let report = run(&plans_for(temp.path(), FlattenMode::KeepEndpoints));
        assert_eq!(
            report.removed_files,
            vec![first.join("shell").join("desktop.ini")]
        );
        assert!(first.join("desktop.ini").exists());
        assert!(first.join("a.jpg").exists());
        assert!(!first.join("shell").exists());
    }

    #[test]
    fn hidden_files_in_leaf_are_moved_not_deleted() {
        let temp = tempdir().unwrap();
        let first = temp.path().join("20250203").join("album");
        let shell = first.join("shell");
        fs::create_dir_all(&shell).unwrap();
        fs::write(shell.join("a.jpg"), "a").unwrap();
        fs::write(shell.join(".meta"), "keep me").unwrap();
        crate::domain::tree::tests::make_hidden(&shell.join(".meta"));

        run(&plans_for(temp.path(), FlattenMode::KeepEndpoints));
        assert!(first.join("a.jpg").exists());
        assert_eq!(fs::read_to_string(first.join(".meta")).unwrap(), "keep me");
        assert!(!shell.exists());
    }

    #[test]
    fn name_conflicts_are_renamed_instead_of_overwritten() {
        let temp = tempdir().unwrap();
        let first = temp.path().join("20250203").join("album");
        fs::create_dir_all(first.join("shell")).unwrap();
        fs::write(first.join("shell").join("a.jpg"), "new").unwrap();

        let plans = plans_for(temp.path(), FlattenMode::KeepEndpoints);
        fs::write(first.join("a.jpg"), "old").unwrap();
        run(&plans);

        assert_eq!(fs::read_to_string(first.join("a.jpg")).unwrap(), "old");
        assert_eq!(fs::read_to_string(first.join("a (1).jpg")).unwrap(), "new");
    }
}
