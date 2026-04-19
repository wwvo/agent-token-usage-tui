# Phase M1 · 项目骨架

创建空项目骨架，`cargo run` 能打印 `agent-token-usage-tui 0.1.0`，所有后续模块有占位文件可填；顺便落地 Codex 借鉴的严格 lints、rustfmt 规范、sidecar 测试约定、AGENTS.md 契约。共 7 个 commit，累计 2.5–3.5 小时。

## 目标

- 可编译运行的最小骨架 + 所有模块占位
- 严格的代码质量基线（Clippy deny 列表 + 禁用 print_stdout/stderr）
- 显式的代码规范文档（AGENTS.md + rustfmt.toml）

## 范围

- **做**：Cargo.toml 元信息与依赖 / rustfmt.toml / rust-toolchain / .gitignore / .editorconfig / Clippy workspace lints / `app_dir` + `logging` + `cli` 三个底层模块 / 其他模块 `mod` 占位 / AGENTS.md / 起步 README
- **不做**：业务逻辑 / DB schema / pricing 拉取 / TUI 渲染

## 验收

- [ ] `cargo fmt --check` / `cargo clippy --all-targets --all-features -- -D warnings` / `cargo test` / `cargo build --release` 全绿
- [ ] `cargo run` 打印 `agent-token-usage-tui 0.1.0`
- [ ] `cargo run -- scan` / `sync-prices` 分别 `todo!()` panic
- [ ] `Cargo.toml` 的 `[lints.clippy]` 含 `print_stdout = "deny"` + `print_stderr = "deny"`，移除后 `cargo clippy` 对任何 `println!` / `eprintln!` 直接报错
- [ ] `AGENTS.md` 落地并在 README 中链接

## Commits

- **C1 · 🔧 chore: 初始化 Rust 项目骨架** — `Cargo.toml`（`edition = "2024"` + `rust-version = "1.85"`）、`rust-toolchain.toml`（`channel = "stable"`，需 ≥ 1.85）、`rustfmt.toml`（`imports_granularity = "Item"` + `edition = "2024"`）、`.gitignore`、`.editorconfig`、最简 `src/main.rs` 仅打印版本
- **C2 · 📦 build(deps): 锁定运行期与构建期依赖** — 补 `[dependencies]`（ratatui / crossterm / rusqlite / tokio / reqwest / serde / clap / tracing / ...）与 `[build-dependencies]`、`[profile.release]`（lto=fat / strip / codegen-units=1 / panic=abort）、`[dev-dependencies]` 加 `pretty_assertions` + `tempfile`
- **C3 · 🔧 chore(lints): 严格 Clippy 规则与 print 禁用** — Cargo.toml 新增 `[lints.clippy]` 的 deny 列表（`unwrap_used` / `expect_used` / `uninlined_format_args` / `redundant_closure_for_method_calls` / `needless_borrow` / `manual_clamp` / `large_enum_variant` / `print_stdout` / `print_stderr` 等 15–20 条 codex 同款）；测试中允许 `unwrap` / `expect` 通过 `clippy.toml` 豁免
- **C4 · ✨ feat(app_dir): portable 目录探测** — `src/app_dir.rs` 导出 `exe_dir / config_path / db_path / log_dir / pricing_cache_path`；sidecar 测试 `src/app_dir_tests.rs`
- **C5 · ✨ feat(logging): 初版 stderr tracing 订阅器** — `src/logging.rs` 导出 `init(LogMode)`；`LogMode::File` 暂 `unimplemented!()` 留给 M7
- **C6 · ✨ feat(cli): clap 子命令骨架** — `src/cli.rs` 定义 `Cli / Command`（Scan / SyncPrices / Version）；`src/main.rs` 薄壳；补齐所有模块占位（domain / storage / pricing / collector / tui / config）
- **C7 · 📝 docs: AGENTS.md 项目契约与起步 README** — `AGENTS.md`（文件上限 500 软 / 800 硬 LoC、Sidecar 测试模式、禁用 API 清单、提交规范指向 user rule）；`README.md`（一句话介绍 + 支持的 agent + 编译运行 + 链接 AGENTS.md）

## 执行时拆分提示

本 Phase 所有 commit 预计单个 < 30 分钟、< 100 行，**不预期拆分**。
