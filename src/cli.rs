use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

const HELP_TEMPLATE: &str = "{about-with-newline}\n用法:\n    {usage}\n\n{all-args}{after-help}";

#[derive(Debug, Parser)]
#[command(
    name = "tidyfs",
    version,
    about = "清理空文件夹，并拉平只有最内层才有文件的多层目录。",
    long_about = "tidyfs 用来做两类事情：\n1. 扫描并清理空文件夹\n2. 拉平单链目录结构\n\n如果不传子命令，程序会进入中文交互菜单。",
    help_template = HELP_TEMPLATE,
    after_help = "示例:\n    tidyfs scan-empty \"D:\\Downloads\"\n    tidyfs remove-empty \"D:\\Downloads\" --dry-run\n    tidyfs flatten-single-chain \"D:\\Photos\" --apply --mode keep-endpoints"
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "读取 tidyfs TOML 配置文件。未指定时会尝试读取当前目录下的 tidyfs.toml。"
    )]
    pub config: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "扫描空文件夹。", help_template = HELP_TEMPLATE)]
    ScanEmpty(ScanArgs),
    #[command(
        about = "删除空文件夹。默认只预览，传 --apply 才会真正删除。",
        help_template = HELP_TEMPLATE
    )]
    RemoveEmpty(ApplyArgs),
    #[command(about = "检测可拉平的目录链。", help_template = HELP_TEMPLATE)]
    DetectChains(ChainDetectArgs),
    #[command(
        about = "执行目录拉平。默认只预览，传 --apply 才会真正执行。",
        help_template = HELP_TEMPLATE,
        after_help = "默认模式说明:\n    keep-endpoints  按日期目录分组，保留日期内第一层，去掉更深壳目录\n    one-level       只提升一层\n    collapse-chain  一路压平到链起点"
    )]
    FlattenSingleChain(FlattenArgs),
}

#[derive(Debug, Args, Clone)]
pub struct ScanArgs {
    #[arg(help = "要处理的根目录。不传时会弹出文件夹选择框。")]
    pub path: Option<PathBuf>,
    #[command(flatten)]
    pub ignore: IgnoreArgs,
    #[arg(long, value_name = "PATH", help = "把输出同时写入日志文件。")]
    pub log_file: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
pub struct ApplyArgs {
    #[arg(help = "要处理的根目录。不传时会弹出文件夹选择框。")]
    pub path: Option<PathBuf>,
    #[command(flatten)]
    pub ignore: IgnoreArgs,
    #[arg(
        long,
        default_value_t = false,
        conflicts_with = "apply",
        help = "仅预览，不修改文件系统。"
    )]
    pub dry_run: bool,
    #[arg(long, help = "真正执行删除。")]
    pub apply: bool,
    #[arg(long, value_name = "PATH", help = "把输出同时写入日志文件。")]
    pub log_file: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
pub struct ChainDetectArgs {
    #[arg(help = "要处理的根目录。不传时会弹出文件夹选择框。")]
    pub path: Option<PathBuf>,
    #[command(flatten)]
    pub ignore: IgnoreArgs,
    #[arg(
        long,
        value_enum,
        help = "拉平规则。默认使用 keep-endpoints，也就是按日期目录分组，保留日期内第一层。"
    )]
    pub mode: Option<FlattenMode>,
    #[arg(long, value_name = "PATH", help = "把输出同时写入日志文件。")]
    pub log_file: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
pub struct FlattenArgs {
    #[arg(help = "要处理的根目录。不传时会弹出文件夹选择框。")]
    pub path: Option<PathBuf>,
    #[command(flatten)]
    pub ignore: IgnoreArgs,
    #[arg(
        long,
        default_value_t = false,
        conflicts_with = "apply",
        help = "仅预览，不修改文件系统。"
    )]
    pub dry_run: bool,
    #[arg(long, help = "真正执行拉平。")]
    pub apply: bool,
    #[arg(
        long,
        value_enum,
        help = "拉平规则。默认使用 keep-endpoints，也就是按日期目录分组，保留日期内第一层。"
    )]
    pub mode: Option<FlattenMode>,
    #[arg(long, value_name = "PATH", help = "把输出同时写入日志文件。")]
    pub log_file: Option<PathBuf>,
}

#[derive(Debug, Args, Clone, Default)]
pub struct IgnoreArgs {
    #[arg(
        long = "ignore-name",
        value_name = "NAME",
        help = "把指定文件名当作噪音文件忽略，例如 desktop.ini、Thumbs.db。可重复传入。"
    )]
    pub names: Vec<String>,
    #[arg(
        long,
        action = clap::ArgAction::Set,
        value_name = "BOOL",
        num_args = 1,
        help = "是否启用内置忽略名单：desktop.ini、Thumbs.db、.DS_Store。"
    )]
    pub default_ignored: Option<bool>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum FlattenMode {
    #[value(name = "keep-endpoints")]
    KeepEndpoints,
    #[value(name = "one-level")]
    OneLevel,
    #[value(name = "collapse-chain")]
    CollapseChain,
}
