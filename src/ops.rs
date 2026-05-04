use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyDirCandidate {
    pub path: PathBuf,
    pub ignored_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlattenPlan {
    pub root: PathBuf,
    pub target_dir: PathBuf,
    pub deepest_dir: PathBuf,
    pub chain_dirs: Vec<PathBuf>,
    pub keep_last_dir: bool,
    pub files: Vec<PathBuf>,
    pub ignored_files: Vec<PathBuf>,
    pub removed_dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveAction {
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlattenResult {
    pub moves: Vec<MoveAction>,
    pub removed_files: Vec<PathBuf>,
    pub removed_dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    Discover { current: usize },
    Scan { current: usize, total: usize },
    Apply { current: usize, total: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanOptions {
    ignored_names: HashSet<String>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self::with_default_ignored()
    }
}

impl ScanOptions {
    pub fn with_default_ignored() -> Self {
        let mut ignored_names = HashSet::new();
        for name in ["desktop.ini", "Thumbs.db", ".DS_Store"] {
            ignored_names.insert(name.to_ascii_lowercase());
        }
        Self { ignored_names }
    }

    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let ignored_names = names
            .into_iter()
            .map(|name| name.as_ref().to_ascii_lowercase())
            .collect();
        Self { ignored_names }
    }

    pub fn extend_names<I, S>(&mut self, names: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.ignored_names.extend(
            names
                .into_iter()
                .map(|name| name.as_ref().to_ascii_lowercase()),
        );
    }

    fn ignores(&self, path: &Path) -> bool {
        path.file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
            .is_some_and(|name| self.ignored_names.contains(&name))
    }

    pub fn matches_name(&self, name: &str) -> bool {
        self.ignored_names.contains(&name.to_ascii_lowercase())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlattenMode {
    KeepEndpoints,
    OneLevel,
    CollapseChain,
}

pub fn collect_empty_dirs(root: &Path, options: &ScanOptions) -> Result<Vec<EmptyDirCandidate>> {
    collect_empty_dirs_with_progress(root, options, |_event| {})
}

pub fn collect_empty_dirs_with_progress<F>(
    root: &Path,
    options: &ScanOptions,
    mut on_progress: F,
) -> Result<Vec<EmptyDirCandidate>>
where
    F: FnMut(ProgressEvent),
{
    ensure_directory(root)?;
    let mut dirs = collect_dirs(root, &mut on_progress);

    dirs.sort_by_key(|path| std::cmp::Reverse(path.components().count()));

    let mut removed: HashSet<PathBuf> = HashSet::new();
    let mut candidates = Vec::new();

    let total = dirs.len();

    for (index, dir) in dirs.into_iter().enumerate() {
        on_progress(ProgressEvent::Scan {
            current: index + 1,
            total,
        });
        let inspection = inspect_directory(&dir, options)?;
        if inspection
            .child_dirs
            .iter()
            .all(|child| removed.contains(child))
            && inspection.regular_files.is_empty()
        {
            removed.insert(dir.clone());
            candidates.push(EmptyDirCandidate {
                path: dir,
                ignored_files: inspection.ignored_files,
            });
        }
    }

    Ok(candidates)
}

pub fn remove_empty_dirs(candidates: &[EmptyDirCandidate]) -> Result<Vec<PathBuf>> {
    remove_empty_dirs_with_progress(candidates, |_event| {})
}

pub fn remove_empty_dirs_with_progress<F>(
    candidates: &[EmptyDirCandidate],
    mut on_progress: F,
) -> Result<Vec<PathBuf>>
where
    F: FnMut(ProgressEvent),
{
    let mut deleted = Vec::new();
    let total = candidates.len();

    for (index, candidate) in candidates.iter().enumerate() {
        on_progress(ProgressEvent::Apply {
            current: index + 1,
            total,
        });
        for ignored in &candidate.ignored_files {
            if ignored.exists() {
                fs::remove_file(ignored).with_context(|| {
                    format!("failed to remove ignored file {}", ignored.display())
                })?;
            }
        }
        fs::remove_dir(&candidate.path)
            .with_context(|| format!("failed to remove empty dir {}", candidate.path.display()))?;
        deleted.push(candidate.path.clone());
    }

    Ok(deleted)
}

pub fn collect_flatten_plans(
    root: &Path,
    options: &ScanOptions,
    mode: FlattenMode,
) -> Result<Vec<FlattenPlan>> {
    collect_flatten_plans_with_progress(root, options, mode, |_event| {})
}

pub fn collect_flatten_plans_with_progress<F>(
    root: &Path,
    options: &ScanOptions,
    mode: FlattenMode,
    mut on_progress: F,
) -> Result<Vec<FlattenPlan>>
where
    F: FnMut(ProgressEvent),
{
    ensure_directory(root)?;
    let mut dirs = collect_dirs(root, &mut on_progress);

    dirs.sort_by_key(|path| path.components().count());

    if mode == FlattenMode::KeepEndpoints {
        let total = dirs.len();
        for (index, _dir) in dirs.into_iter().enumerate() {
            on_progress(ProgressEvent::Scan {
                current: index + 1,
                total,
            });
        }
        return analyze_keep_endpoints(root, options);
    }

    let mut covered = HashSet::new();
    let mut plans = Vec::new();

    let total = dirs.len();

    for (index, dir) in dirs.into_iter().enumerate() {
        on_progress(ProgressEvent::Scan {
            current: index + 1,
            total,
        });
        if covered.contains(&dir) {
            continue;
        }

        let found_plans: Vec<FlattenPlan> =
            analyze_chain(&dir, options, mode)?.into_iter().collect();

        if !found_plans.is_empty() {
            for plan in &found_plans {
                for chain_dir in &plan.chain_dirs {
                    covered.insert(chain_dir.clone());
                }
                for removed_dir in &plan.removed_dirs {
                    covered.insert(removed_dir.clone());
                }
            }
            plans.extend(found_plans);
        }
    }

    Ok(plans)
}

fn collect_dirs<F>(root: &Path, on_progress: &mut F) -> Vec<PathBuf>
where
    F: FnMut(ProgressEvent),
{
    let mut dirs = Vec::new();

    for (index, entry) in WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_dir())
        .enumerate()
    {
        on_progress(ProgressEvent::Discover { current: index + 1 });
        dirs.push(entry.into_path());
    }

    dirs
}

pub fn execute_flatten_plan(plan: &FlattenPlan) -> Result<FlattenResult> {
    execute_flatten_plan_with_progress(plan, |_event| {})
}

pub fn execute_flatten_plan_with_progress<F>(
    plan: &FlattenPlan,
    mut on_progress: F,
) -> Result<FlattenResult>
where
    F: FnMut(ProgressEvent),
{
    let mut moves = Vec::new();
    let mut removed_files = Vec::new();
    let total = plan.files.len() + plan.ignored_files.len() + plan.removed_dirs.len();
    let mut current = 0;

    for source in &plan.files {
        current += 1;
        on_progress(ProgressEvent::Apply { current, total });
        if plan.keep_last_dir && !plan.target_dir.exists() {
            fs::create_dir_all(&plan.target_dir).with_context(|| {
                format!(
                    "failed to create destination dir {}",
                    plan.target_dir.display()
                )
            })?;
        }
        let file_name = source
            .file_name()
            .map(OsString::from)
            .with_context(|| format!("missing file name for {}", source.display()))?;
        let destination = unique_destination(&plan.target_dir, &file_name);

        fs::rename(source, &destination).with_context(|| {
            format!(
                "failed to move {} to {}",
                source.display(),
                destination.display()
            )
        })?;

        moves.push(MoveAction {
            from: source.clone(),
            to: destination,
        });
    }

    for ignored in &plan.ignored_files {
        current += 1;
        on_progress(ProgressEvent::Apply { current, total });
        if ignored.exists() {
            fs::remove_file(ignored)
                .with_context(|| format!("failed to remove ignored file {}", ignored.display()))?;
            removed_files.push(ignored.clone());
        }
    }

    for dir in &plan.removed_dirs {
        current += 1;
        on_progress(ProgressEvent::Apply { current, total });
        if dir.exists() && is_directory_empty(dir)? {
            fs::remove_dir(dir)
                .with_context(|| format!("failed to remove emptied dir {}", dir.display()))?;
        }
    }

    Ok(FlattenResult {
        moves,
        removed_files,
        removed_dirs: plan.removed_dirs.clone(),
    })
}

fn analyze_chain(
    start: &Path,
    options: &ScanOptions,
    mode: FlattenMode,
) -> Result<Option<FlattenPlan>> {
    let mut current = start.to_path_buf();
    let mut chain = vec![current.clone()];

    loop {
        let inspection = inspect_directory(&current, options)?;
        let dirs_here = inspection.child_dirs;
        let files_here = inspection.regular_files;

        if !files_here.is_empty() {
            return Ok(None);
        }

        if dirs_here.len() != 1 {
            return Ok(None);
        }

        current = dirs_here[0].clone();
        chain.push(current.clone());

        let nested = inspect_directory(&current, options)?;
        let nested_dirs = nested.child_dirs;
        let nested_files = nested.regular_files;
        let ignored_files = nested.ignored_files;

        if !nested_files.is_empty() {
            if nested_dirs.is_empty() {
                let root = chain.first().cloned().expect("chain has root");
                let deepest_dir = current.clone();
                let keep_last_dir = matches!(mode, FlattenMode::KeepEndpoints);
                if keep_last_dir && chain.len() < 3 {
                    return Ok(None);
                }
                let target_dir = match mode {
                    FlattenMode::KeepEndpoints => {
                        let deepest_name = deepest_dir
                            .file_name()
                            .map(OsString::from)
                            .expect("deepest dir has a name");
                        unique_dir_destination(&root, &deepest_name)
                    }
                    FlattenMode::OneLevel => chain
                        .get(chain.len().saturating_sub(2))
                        .cloned()
                        .unwrap_or_else(|| root.clone()),
                    FlattenMode::CollapseChain => root.clone(),
                };
                let removed_dirs = match mode {
                    FlattenMode::KeepEndpoints => {
                        chain[1..].iter().rev().cloned().collect::<Vec<PathBuf>>()
                    }
                    FlattenMode::OneLevel => vec![deepest_dir.clone()],
                    FlattenMode::CollapseChain => {
                        chain[1..].iter().rev().cloned().collect::<Vec<PathBuf>>()
                    }
                };

                return Ok(Some(FlattenPlan {
                    root,
                    target_dir,
                    deepest_dir,
                    chain_dirs: chain.clone(),
                    keep_last_dir,
                    files: nested_files,
                    ignored_files,
                    removed_dirs,
                }));
            }

            return Ok(None);
        }

        if nested_dirs.len() != 1 {
            return Ok(None);
        }
    }
}

fn analyze_keep_endpoints(start: &Path, options: &ScanOptions) -> Result<Vec<FlattenPlan>> {
    let start_inspection = inspect_directory(start, options)?;
    let mut plans = Vec::new();

    for anchor in &start_inspection.child_dirs {
        let anchor_inspection = inspect_directory(&anchor, options)?;
        collect_date_anchor_flatten_plans(options, &anchor_inspection, &mut plans)?;
    }

    if plans.is_empty() {
        collect_date_anchor_flatten_plans(options, &start_inspection, &mut plans)?;
    }

    Ok(plans)
}

fn collect_date_anchor_flatten_plans(
    options: &ScanOptions,
    anchor_inspection: &DirectoryInspection,
    plans: &mut Vec<FlattenPlan>,
) -> Result<()> {
    for first_level in &anchor_inspection.child_dirs {
        let first_level_inspection = inspect_directory(first_level, options)?;
        let mut ignored_files = Vec::new();
        ignored_files.extend(first_level_inspection.ignored_files);

        for child in first_level_inspection.child_dirs {
            let mut chain = vec![first_level.clone(), child];
            collect_keep_endpoint_leaves(
                first_level,
                options,
                &mut chain,
                &mut ignored_files,
                plans,
            )?;
        }
    }

    Ok(())
}

fn collect_keep_endpoint_leaves(
    root: &Path,
    options: &ScanOptions,
    chain: &mut Vec<PathBuf>,
    inherited_ignored_files: &mut Vec<PathBuf>,
    plans: &mut Vec<FlattenPlan>,
) -> Result<()> {
    let current = chain.last().cloned().expect("chain has current directory");
    let inspection = inspect_directory(&current, options)?;
    let inherited_len = inherited_ignored_files.len();
    inherited_ignored_files.extend(inspection.ignored_files.clone());

    if !inspection.regular_files.is_empty() {
        if inspection.child_dirs.is_empty() {
            let deepest_dir = current;
            let target_dir = root.to_path_buf();
            let removed_dirs = chain[1..].iter().rev().cloned().collect::<Vec<PathBuf>>();

            plans.push(FlattenPlan {
                root: root.to_path_buf(),
                target_dir,
                deepest_dir,
                chain_dirs: chain.clone(),
                keep_last_dir: false,
                files: inspection.regular_files,
                ignored_files: inherited_ignored_files.clone(),
                removed_dirs,
            });
        }

        inherited_ignored_files.truncate(inherited_len);
        return Ok(());
    }

    for child in inspection.child_dirs {
        chain.push(child);
        collect_keep_endpoint_leaves(root, options, chain, inherited_ignored_files, plans)?;
        chain.pop();
    }

    inherited_ignored_files.truncate(inherited_len);
    Ok(())
}

fn ensure_directory(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("path does not exist: {}", path.display());
    }

    if !path.is_dir() {
        bail!("path is not a directory: {}", path.display());
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectoryInspection {
    child_dirs: Vec<PathBuf>,
    regular_files: Vec<PathBuf>,
    ignored_files: Vec<PathBuf>,
}

fn inspect_directory(path: &Path, options: &ScanOptions) -> Result<DirectoryInspection> {
    let mut child_dirs = Vec::new();
    let mut regular_files = Vec::new();
    let mut ignored_files = Vec::new();

    for entry in fs::read_dir(path)
        .with_context(|| format!("failed to read directory {}", path.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read entry in {}", path.display()))?;
        let child_path = entry.path();
        if child_path.is_dir() {
            child_dirs.push(child_path);
        } else if child_path.is_file() {
            if options.ignores(&child_path) {
                ignored_files.push(child_path);
            } else {
                regular_files.push(child_path);
            }
        }
    }

    child_dirs.sort();
    regular_files.sort();
    ignored_files.sort();

    Ok(DirectoryInspection {
        child_dirs,
        regular_files,
        ignored_files,
    })
}

fn is_directory_empty(path: &Path) -> Result<bool> {
    let mut entries = fs::read_dir(path)
        .with_context(|| format!("failed to read directory {}", path.display()))?;
    Ok(entries.next().is_none())
}

fn unique_destination(target_dir: &Path, file_name: &OsString) -> PathBuf {
    let base_path = target_dir.join(file_name);
    if !base_path.exists() {
        return base_path;
    }

    let file_path = Path::new(file_name);
    let stem = file_path
        .file_stem()
        .map(OsString::from)
        .unwrap_or_else(|| file_name.clone());
    let extension = file_path.extension().map(OsString::from);

    for index in 1.. {
        let mut candidate_name = stem.clone();
        candidate_name.push(format!(" ({index})"));
        if let Some(ext) = &extension {
            candidate_name.push(".");
            candidate_name.push(ext);
        }

        let candidate = target_dir.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("infinite iterator always returns a destination")
}

fn unique_dir_destination(target_dir: &Path, dir_name: &OsString) -> PathBuf {
    let base_path = target_dir.join(dir_name);
    if !base_path.exists() {
        return base_path;
    }

    for index in 1.. {
        let mut candidate_name = dir_name.clone();
        candidate_name.push(format!(" ({index})"));
        let candidate = target_dir.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("infinite iterator always returns a destination")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
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

        let candidates = collect_empty_dirs(root, &ScanOptions::default()).unwrap();
        let paths = candidates
            .into_iter()
            .map(|item| item.path)
            .collect::<Vec<_>>();

        assert_eq!(paths, vec![abc, root.join("a").join("b"), root.join("a"),]);
    }

    #[test]
    fn detects_single_chain_that_ends_with_files() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let deep = root.join("outer").join("inner").join("deep");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("x.txt"), "x").unwrap();
        fs::write(deep.join("y.txt"), "y").unwrap();

        let plans =
            collect_flatten_plans(root, &ScanOptions::default(), FlattenMode::CollapseChain)
                .unwrap();
        assert_eq!(plans.len(), 1);

        let plan = &plans[0];
        assert_eq!(plan.root, root.join("outer"));
        assert_eq!(plan.target_dir, root.join("outer"));
        assert_eq!(plan.deepest_dir, deep);
        assert_eq!(
            plan.chain_dirs,
            vec![
                root.join("outer"),
                root.join("outer").join("inner"),
                root.join("outer").join("inner").join("deep")
            ]
        );
        assert_eq!(
            plan.removed_dirs,
            vec![
                root.join("outer").join("inner").join("deep"),
                root.join("outer").join("inner")
            ]
        );
    }

    #[test]
    fn unique_destination_renames_on_conflict() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join("deep.txt"), "existing").unwrap();

        let destination = unique_destination(root, &OsString::from("deep.txt"));

        assert_eq!(
            destination.file_name().unwrap().to_string_lossy().as_ref(),
            "deep (1).txt"
        );
    }

    #[test]
    fn skips_chain_when_intermediate_dir_contains_file() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let outer = root.join("outer");
        let inner = outer.join("inner");
        fs::create_dir_all(&inner).unwrap();
        let mut file = fs::File::create(outer.join("note.txt")).unwrap();
        writeln!(file, "keep").unwrap();
        fs::write(inner.join("x.txt"), "x").unwrap();

        let plans =
            collect_flatten_plans(root, &ScanOptions::default(), FlattenMode::CollapseChain)
                .unwrap();
        assert!(plans.is_empty());
    }

    #[test]
    fn ignored_junk_files_still_count_as_empty() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let folder = root.join("album");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("desktop.ini"), "noise").unwrap();

        let candidates = collect_empty_dirs(root, &ScanOptions::default()).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].ignored_files,
            vec![folder.join("desktop.ini")]
        );
    }

    #[test]
    fn one_level_mode_moves_to_parent_only() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let deep = root.join("outer").join("inner").join("deep");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("x.txt"), "x").unwrap();

        let plans =
            collect_flatten_plans(root, &ScanOptions::default(), FlattenMode::OneLevel).unwrap();
        assert_eq!(plans.len(), 1);
        let plan = &plans[0];

        assert_eq!(plan.root, root.join("outer"));
        assert_eq!(plan.target_dir, root.join("outer").join("inner"));
        assert_eq!(
            plan.chain_dirs,
            vec![
                root.join("outer"),
                root.join("outer").join("inner"),
                root.join("outer").join("inner").join("deep")
            ]
        );
        assert_eq!(
            plan.removed_dirs,
            vec![root.join("outer").join("inner").join("deep")]
        );
    }

    #[test]
    fn keep_endpoints_mode_keeps_first_folder_inside_date_anchor() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let anchor = root.join("a");
        let deep = anchor.join("b").join("c");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("x.txt"), "x").unwrap();

        let plans =
            collect_flatten_plans(root, &ScanOptions::default(), FlattenMode::KeepEndpoints)
                .unwrap();
        assert_eq!(plans.len(), 1);

        let plan = &plans[0];
        assert_eq!(plan.root, anchor.join("b"));
        assert_eq!(plan.deepest_dir, anchor.join("b").join("c"));
        assert_eq!(plan.target_dir, anchor.join("b"));
        assert!(!plan.keep_last_dir);
        assert_eq!(plan.removed_dirs, vec![anchor.join("b").join("c")]);
    }

    #[test]
    fn keep_endpoints_mode_removes_shared_middle_folder_for_multiple_leaves() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let anchor = root.join("a");
        let c = anchor.join("keep").join("b").join("c");
        let d = anchor.join("keep").join("b").join("d");
        fs::create_dir_all(&c).unwrap();
        fs::create_dir_all(&d).unwrap();
        fs::write(c.join("c.txt"), "c").unwrap();
        fs::write(d.join("d.txt"), "d").unwrap();

        let mut plans =
            collect_flatten_plans(root, &ScanOptions::default(), FlattenMode::KeepEndpoints)
                .unwrap();
        plans.sort_by_key(|plan| plan.deepest_dir.clone());

        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].target_dir, anchor.join("keep"));
        assert_eq!(plans[1].target_dir, anchor.join("keep"));

        execute_flatten_plan(&plans[0]).unwrap();
        execute_flatten_plan(&plans[1]).unwrap();

        assert!(anchor.join("keep").join("c.txt").exists());
        assert!(anchor.join("keep").join("d.txt").exists());
        assert!(!anchor.join("keep").join("b").exists());
    }

    #[test]
    fn keep_endpoints_mode_removes_all_middle_layers() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let anchor = root.join("a");
        let deep = anchor.join("keep").join("b").join("x").join("y").join("c");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("photo.jpg"), "photo").unwrap();

        let mut plans =
            collect_flatten_plans(root, &ScanOptions::default(), FlattenMode::KeepEndpoints)
                .unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].target_dir, anchor.join("keep"));

        execute_flatten_plan(&plans.remove(0)).unwrap();

        assert!(anchor.join("keep").join("photo.jpg").exists());
        assert!(!anchor.join("keep").join("b").exists());
    }

    #[test]
    fn keep_endpoints_mode_keeps_first_folder_and_removes_title_shell() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let anchor = root.join("20250203");
        let leaf = anchor.join("project_alpha").join("archive_shell");
        fs::create_dir_all(&leaf).unwrap();
        fs::write(anchor.join("already-here.txt"), "keep").unwrap();
        fs::write(leaf.join("video.mp4"), "video").unwrap();

        let mut plans =
            collect_flatten_plans(root, &ScanOptions::default(), FlattenMode::KeepEndpoints)
                .unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].root, anchor.join("project_alpha"));
        assert_eq!(plans[0].target_dir, anchor.join("project_alpha"));
        assert!(!plans[0].keep_last_dir);

        execute_flatten_plan(&plans.remove(0)).unwrap();

        assert!(anchor.join("already-here.txt").exists());
        assert!(anchor.join("project_alpha").join("video.mp4").exists());
        assert!(!anchor.join("project_alpha").join("archive_shell").exists());
    }

    #[test]
    fn keep_endpoints_mode_can_scan_selected_date_folder_directly() {
        let temp = tempdir().unwrap();
        let anchor = temp.path().join("20250203");
        let leaf = anchor.join("project_alpha").join("archive_shell");
        fs::create_dir_all(&leaf).unwrap();
        fs::write(leaf.join("clip.mp4"), "clip").unwrap();

        let mut plans =
            collect_flatten_plans(&anchor, &ScanOptions::default(), FlattenMode::KeepEndpoints)
                .unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].root, anchor.join("project_alpha"));
        assert_eq!(plans[0].target_dir, anchor.join("project_alpha"));

        execute_flatten_plan(&plans.remove(0)).unwrap();

        assert!(anchor.join("project_alpha").join("clip.mp4").exists());
        assert!(!anchor.join("project_alpha").join("archive_shell").exists());
    }

    #[test]
    fn execute_flatten_removes_ignored_files_in_deepest_dir() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let deep = root.join("outer").join("inner");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("x.txt"), "x").unwrap();
        fs::write(deep.join("desktop.ini"), "noise").unwrap();

        let mut plans =
            collect_flatten_plans(root, &ScanOptions::default(), FlattenMode::OneLevel).unwrap();
        let result = execute_flatten_plan(&plans.remove(0)).unwrap();

        assert_eq!(result.removed_files, vec![deep.join("desktop.ini")]);
        assert!(root.join("outer").join("x.txt").exists());
        assert!(!deep.exists());
    }
}
