use anyhow::{Result, bail};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use rfd::FileDialog;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tidyfs::cli::{Cli, Command};
use tidyfs::config::{LoadedConfig, load_config};
use tidyfs::journal::Journal;
use tidyfs::ops::{
    EmptyRemovalFailureKind, ProgressEvent, collect_empty_dirs_with_progress,
    collect_flatten_plans_with_progress, execute_flatten_plans_parallel_with_progress,
    remove_empty_dirs_with_progress,
};

fn main() {
    let cli = Cli::parse();
    let interactive = cli.command.is_none();

    let result = run(cli);

    match result {
        Ok(()) => {}
        Err(error) => {
            eprintln!("错误: {error:#}");
            if interactive {
                pause_console("按回车继续...");
            }
            std::process::exit(1);
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let config = load_config(cli.config.as_deref())?;

    match cli.command {
        Some(command) => run_command(command, &config),
        None => run_interactive(&config),
    }
}

fn run_command(command: Command, config: &LoadedConfig) -> Result<()> {
    let mut log_lines = Vec::new();

    if let Some(path) = &config.path {
        emit(&mut log_lines, format!("[CONFIG] {}", path.display()));
    }

    match command {
        Command::ScanEmpty(args) => {
            let path = resolve_target_path(args.path)?;
            let options = config.scan_options(args.ignore.default_ignored, &args.ignore.names);
            let mut reporter = ScanProgress::new("Scanning folders");
            let candidates = collect_empty_dirs_with_progress(&path, &options, |event| {
                reporter.update(event);
            })?;
            reporter.finish(format!(
                "Scan complete: {} empty folders found",
                candidates.len()
            ));

            if candidates.is_empty() {
                emit(&mut log_lines, "No empty directories found.");
            } else {
                for candidate in candidates {
                    emit(
                        &mut log_lines,
                        format!("[EMPTY] {}", candidate.path.display()),
                    );
                    for ignored in candidate.ignored_files {
                        emit(&mut log_lines, format!("[IGNORED] {}", ignored.display()));
                    }
                }
            }
            flush_log(args.log_file.as_deref(), &log_lines)?;
        }
        Command::RemoveEmpty(args) => {
            let path = resolve_target_path(args.path)?;
            let options = config.scan_options(args.ignore.default_ignored, &args.ignore.names);
            let mut reporter = ScanProgress::new("Scanning folders");
            let candidates = collect_empty_dirs_with_progress(&path, &options, |event| {
                reporter.update(event);
            })?;
            reporter.finish(format!(
                "Scan complete: {} empty folders ready",
                candidates.len()
            ));

            if candidates.is_empty() {
                emit(&mut log_lines, "No empty directories found.");
            } else if args.apply {
                let apply_progress = make_bar(candidates.len() as u64, "Removing empty folders...");
                let mut journal = Journal::default();
                let result = remove_empty_dirs_with_progress(&candidates, |event| {
                    update_progress(&apply_progress, event, "Removing empty folders");
                })?;
                for removed_file in &result.removed_files {
                    emit(
                        &mut log_lines,
                        format!("[REMOVED-FILE] {}", removed_file.display()),
                    );
                }
                let deleted_count = result.deleted_dirs.len();
                for deleted in result.deleted_dirs {
                    emit(&mut log_lines, format!("[REMOVED] {}", deleted.display()));
                    journal.record_removed_dirs([deleted]);
                }
                for failure in &result.failures {
                    emit(
                        &mut log_lines,
                        format_empty_removal_failure(failure.kind, &failure.path, &failure.error),
                    );
                }
                finish_progress(
                    &apply_progress,
                    format!(
                        "Finished: {} removed, {} failed",
                        deleted_count,
                        result.failures.len()
                    ),
                );
                write_journal(&path, &journal)?;
            } else {
                for candidate in candidates {
                    for ignored in candidate.ignored_files {
                        emit(
                            &mut log_lines,
                            format!("[DRY-RUN][REMOVE-FILE] {}", ignored.display()),
                        );
                    }
                    emit(
                        &mut log_lines,
                        format!("[DRY-RUN][REMOVE] {}", candidate.path.display()),
                    );
                }
            }
            flush_log(args.log_file.as_deref(), &log_lines)?;
        }
        Command::DetectChains(args) => {
            let path = resolve_target_path(args.path)?;
            let options = config.scan_options(args.ignore.default_ignored, &args.ignore.names);
            let mut reporter = ScanProgress::new("Scanning for chains");
            let plans = collect_flatten_plans_with_progress(
                &path,
                &options,
                config.flatten_mode(args.mode),
                |event| reporter.update(event),
            )?;
            reporter.finish(format!(
                "Scan complete: {} flattenable chains found",
                plans.len()
            ));

            if plans.is_empty() {
                emit(&mut log_lines, "No flattenable single-child chains found.");
            } else {
                for plan in plans {
                    emit(
                        &mut log_lines,
                        format!(
                            "[CHAIN] {} -> {}",
                            plan.deepest_dir.display(),
                            plan.target_dir.display()
                        ),
                    );
                    for ignored in plan.ignored_files {
                        emit(&mut log_lines, format!("[IGNORED] {}", ignored.display()));
                    }
                    for file in plan.files {
                        emit(&mut log_lines, format!("[FILE] {}", file.display()));
                    }
                }
            }
            flush_log(args.log_file.as_deref(), &log_lines)?;
        }
        Command::FlattenSingleChain(args) => {
            let path = resolve_target_path(args.path)?;
            let options = config.scan_options(args.ignore.default_ignored, &args.ignore.names);
            let mut reporter = ScanProgress::new("Scanning for chains");
            let plans = collect_flatten_plans_with_progress(
                &path,
                &options,
                config.flatten_mode(args.mode),
                |event| reporter.update(event),
            )?;
            reporter.finish(format!(
                "Scan complete: {} flattenable chains found",
                plans.len()
            ));

            if plans.is_empty() {
                emit(&mut log_lines, "No flattenable single-child chains found.");
            } else if args.apply {
                let total_steps = plans
                    .iter()
                    .map(|plan| {
                        plan.files.len() + plan.ignored_files.len() + plan.removed_dirs.len()
                    })
                    .sum::<usize>();
                let chain_count = plans.len();
                let apply_progress = make_bar(total_steps as u64, "Applying flatten operations...");
                let mut journal = Journal::default();

                for plan in &plans {
                    emit(
                        &mut log_lines,
                        format!(
                            "[CHAIN] {} -> {}",
                            plan.deepest_dir.display(),
                            plan.target_dir.display()
                        ),
                    );
                }

                let results = execute_flatten_plans_parallel_with_progress(&plans, |event| {
                    if let ProgressEvent::Apply { current, total } = event {
                        apply_progress.set_position(current as u64);
                        apply_progress.set_message(format!(
                            "Applying flatten operations ({current}/{total})"
                        ));
                    }
                })?;

                journal.record_flatten_results_ordered(&results);
                for result in &results {
                    for moved in &result.moves {
                        emit(
                            &mut log_lines,
                            format!("[MOVED] {} -> {}", moved.from.display(), moved.to.display()),
                        );
                    }
                }
                for result in &results {
                    for file in &result.removed_files {
                        emit(&mut log_lines, format!("[REMOVED-FILE] {}", file.display()));
                    }
                }
                let mut removed_dirs = results
                    .into_iter()
                    .flat_map(|result| result.removed_dirs)
                    .collect::<Vec<_>>();
                removed_dirs.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
                for dir in removed_dirs {
                    emit(&mut log_lines, format!("[REMOVED] {}", dir.display()));
                }

                finish_progress(
                    &apply_progress,
                    format!(
                        "Finished flattening {} chains in {} steps",
                        chain_count, total_steps
                    ),
                );
                write_journal(&path, &journal)?;
            } else {
                for plan in plans {
                    emit(
                        &mut log_lines,
                        format!(
                            "[DRY-RUN][CHAIN] {} -> {}",
                            plan.deepest_dir.display(),
                            plan.target_dir.display()
                        ),
                    );
                    for ignored in plan.ignored_files {
                        emit(
                            &mut log_lines,
                            format!("[DRY-RUN][REMOVE-FILE] {}", ignored.display()),
                        );
                    }
                    for file in &plan.files {
                        let name = file
                            .file_name()
                            .map(|part| part.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "<unknown>".to_string());
                        emit(
                            &mut log_lines,
                            format!(
                                "[DRY-RUN][MOVE] {} -> {}",
                                file.display(),
                                plan.target_dir.join(name).display()
                            ),
                        );
                    }
                    for dir in plan.removed_dirs {
                        emit(
                            &mut log_lines,
                            format!("[DRY-RUN][REMOVE] {}", dir.display()),
                        );
                    }
                }
            }
            flush_log(args.log_file.as_deref(), &log_lines)?;
        }
    }

    Ok(())
}

fn run_interactive(config: &LoadedConfig) -> Result<()> {
    loop {
        println!();
        println!("tidyfs");
        println!("1. 扫描空文件夹，然后决定是否删除");
        println!("2. 扫描可拉平目录，然后决定是否执行");
        println!("3. 退出");
        print!("请选择功能: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        match input.trim() {
            "1" => {
                if let Err(error) = run_interactive_empty_cleanup(config) {
                    eprintln!("错误: {error:#}");
                }
                pause_console("按回车返回主菜单...");
            }
            "2" => {
                if let Err(error) = run_interactive_flatten(config) {
                    eprintln!("错误: {error:#}");
                }
                pause_console("按回车返回主菜单...");
            }
            "3" | "" => return Ok(()),
            _ => {
                println!("无效选择，请重新输入。");
            }
        }
    }
}

fn run_interactive_empty_cleanup(config: &LoadedConfig) -> Result<()> {
    let path = resolve_target_path(None)?;
    let options = config.scan_options(None, &[]);
    let mut reporter = ScanProgress::new("Scanning folders");
    let candidates = collect_empty_dirs_with_progress(&path, &options, |event| {
        reporter.update(event);
    })?;
    reporter.finish(format!("扫描完成：找到 {} 个空文件夹", candidates.len()));

    if candidates.is_empty() {
        println!("没有发现空文件夹。");
        return Ok(());
    }

    preview_empty_candidates(&candidates);

    if confirm("现在删除这些空文件夹吗？[y/N]: ")? {
        let progress = make_bar(candidates.len() as u64, "正在删除空文件夹...");
        let mut journal = Journal::default();
        let result = remove_empty_dirs_with_progress(&candidates, |event| {
            update_progress(&progress, event, "正在删除空文件夹");
        })?;
        let deleted_count = result.deleted_dirs.len();
        journal.record_removed_dirs(result.deleted_dirs.clone());
        finish_progress(
            &progress,
            format!(
                "删除完成：成功 {} 个，失败 {} 个",
                deleted_count,
                result.failures.len()
            ),
        );
        for removed_file in &result.removed_files {
            println!("[已删除文件] {}", removed_file.display());
        }
        for deleted in &result.deleted_dirs {
            println!("[已删除] {}", deleted.display());
        }
        for failure in &result.failures {
            println!(
                "{}",
                format_empty_removal_failure(failure.kind, &failure.path, &failure.error)
            );
        }
        write_journal(&path, &journal)?;
    } else {
        println!("已取消删除。");
    }

    Ok(())
}

fn run_interactive_flatten(config: &LoadedConfig) -> Result<()> {
    let path = resolve_target_path(None)?;
    let options = config.scan_options(None, &[]);
    let mode = config.flatten_mode(None);
    let mut reporter = ScanProgress::new("Scanning for chains");
    let plans = collect_flatten_plans_with_progress(&path, &options, mode, |event| {
        reporter.update(event);
    })?;
    reporter.finish(format!("扫描完成：找到 {} 组可拉平目录", plans.len()));

    if plans.is_empty() {
        println!("没有发现可拉平目录。");
        return Ok(());
    }

    preview_flatten_plans(&plans);

    if confirm("现在执行这些拉平操作吗？[y/N]: ")? {
        let chain_count = plans.len();
        let total_steps = plans
            .iter()
            .map(|plan| plan.files.len() + plan.ignored_files.len() + plan.removed_dirs.len())
            .sum::<usize>();
        let progress = make_bar(total_steps as u64, "正在执行拉平操作...");
        let mut journal = Journal::default();

        let results = execute_flatten_plans_parallel_with_progress(&plans, |event| {
            if let ProgressEvent::Apply { current, total } = event {
                progress.set_position(current as u64);
                progress.set_message(format!("正在执行拉平操作 ({current}/{total})"));
            }
        })?;

        journal.record_flatten_results_ordered(&results);

        finish_progress(
            &progress,
            format!(
                "拉平完成：共处理 {} 组目录，执行 {} 步",
                chain_count, total_steps
            ),
        );
        write_journal(&path, &journal)?;
    } else {
        println!("已取消拉平。");
    }

    Ok(())
}

fn preview_empty_candidates(candidates: &[tidyfs::ops::EmptyDirCandidate]) {
    let preview_limit = 20usize;
    for candidate in candidates.iter().take(preview_limit) {
        println!("[空文件夹] {}", candidate.path.display());
    }
    if candidates.len() > preview_limit {
        println!("... 另外还有 {} 个", candidates.len() - preview_limit);
    }
}

fn preview_flatten_plans(plans: &[tidyfs::ops::FlattenPlan]) {
    let preview_limit = 20usize;
    for plan in plans.iter().take(preview_limit) {
        println!(
            "[拉平] {} -> {}",
            plan.deepest_dir.display(),
            plan.target_dir.display()
        );
    }
    if plans.len() > preview_limit {
        println!("... 另外还有 {} 组", plans.len() - preview_limit);
    }
}

fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let answer = input.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

fn resolve_target_path(path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = path {
        return Ok(path);
    }

    match FileDialog::new()
        .set_title("请选择要处理的文件夹")
        .pick_folder()
    {
        Some(path) => Ok(path),
        None => bail!("没有选择文件夹"),
    }
}

fn emit<T: AsRef<str>>(lines: &mut Vec<String>, line: T) {
    let line = line.as_ref().to_string();
    println!("{line}");
    lines.push(line);
}

fn flush_log(path: Option<&Path>, lines: &[String]) -> Result<()> {
    if let Some(path) = path {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let content = if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        };
        fs::write(path, content)?;
    }
    Ok(())
}

fn write_journal(root: &Path, journal: &Journal) -> Result<()> {
    if let Some(path) = journal.write_to_root(root)? {
        println!("[JOURNAL] {}", path.display());
    }
    Ok(())
}

fn format_empty_removal_failure(kind: EmptyRemovalFailureKind, path: &Path, error: &str) -> String {
    match kind {
        EmptyRemovalFailureKind::IgnoredFile => {
            format!("[FAILED-FILE] {} | {}", path.display(), error)
        }
        EmptyRemovalFailureKind::EmptyDir => {
            format!("[FAILED] {} | {}", path.display(), error)
        }
    }
}

fn pause_console(message: &str) {
    print!("{message}");
    let _ = io::stdout().flush();
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
}

fn make_bar(length: u64, message: &str) -> ProgressBar {
    let progress = ProgressBar::new(length.max(1));
    progress.set_style(
        ProgressStyle::with_template("{bar:40.cyan/blue} {pos}/{len} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );
    progress.set_message(message.to_string());
    progress
}

fn update_progress(progress: &ProgressBar, event: ProgressEvent, label: &str) {
    match event {
        ProgressEvent::Discover { .. } => {}
        ProgressEvent::Scan { .. } => {}
        ProgressEvent::Apply { current, total } => {
            progress.set_length((total as u64).max(1));
            progress.set_position(current as u64);
            progress.set_message(format!("{label} ({current}/{total})"));
        }
    }
}

fn finish_progress(progress: &ProgressBar, message: String) {
    progress.finish_and_clear();
    println!("{message}");
}

struct ScanProgress {
    label: &'static str,
    progress: ProgressBar,
}

impl ScanProgress {
    fn new(label: &'static str) -> Self {
        let progress = ProgressBar::new_spinner();
        progress.enable_steady_tick(Duration::from_millis(80));
        progress.set_style(
            ProgressStyle::with_template("{spinner} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        progress.set_message(format!("{label}：正在发现文件夹"));
        Self { label, progress }
    }

    fn update(&mut self, event: ProgressEvent) {
        match event {
            ProgressEvent::Discover { current } => {
                self.progress
                    .set_message(format!("{}：正在发现文件夹（{}）", self.label, current));
            }
            ProgressEvent::Scan { current, total } => {
                self.progress.set_length(total as u64);
                self.progress.set_position(current as u64);
                self.progress.set_style(
                    ProgressStyle::with_template("{bar:40.cyan/blue} {pos}/{len} {msg}")
                        .unwrap_or_else(|_| ProgressStyle::default_bar()),
                );
                self.progress
                    .set_message(format!("{}：正在分析文件夹", self.label));
            }
            ProgressEvent::Apply { .. } => {}
        }
    }

    fn finish(&self, message: String) {
        self.progress.finish_and_clear();
        println!("{message}");
    }
}
