# 更新日志

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [0.2.0] - 2026-10-05

### 新增
- 基于 Ink 的全新终端界面：多选磁盘和文件夹、实时进度、结果预览、导出完整清单。
- 多磁盘并行：同一磁盘顺序处理，不同磁盘并行处理。
- `tidyfs serve` JSON Lines 引擎协议，供界面调用。
- `tidyfs drives` 列出本机磁盘。
- 只含隐藏文件的文件夹也视为空文件夹（`hidden_files_are_noise`，默认开启）。
- 自动跳过系统目录、`.git`、`node_modules`、`AppData` 等，可用 `skip_dirs` 追加。
- 诊断日志（tracing / pino），按天滚动。

### 变更
- 扫描改为一次遍历建立内存目录树，每个目录只读一次，速度明显提升。
- 不再跟随符号链接和 Junction。
- 拉平执行时单项失败不再中断整批任务。
- 操作日志和诊断日志都写到程序部署目录（`journals\`、`logs\`），不再写进被处理的文件夹。
- 命令改名为 `empty` / `flatten`，旧命令名保留为别名，支持一次传多个路径。

## [0.1.0]

- 第一个版本：中文菜单、空文件夹清理、目录拉平。

[0.2.0]: https://github.com/kekincai/tidyfs/releases/tag/v0.2.0
