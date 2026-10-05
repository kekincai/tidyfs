//! tidyfs：清理空文件夹、拉平多层嵌套目录。
//!
//! - `domain` 纯业务逻辑：目录树、空目录分析、拉平规则、执行结果
//! - `infra`  和外部环境打交道：文件系统工具、磁盘枚举、配置、操作日志、诊断日志
//! - `engine` 多磁盘调度：同一块磁盘顺序处理，不同磁盘并行
//! - `app`    入口层：命令行、控制台输出、给 Node 界面用的 JSON 服务、界面启动器
pub mod app;
pub mod domain;
pub mod engine;
pub mod infra;
