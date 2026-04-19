# AGENTS.md — 项目契约

给每个会来触碰这个代码库的人（人类 & AI agent）看的短契约。先读这一页，再写代码。

> `agent-token-usage-tui` 是一个跨平台终端 UI，聚合 Claude Code / Codex / OpenClaw / OpenCode 的本地会话记录，用 litellm 价格表算出 token 用量与费用，全部 portable 落在 exe 同目录。

## 目录与模块

```
src/
├── main.rs                  # 薄壳：dispatch 到 cli::run
├── lib.rs                   # 公共 re-export
├── cli.rs                   # clap 子命令与派发
├── app_dir.rs               # portable 目录探测（所有运行时路径的源头）
├── config.rs                # TOML 配置（M5 C6 起填充字段）
├── logging.rs               # tracing 订阅器（M7 C1 加按天滚动）
├── domain/                  # 数据模型（M2 C1）
├── storage/                 # SQLite + schema + CRUD（M2 C2–C5）
├── pricing/                 # litellm 同步 + 费用公式（M2 C6–C7）
├── collector/               # Claude / Codex / OpenClaw / OpenCode 采集器（M3–M4）
└── tui/                     # ratatui + crossterm（M5–M6）
```

详细里程碑与 commit 粒度见 `plans/phase[1-7]-*-commits.md`。

## 硬约束（不可违反）

### 文件大小

* **500 LoC 软上限** — 超过请考虑拆子模块。
* **800 LoC 硬上限** — 超过必须拆，PR/commit 里解释拆法。
* `.rs` 文件可选择 sidecar 测试模式把测试分出去（见下节），主文件更容易保持 ≤ 500。

### Sidecar 测试模式

参考 Codex 实践：**foo.rs ↔ foo_tests.rs**，而不是内嵌 `mod tests {}`。

`foo.rs` 末尾：

```rust
#[cfg(test)]
#[path = "foo_tests.rs"]
mod tests;
```

兄弟文件 `foo_tests.rs`：

```rust
use super::*;
use pretty_assertions::assert_eq;

#[test]
fn case_1() { /* ... */ }
```

好处：主文件短、测试独立 grep、PR review 只需看改动那边。

### Clippy / 格式

* **workspace lints 配置在 `Cargo.toml` 的 `[lints.clippy]`**，14 条 deny（见文件注释）。
* `unwrap_used` / `expect_used` 生产代码 deny；**测试代码通过 `clippy.toml` 豁免**。
* `print_stdout` / `print_stderr` 一律 deny — 用 `tracing::info!` / `tracing::warn!`，实在要写 stdout 就 `writeln!(std::io::stdout().lock(), ...)`。
* `rustfmt.toml` 里 `imports_granularity = "Item"` 是 nightly-only：日常 `cargo fmt` 跑 stable 会 warn 但不 fail；想统一 import 风格定期跑一次 `cargo +nightly fmt`。

### 错误处理

* 库层用 `thiserror` 派生具体错误枚举；应用层用 `anyhow`，通过 `Context` 附加上下文。
* 主入口 `main.rs` 打印 `{err:#}`（anyhow 的 `Display:#` 展开 cause chain）。
* `?` 是推荐写法；`.unwrap()` / `.expect()` 只在测试和证明过的不变式里用。

## 提交规范

中文 Conventional Commits（详见全局 user rule / `plans/phase[1-7]-*-commits.md` 的示例）：

```
<emoji> <type>(<scope>): <主题>

- 改动点 1
- 改动点 2

Why:
- 关键取舍 1
- 关键取舍 2
```

### Type 与 emoji 对照

| Type | Emoji | 适用 |
|---|---|---|
| `feat` | ✨ | 新功能 |
| `fix` | 🐛 | 缺陷修复 |
| `docs` | 📝 | 文档 |
| `style` | 🎨 | 格式 / 空白 |
| `refactor` | ♻️ | 不改行为的结构调整 |
| `perf` | ⚡️ | 性能 |
| `test` | ✅ | 新增 / 更新测试 |
| `chore` | 🔧 | 工具链 / 维护 |
| `ci` | 👷 | CI/CD |
| `build` | 📦 | 构建系统 / 依赖 |
| `revert` | ⏪ | 回滚 |

### 粒度铁律

* **每个 commit `cargo build` 通过** — 不交半截。
* **依赖 / 业务 / 测试 / 文档 分别独立 commit** — review 友好。
* **Cargo.toml 的依赖改动**尽量集中在 phase 开头，后续 commit 不反复增删。
* **大 commit 允许就地拆子任务**：工作量 > 2h / > 200 行实质代码 / 多个独立子步骤时必须拆，标题统一在原 scope 下编号 `Cna / Cnb / Cnc`。

## 测试约定

* 使用 `pretty_assertions::assert_eq` 代替默认 `assert_eq`，diff 更清楚。
* 优先整对象比对，而非逐字段断言。
* fixture 放 `tests/fixtures/<source>/<case>.jsonl`，保证可复现。
* 所有 `pub` 模块至少 1 个单元测试；关键逻辑（去重、模糊匹配、非重叠语义）必须有回归测试。

## 禁用模式

* 不往 `%APPDATA%` / `~/.config` / `~/.local/share` 写数据 — portable 模式是产品形态。
* 不启后台 watcher — 用户按 `r` 手动重扫。
* 不开 HTTP server / Web UI。
* 不锁死 Rust 特定 patch 版本（`rust-toolchain.toml` 只写 `channel = "stable"`）。

## 快速命令

```bash
cargo build                                    # dev
cargo build --release                          # 瘦身 release（~15 MB）
cargo test                                     # 全量单测
cargo clippy --all-targets -- -D warnings      # lint 闸门
cargo fmt --check                              # 格式闸门
cargo run -- version                           # 版本号
cargo run -- scan                              # 一次性扫描（TUI 外）
cargo run                                      # 进 TUI（M5 C6 后可用）
```

## Phase 2 预告

* **Windsurf 采集** — Windsurf 的 cascade trajectory 不落盘，MVP 无法直接读。Phase 2 会提供配套 VSCode 扩展定时把 trajectory dump 到 `<exe-dir>/windsurf-sessions/*.jsonl`，`collector::windsurf` 以文件方式消费。

## 更多文档

* 架构全貌：`plans/agent-token-usage-tui-architecture-77d40b.md`
* Phase 1–7 commit 清单：`plans/phase[1-7]-*-commits.md`
