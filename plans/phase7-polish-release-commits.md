# Phase M7 · 打磨 + 发布

日志按天滚动 + 错误边界收敛 + 版本信息 + justfile + 文档完善 + CI 发布 workflow；共 7 个 commit，累计 3.5–4.5 小时。

## 目标

- 生产级质量：错误全部 `anyhow::Error` 通过 / `unwrap/expect` 清零 / panic 不留坏终端
- 日志按天滚动到 `log/YYYY-MM-DD.log`，自动清理 > 30 天
- 完整 README（含截图）+ CHANGELOG + AGENTS.md 对齐
- justfile 集中日常命令（Codex 借鉴）
- GitHub Actions 跨平台 release workflow（含 `cargo-shear` 死依赖检测）

## 范围

- **做**：日志滚动 + 清理 / git commit + build time 注入 / `version` 子命令 / TUI unwrap 审计 + NO_COLOR 降级 / justfile / 完整 README 重写 + CHANGELOG / release workflow
- **不做**：CSV 导出 / cargo-deny 许可审计（开源后再加）/ Windsurf VSCode 扩展（Phase 2）

## 验收

- [ ] `target/release/agent-token-usage-tui.exe` ≤ 15 MB
- [ ] 连续运行 10 次，`log/` 下按日期命名且 > 30 天文件被清理
- [ ] 人为 `kill -9` TUI 后终端恢复
- [ ] `agent-token-usage-tui version` 输出 `v0.1.0 (abc1234) built 2026-04-19`
- [ ] `cargo clippy --all-targets -- -D warnings` 全绿、无 `unwrap_used` / `expect_used` 违规
- [ ] `just` 无参列出所有 recipe；`just ci` 一条命令跑完 fmt / clippy / test
- [ ] GitHub Actions matrix 构建 Windows / macOS / Linux × (x86_64 + arm64) 产 6 份 tar.gz

## Commits

- **C1 · ✨ feat(logging): 按天滚动与旧文件清理** — `LogMode::File` 实装：`tracing_appender::rolling::daily` 写 `log/YYYY-MM-DD.log`；启动时扫 `log/` 按文件名正则删 > 30 天；TUI 模式只 File，CLI 模式 File + stderr 双写
- **C2 · 📦 build: 注入 git commit 与 build time** — `build.rs` 调 `git rev-parse --short HEAD` 和 `chrono::Utc::now()` 写入 env（`GIT_HASH` / `BUILD_TIME`）；编译期 fallback 到 `"unknown"`
- **C3 · ✨ feat(cli): version 子命令** — 输出 `agent-token-usage-tui v0.1.0 (abc1234) built 2026-04-19T10:00:00Z`；`--help` 文案审校（中英文一致性）
- **C4 · 🎨 style(tui): unwrap 审计与 NO_COLOR 降级** — 全量 grep `unwrap()` / `expect()` 替换为 `?` 或带上下文的 `anyhow::Context`；确保 `NO_COLOR` 环境变量生效时所有 Color 转 `Color::Reset`；panic hook 增强（捕获后输出用户可读错误）
- **C5 · 🔧 chore: justfile 集中日常命令** — `justfile` recipes：`fmt` / `fmt-check` / `clippy` / `test` / `run` / `scan` / `build-release` / `ci`（= fmt-check + clippy + test + build-release）；`alias t := test` 等短别名；`[no-cd]` 配置工作目录
- **C6 · 📝 docs: 完整 README + CHANGELOG + 配置示例** — `README.md`（Features / Install / Quick Start / 配置示例 / ASCII 截图 / FAQ / Roadmap / Windsurf Phase 2 说明）；`CHANGELOG.md` 首条 `0.1.0 - YYYY-MM-DD`；`AGENTS.md` 回顾补充 M2-M6 过程中沉淀的项目约定
- **C7 · 👷 ci(release): 跨平台发布 workflow** — `.github/workflows/release.yml`：matrix 构建 `x86_64-pc-windows-msvc` / `x86_64-apple-darwin` / `aarch64-apple-darwin` / `x86_64-unknown-linux-gnu` / `aarch64-unknown-linux-gnu`；`cargo-shear` 预检；artifact 打 tar.gz + 附 SHA256；推到 GitHub Release（tag 触发）

## 执行时拆分提示

| commit | 拆分触发条件 | 建议子拆分 |
|---|---|---|
| C1 | 滚动 + 清理 + 双写模式 > 200 行 | C1a `rolling::daily` 接入 / C1b 30 天清理 + 双写路由 |
| C4 | unwrap 审计涉及 > 10 个模块 | C4a collector / storage / pricing 审计 / C4b tui + cli 审计 + NO_COLOR |
| C7 | matrix + `cargo-shear` + 签名 > 150 行 yaml | C7a matrix 构建 + artifact / C7b `cargo-shear` 预检 + SHA256 |
