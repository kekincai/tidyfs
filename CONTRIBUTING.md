# 参与贡献

感谢你愿意改进 tidyfs！

## 开发环境

- Rust（stable，edition 2024）
- Node.js 22+

```bash
cargo build
npm --prefix ui ci
npm --prefix ui run build
node ui/dist/cli.js
```

## 提交前请确认

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
npm --prefix ui run lint
npm --prefix ui test
```

## 约定

- 会删除或移动文件的改动，请一定附带测试（`tempfile` 临时目录）。
- `src/domain` 只放业务规则，不直接依赖界面和协议；协议改动需要同时更新 `src/app/serve/protocol.rs` 和 `ui/src/engine/protocol.ts`。
- 界面配色只使用 `ui/src/components/theme.ts` 里的颜色。
- 提交信息用一句话说明“做了什么”，中英文都可以。
