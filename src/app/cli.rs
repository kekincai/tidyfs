use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::domain::flatten::FlattenMode;
use crate::engine::Task;
use crate::infra::config::{LoadedConfig, load_config};
use crate::infra::{drives, logging};

use super::{console, launcher, serve};

const HELP_TEMPLATE: &str = "{about-with-newline}\n用法:\n    {usage}\n\n{all-args}{after-help}";

#[derive(Debug, Parser)]
#[command(
    name = "tidyfs",
    version,
    about = "清理空文件夹，并拉平只有最内层才有文件的多层目录。",
    long_about = "清理空文件夹，并拉平只有最内层才有文件的多层目录。\n\n不带子命令运行时会打开交互界面（需要 Node.js 22+）。",
    help_template = HELP_TEMPLATE,
    after_help = "示例:\n    tidyfs empty D:\\ E:\\              同时扫描两块磁盘的空文件夹\n    tidyfs empty D:\\Downloads --apply   删除空文件夹\n    tidyfs flatten D:\\Photos --mode keep-endpoints --apply"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "配置文件路径。默认读取当前目录或程序目录下的 tidyfs.toml。"
    )]
    config: Option<PathBuf>,
    #[arg(short, long, global = true, help = "把诊断日志同时输出到终端。")]
    verbose: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(
        about = "扫描空文件夹，传 --apply 才会真正删除。可以同时传多个目录或磁盘。",
        visible_alias = "scan-empty",
        alias = "remove-empty",
        help_template = HELP_TEMPLATE
    )]
    Empty(TaskArgs),
    #[command(
        about = "扫描可拉平的目录，传 --apply 才会真正执行。",
        visible_alias = "detect-chains",
        alias = "flatten-single-chain",
        help_template = HELP_TEMPLATE,
        after_help = "模式:\n    keep-endpoints  按日期目录分组，保留日期内第一层，去掉更深壳目录（默认）\n    one-level       只提升一层\n    collapse-chain  一路压平到链起点"
    )]
    Flatten(FlattenArgs),
    #[command(about = "列出本机磁盘。", help_template = HELP_TEMPLATE)]
    Drives {
        #[arg(long, help = "以 JSON 输出。")]
        json: bool,
    },
    #[command(about = "打开交互界面。", help_template = HELP_TEMPLATE)]
    Ui,
    #[command(
        about = "以 JSON Lines 协议在 stdin/stdout 上提供服务（供界面使用）。",
        hide = true
    )]
    Serve,
}

#[derive(Debug, Args, Clone)]
pub struct TaskArgs {
    #[arg(
        value_name = "PATH",
        help = "要处理的目录或磁盘，可传多个。不传时弹出文件夹选择框。"
    )]
    pub paths: Vec<PathBuf>,
    #[arg(long, help = "真正执行。不传时只预览。")]
    pub apply: bool,
    #[arg(long, conflicts_with = "apply", hide = true)]
    pub dry_run: bool,
    #[arg(
        long = "ignore-name",
        value_name = "NAME",
        help = "额外当作噪音文件的文件名，可重复传入。"
    )]
    pub ignore_names: Vec<String>,
    #[arg(long, value_name = "BOOL", action = clap::ArgAction::Set, help = "是否启用内置噪音文件名单（desktop.ini、Thumbs.db、.DS_Store）。")]
    pub default_ignored: Option<bool>,
    #[arg(long, value_name = "PATH", help = "把输出同时写入文件。")]
    pub log_file: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
pub struct FlattenArgs {
    #[command(flatten)]
    pub common: TaskArgs,
    #[arg(long, value_enum, help = "拉平规则，默认 keep-endpoints。")]
    pub mode: Option<FlattenMode>,
}

/// 程序入口，返回进程退出码。
pub fn run() -> i32 {
    let cli = Cli::parse();
    let component = match cli.command {
        Some(Command::Serve) => "engine",
        _ => "cli",
    };
    let _log = logging::init(component, cli.verbose);
    tracing::info!(version = env!("CARGO_PKG_VERSION"), command = ?cli.command, "start");

    let double_clicked = cli.command.is_none();
    let result =
        load_config(cli.config.as_deref()).and_then(|config| dispatch(cli.command, &config));
    match result {
        Ok(code) => code,
        Err(error) => {
            tracing::error!(error = %format!("{error:#}"), "fatal");
            eprintln!("错误: {error:#}");
            if double_clicked {
                eprintln!();
                eprintln!("也可以直接使用命令行，例如：tidyfs empty D:\\ --apply");
                console::pause("按回车退出...");
            }
            1
        }
    }
}

fn dispatch(command: Option<Command>, config: &LoadedConfig) -> anyhow::Result<i32> {
    match command {
        None | Some(Command::Ui) => launcher::launch(),
        Some(Command::Serve) => serve::run(config).map(|()| 0),
        Some(Command::Drives { json }) => {
            let drives = drives::list();
            if json {
                println!("{}", serde_json::to_string_pretty(&drives)?);
            } else {
                for drive in drives {
                    println!(
                        "{:<5} {:<16} {:<6} {:>10} 可用 / {:>10}",
                        drive.path,
                        drive.label,
                        drive.file_system,
                        console::format_bytes(drive.free_bytes),
                        console::format_bytes(drive.total_bytes)
                    );
                }
            }
            Ok(0)
        }
        Some(Command::Empty(args)) => console::run_task(config, Task::Empty, &args),
        Some(Command::Flatten(args)) => {
            let task = Task::Flatten(config.flatten_mode(args.mode));
            console::run_task(config, task, &args.common)
        }
    }
}
