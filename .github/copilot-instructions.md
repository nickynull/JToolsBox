# Copilot 说明

本仓库的 AI 协作上下文见根目录 [AGENTS.md](../../AGENTS.md)。写代码前请先读它，重点：

- 桌面端 `src/` 与文件分享网页端 `web/file-sharing/` 是两套独立前端，不要互相共享代码。
- `web/*/dist` 是编译期嵌入的产物（`build.rs` → `web_assets.rs`），不入库，改动后需重新编译。
- 配置与数据在 exe 同级（`config.json`、`function-datas/`），不放 AppData。
- 版本号由 release-please 维护，提交信息遵循 Conventional Commits。
