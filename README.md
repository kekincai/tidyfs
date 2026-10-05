<div align="center">

# ◆ tidyfs

**清理空文件夹，拉平多层嵌套目录。多块磁盘同时扫描，一个终端界面搞定。**

[![CI](https://github.com/kekincai/tidyfs/actions/workflows/ci.yml/badge.svg)](https://github.com/kekincai/tidyfs/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/kekincai/tidyfs?display_name=tag&sort=semver)](https://github.com/kekincai/tidyfs/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Windows-0078D6)

</div>

```text
  ◆ tidyfs   选择位置 › 扫描 › 确认 › 执行 › 完成                                      v0.2.1
  ──────────────────────────────────────────────────────────────────────────────────────────

   清理空文件夹   拉平目录

  删除没有任何内容的文件夹。隐藏文件和 desktop.ini、Thumbs.db 这类噪音文件不算内容，会一起清理。

  位置 ─────────────────────────────────────────────────────── 已选 2 · 不同磁盘并行扫描
  › ● C:\     Windows         NTFS    ━━━━━━━━━━━━━━  215 GB 可用  476 GB
    ○ D:\     Data            exFAT   ━━━━━━━━━━━━━━  1.1 TB 可用  4.5 TB
    ● E:\     Elements        exFAT   ━━━━━━━━━━━━━━  949 GB 可用  4.5 TB
    ○ D:\Photos\2025                  文件夹

  ──────────────────────────────────────────────────────────────────────────────────────────
  ↑↓ 移动   空格 勾选   a 全选   Tab 切换功能   f 选择文件夹   p 粘贴路径   Enter 开始扫描
```

## 特点

- **多磁盘并行**：同一块磁盘上的目录按顺序扫描（单盘多线程读小目录反而更慢），不同磁盘各开一个线程同时跑。
- **快**：每个目录只读一次，只用目录项自带的类型信息，不对每个文件额外 `stat`；分析全在内存里完成。
- **先看再做**：扫描结果逐项预览、可导出完整清单，确认后才执行。
- **安全**：
  - 不跟随符号链接和 Junction；
  - 自动跳过 `Windows`、`Program Files`、`$Recycle.Bin`、`AppData`、`.git`、`node_modules` 等目录；
  - 单项失败（权限、占用）只记录，不中断整批；
  - 同名文件自动改名，不会覆盖；
  - 拉平不允许直接作用于整块磁盘；
  - 每次执行都写操作日志，方便核对。
- **现代终端界面**：基于 [Ink](https://github.com/vadimdemedes/ink)（React for CLI），能适应窄窗口；也保留了完整的命令行模式，方便写脚本。

## 安装

### 下载发布版（推荐）

到 [Releases](https://github.com/kekincai/tidyfs/releases) 下载 `tidyfs-windows-x64.zip`，解压后双击 `tidyfs.exe`。

交互界面需要 [Node.js 22+](https://nodejs.org/)。不装 Node 也可以直接用[命令行模式](#命令行)。

### 从源码构建

```bash
git clone https://github.com/kekincai/tidyfs.git
cd tidyfs
cargo build --release
npm --prefix ui ci
npm --prefix ui run build
```

然后双击 `target\release\tidyfs.exe`，或者运行仓库根目录的 `tidyfs.cmd`。

## 使用

### 交互界面

1. `Tab` 选择功能：**清理空文件夹** 或 **拉平目录**；
2. 用 `空格` 勾选一个或多个磁盘，也可以按 `f` 弹出文件夹选择框（可多选）、按 `p` 粘贴路径；
3. `Enter` 开始扫描，每个位置一行实时进度；
4. 在结果页用 `←→` 切换位置、`空格` 排除某个位置、`o` 导出完整清单；
5. `Enter` 后按 `y` 确认执行。

### 命令行

```bash
# 同时扫描两块磁盘上的空文件夹（只预览）
tidyfs empty D:\ E:\

# 真正删除
tidyfs empty D:\Downloads --apply

# 拉平目录
tidyfs flatten D:\Photos --mode keep-endpoints --apply

# 列出磁盘
tidyfs drives
```

不传路径时会弹出文件夹选择框。旧的命令名 `scan-empty`、`remove-empty`、`detect-chains`、`flatten-single-chain` 仍然可用。

## 规则说明

### 空文件夹

从深到浅判断，满足下面条件的文件夹算空：

- 没有普通文件（隐藏文件和噪音文件不算）；
- 子文件夹也全部是空文件夹；
- 里面没有符号链接、Junction 或被跳过的系统目录。

删除时会连同里面的隐藏文件、噪音文件一起删除。磁盘根目录本身永远不会被删除。

### 拉平目录

默认规则 `keep-endpoints`：选中目录下的每个日期目录（如 `20250203`）是一个整理单元，保留日期目录里的第一层，去掉更深的“壳目录”。

```text
20250203/project_alpha/archive_shell/video.mp4  →  20250203/project_alpha/video.mp4
20250101/a/b/x/y/c/photo.jpg                    →  20250101/a/photo.jpg
```

同一个壳目录下有多个末端文件夹时，保留末端文件夹名，避免把不同来源的文件混在一起：

```text
20250101/a/b/c/c.txt  →  20250101/a/c/c.txt
20250101/a/b/d/d.txt  →  20250101/a/d/d.txt
```

其它模式：

| 模式 | 效果 |
| --- | --- |
| `keep-endpoints` | 保留首尾（默认） |
| `one-level` | 只把最内层的文件提升一层 |
| `collapse-chain` | 单链目录一路压平到链的起点 |

中间壳目录里有普通文件、或结构更复杂时不会处理。最内层的隐藏文件会跟着一起移动，不会被删除。

## 配置

程序会读取当前目录或程序所在目录下的 `tidyfs.toml`，也可以用 `--config` 指定：

```toml
default_ignored = true          # 内置噪音文件名单：desktop.ini、Thumbs.db、.DS_Store
hidden_files_are_noise = true   # 只含隐藏文件的文件夹也算空文件夹
ignore_names = [".nomedia"]     # 额外的噪音文件名
skip_dirs = ["cache"]           # 额外跳过的目录名
flatten_mode = "keep-endpoints"
```

命令行参数优先于配置文件。

## 日志

| 内容 | 位置 |
| --- | --- |
| 操作日志：每次执行实际移动 / 删除了什么 | `tidyfs.exe` 所在目录下的 `journals\` |
| 诊断日志：扫描耗时、错误等，按天滚动保留 14 天 | `tidyfs.exe` 所在目录下的 `logs\`（引擎和界面日志都在这里） |

日志只写在程序的部署目录里，绝不写进被扫描、被处理的文件夹。如果部署目录没有写权限（比如放在 `Program Files` 下），会改写到 `%LOCALAPPDATA%\tidyfs\`。也可以用环境变量 `TIDYFS_HOME` 指定日志根目录。

诊断日志级别用环境变量 `TIDYFS_LOG` 控制，例如 `TIDYFS_LOG=debug`。命令行加 `-v` 会同时输出到终端。

## 架构

```text
┌──────────────────────────┐   JSON Lines (stdin/stdout)   ┌──────────────────────────────┐
│  ui/  Node.js · Ink      │ ────────────────────────────▶ │  tidyfs serve  (Rust 引擎)    │
│  界面、交互、pino 日志    │ ◀──────────────────────────── │  扫描 / 执行 / 磁盘枚举       │
└──────────────────────────┘                               └──────────────────────────────┘
```

```text
src/
├── domain/   纯业务逻辑：目录树、空目录分析、拉平规则、执行结果
├── engine/   多磁盘调度：按磁盘分组，组内顺序、组间并行
├── infra/    文件系统工具、磁盘枚举、配置、操作日志、诊断日志（tracing）
└── app/      命令行、控制台输出、JSON 服务、界面启动器
ui/src/
├── engine/   引擎子进程客户端与协议类型
├── screens/  选择位置 / 扫描 / 确认 / 执行 / 完成
└── components/  设计系统：配色、框架、进度条、路径显示
```

扫描结果保存在引擎进程里，界面只拿预览（每个位置最多 2000 条），执行时直接使用内存里的计划。

## 开发

```bash
cargo test                 # Rust 单元测试
cargo clippy --all-targets
npm --prefix ui run lint   # Prettier + TypeScript 检查
npm --prefix ui test       # 端到端界面测试（需要先 cargo build）
```

欢迎提 Issue 和 PR，详见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 许可证

[MIT](LICENSE)
