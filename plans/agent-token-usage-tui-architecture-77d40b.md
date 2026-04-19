# agent-token-usage-tui — 架构设计草案

用 Rust + ratatui 构建跨平台纯 TUI，portable 部署在 exe 同目录，采集 Claude Code / Codex / OpenClaw / OpenCode 四家的本地 JSONL/SQLite 会话，基于 litellm 价格表计算费用；Windsurf 不落盘，推到 Phase 2 用配套 VSCode 扩展导出 JSONL。

---

## 1. 项目元信息

| 项           | 值                                                  |
| ------------ | --------------------------------------------------- |
| 目录         | `C:\Users\Administrator\code\agent-token-usage-tui` |
| Cargo 包名   | `agent-token-usage-tui`                             |
| 可执行文件名 | `agent-token-usage-tui`（Windows 下 `.exe`）        |
| 版本策略     | 语义化 `0.1.0` 起步                                 |
| License      | Apache-2.0（与参考仓库一致）                        |
| 最低 Rust    | 1.85+（edition 2024）                               |
| 目标平台     | Windows / macOS / Linux，amd64 + arm64              |

---

## 2. 总体约束

- **纯 TUI**：不启 HTTP server，不嵌 ECharts
- **Portable**：数据 / 配置 / 日志 / 价格缓存**全部放 exe 同目录**，不碰 `%APPDATA%` / `~/.config`
- **零配置可跑**：`agent-token-usage-tui.toml` 不存在时使用默认路径自动探测
- **不做 watcher**：启动时全量扫描一次，TUI 里按 `r` 手动重扫
- **增量扫描**：基于 `file_state` 表里记录的 offset + scan_context，不重读已读部分
- **MVP 采集器 = 4 家**：Claude Code / Codex / OpenClaw / OpenCode；Windsurf 留 Phase 2
- **价格兜底**：`build.rs` 编译时嵌入一份 litellm JSON，离线也能算费用

---

## 3. 技术栈

| 关注点   | crate                                                   | 用途                      |
| -------- | ------------------------------------------------------- | ------------------------- |
| TUI 框架 | `ratatui` ≥ 0.30                                        | 组件、布局                |
| 终端后端 | `crossterm`                                             | Windows 原生事件          |
| SQLite   | `rusqlite`（`bundled` feature）                         | 无需系统 sqlite           |
| 异步     | `tokio`（`rt-multi-thread` + `fs` + `macros`）          | 并行扫描、HTTP            |
| HTTP     | `reqwest`（`rustls-tls`, `json`）                       | 拉 litellm 价格           |
| 序列化   | `serde` + `serde_json` + `toml`                         | JSONL / config            |
| CLI      | `clap` v4（derive）                                     | 子命令                    |
| 时间     | `chrono`（`serde`）                                     | RFC3339 解析              |
| 路径     | —（用 `std::env::current_exe()`，不用 `dirs`）          | portable 专用             |
| 日志     | `tracing` + `tracing-subscriber`（`fmt` + `EnvFilter`） | 文件日志                  |
| 错误     | `anyhow` + `thiserror`                                  | 应用 / 库分工             |
| 构建期   | `reqwest`（blocking, build-dep）                        | build.rs 拉 fallback JSON |

二进制目标体积：≤ 15 MB（release + strip + lto），含内嵌的 500KB 价格 JSON。

---

## 4. 目录结构

```
agent-token-usage-tui/
├── .gitignore
├── .editorconfig
├── Cargo.toml
├── build.rs                       # 编译期拉 litellm JSON 内嵌
├── README.md
├── src/
│   ├── main.rs                    # clap 入口
│   ├── lib.rs                     # re-export，便于测试
│   ├── cli.rs                     # 子命令定义
│   ├── app_dir.rs                 # portable 路径解析
│   ├── config.rs                  # TOML 读取 + 默认值 + 自动探测
│   ├── logging.rs                 # tracing-subscriber 初始化（写文件）
│   ├── domain/
│   │   ├── mod.rs
│   │   ├── record.rs              # UsageRecord
│   │   ├── session.rs             # SessionRecord
│   │   ├── prompt.rs              # PromptEvent
│   │   ├── price.rs               # ModelPrice
│   │   └── source.rs              # enum Source { Claude, Codex, OpenClaw, OpenCode, Windsurf }
│   ├── storage/
│   │   ├── mod.rs                 # pub struct Db
│   │   ├── schema.rs              # CREATE TABLE + 版本化 migration
│   │   ├── file_state.rs          # offset + scan_context
│   │   ├── records.rs             # InsertUsageBatch / InsertPromptBatch / UpsertSession
│   │   ├── pricing.rs             # UpsertPricing / GetAllPricing
│   │   ├── costs.rs               # RecalcCosts + matchPricing 模糊匹配
│   │   └── queries.rs             # TUI 查询（stats / cost_by_model / trend / sessions）
│   ├── pricing/
│   │   ├── mod.rs
│   │   ├── fallback.rs            # include_bytes! build.rs 产物
│   │   ├── sync.rs                # reqwest 拉 GitHub
│   │   └── cost.rs                # 费用计算公式
│   ├── collector/
│   │   ├── mod.rs                 # trait Collector { async fn scan(&self, db, reporter) }
│   │   ├── reporter.rs            # 扫描进度 channel（给 TUI 显示 progress）
│   │   ├── util.rs                # 共享：hasToolResultBlock / isRealUserPrompt 等
│   │   ├── claude.rs              # ~/.claude/projects/**/*.jsonl
│   │   ├── codex.rs               # ~/.codex/sessions/**/*.jsonl
│   │   ├── openclaw.rs            # ~/.openclaw/agents/<id>/sessions/*.jsonl
│   │   ├── opencode.rs            # ~/.local/share/opencode/opencode.db (只读)
│   │   └── windsurf.rs            # Phase 2 TODO — 仅放 trait 骨架 + 注释
│   └── tui/
│       ├── mod.rs                 # pub fn run(db)
│       ├── app.rs                 # AppState + event-loop（Elm 风格）
│       ├── event.rs               # crossterm → AppMsg
│       ├── theme.rs               # 颜色 / 符号
│       ├── widgets/
│       │   ├── tabs.rs
│       │   ├── summary_cards.rs
│       │   └── status_bar.rs
│       └── views/
│           ├── overview.rs        # BarChart + Sparkline + 4 卡片
│           ├── sessions.rs        # Table + 筛选 + 选中详情
│           ├── models.rs          # 按模型聚合的 BarChart + Table
│           └── trend.rs           # Chart（折线） + 粒度切换
├── assets/
│   └── litellm-prices.fallback.json   # build.rs 写入，.gitignore 排除
└── tests/
    ├── fixtures/
    │   ├── claude_sample.jsonl
    │   ├── codex_sample.jsonl
    │   └── openclaw_sample.jsonl
    ├── collector_claude_test.rs
    ├── collector_codex_test.rs
    ├── storage_dedup_test.rs
    └── pricing_match_test.rs
```

---

## 5. 模块职责（速览）

| 模块        | 职责                                                                                          | 关键点                                                                   |
| ----------- | --------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| `app_dir`   | 返回 exe 所在目录；提供 `config_path()` / `db_path()` / `log_path()` / `pricing_cache_path()` | `cargo run` 时自然落在 `target/debug/` — 这是 portable 正确行为          |
| `config`    | 读 TOML → `Config`；不存在时给默认；支持 `~` / env 展开                                       | 和 agent-usage 字段兼容但**用独立文件名**（见 §8）                       |
| `domain`    | 纯数据结构 + `Source` 枚举                                                                    | `UsageRecord` 字段 1:1 对齐 agent-usage schema                           |
| `storage`   | rusqlite 封装；`Arc<Mutex<Connection>>` 串行化所有写                                          | 迁移版本号记在 `meta` 表，与 agent-usage 同思路                          |
| `pricing`   | 启动 → 尝试 reqwest 拉最新 → 失败回退到内嵌 fallback → 写 DB                                  | fallback 通过 `include_bytes!("../assets/litellm-prices.fallback.json")` |
| `collector` | 每家一个 struct，实现 `async fn scan(&self, db, reporter) -> Result<ScanSummary>`             | 串行调度，progress 通过 `mpsc::Sender` 回报给 TUI                        |
| `cli`       | clap derive 结构，子命令派发                                                                  | 详见 §7                                                                  |
| `tui`       | Elm 架构；`AppState { tab, data, loading, error, ... }`                                       | 每个 tab 独立 view 模块，共享 `SharedData`                               |

---

## 6. SQLite Schema

参考 `@C:\Users\Administrator\code\agent-usage\internal\storage\sqlite.go:72-138`，直接沿用（不做修改以便未来如果反悔还能复用 agent-usage 的分析 SQL）：

```sql
CREATE TABLE usage_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    session_id TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cache_creation_input_tokens INTEGER DEFAULT 0,
    cache_read_input_tokens INTEGER DEFAULT 0,
    reasoning_output_tokens INTEGER DEFAULT 0,
    cost_usd REAL DEFAULT 0,
    timestamp DATETIME NOT NULL,
    project TEXT DEFAULT '',
    git_branch TEXT DEFAULT ''
);
CREATE UNIQUE INDEX idx_usage_dedup
    ON usage_records(session_id, model, timestamp, input_tokens, output_tokens);

CREATE TABLE sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    session_id TEXT NOT NULL UNIQUE,
    project TEXT DEFAULT '', cwd TEXT DEFAULT '',
    version TEXT DEFAULT '', git_branch TEXT DEFAULT '',
    start_time DATETIME, prompts INTEGER DEFAULT 0
);

CREATE TABLE prompt_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL, session_id TEXT NOT NULL,
    timestamp DATETIME NOT NULL
);
CREATE UNIQUE INDEX idx_prompt_dedup ON prompt_events(session_id, timestamp);

CREATE TABLE file_state (
    path TEXT PRIMARY KEY,
    size INTEGER DEFAULT 0,
    last_offset INTEGER DEFAULT 0,
    scan_context TEXT DEFAULT ''     -- JSON: {session_id, cwd, version, model}
);

CREATE TABLE pricing (
    model TEXT PRIMARY KEY,
    input_cost_per_token REAL, output_cost_per_token REAL,
    cache_read_input_token_cost REAL, cache_creation_input_token_cost REAL,
    updated_at DATETIME
);

CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT DEFAULT '');
```

---

## 7. CLI 设计

```
agent-token-usage-tui           # 默认：扫一次 + 进 TUI
agent-token-usage-tui scan      # 只扫不进 TUI（适合 cron）
agent-token-usage-tui sync-prices   # 只同步价格 + 回填费用
agent-token-usage-tui version   # 版本号 + commit + build time
agent-token-usage-tui --help
```

全局开关：

```
--config <PATH>   覆盖默认 TOML 位置
--data-dir <DIR>  覆盖 portable 目录（测试用）
--verbose / -v    写更多日志到 log 文件
--no-scan         进 TUI 前不扫（直接用现有 DB）
--no-prices       跳过 pricing sync（使用本地缓存）
```

---

## 8. Portable 路径约定

exe 所在目录 = `EXE_DIR`（通过 `std::env::current_exe()?.parent()?` 获取）：

| 文件     | 路径                           | 用途                             |
| -------- | ------------------------------ | -------------------------------- |
| 配置     | `EXE_DIR/config.toml`          | 可选，不存在走默认值             |
| 数据库   | `EXE_DIR/data.db`              | SQLite + WAL                     |
| 日志目录 | `EXE_DIR/log/<YYYY-MM-DD>.log` | 每天一个文件，启动时自动创建目录 |
| 价格缓存 | `EXE_DIR/pricing.json`         | 上次同步成功的副本               |

如果 `EXE_DIR` 无写权限（比如装在 `C:\Program Files`）→ 启动时报 `IoError` 并提示用户把 exe 移到可写目录。**不自动回退到 AppData**。

---

## 9. Token 语义（非重叠）

严格对齐 `@C:\Users\Administrator\code\agent-usage\AGENTS.md:48-64`：

```
input_tokens              — 非缓存输入（不含 cache 字段）
cache_read_input_tokens   — 缓存命中
cache_creation_input_tokens — 写缓存
output_tokens             — 输出总量
reasoning_output_tokens   — 推理 token（是 output 的子集，仅展示用）

total_input  = input + cache_read + cache_creation
total_tokens = total_input + output_tokens
```

Codex 源头原始是 `input_tokens` 含 cache，collector 里必须 `input -= cached` 后再写 DB（见 `@C:\Users\Administrator\code\agent-usage\internal\collector\codex.go:184`）。

费用公式（`pricing/cost.rs`）：

```
cost = input × input_price
     + cache_creation × cache_creation_price
     + cache_read × cache_read_price
     + output × output_price
```

---

## 10. TUI 布局（k9s 风格）

```
╭─ agent-token-usage-tui 0.1.0 ──────────────── 2026-04-19 09:53 ─╮
│  [1]Overview  [2]Sessions  [3]Models  [4]Trend                  │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ╭ Total Cost ╮  ╭ Tokens ╮  ╭ Sessions ╮  ╭ API Calls ╮        │
│  │   $12.34   │  │  1.2M  │  │   156    │  │  2,341    │        │
│  ╰────────────╯  ╰────────╯  ╰──────────╯  ╰───────────╯        │
│                                                                 │
│  Cost by Model ────────────────   Token Trend (7d) ──────────   │
│   sonnet-4.5  ████████  $4.80      ▁▂▅▇█▆▃                      │
│   opus-4      █████     $3.20                                   │
│   gpt-5-codex ███       $2.10                                   │
│   ...                                                           │
├─────────────────────────────────────────────────────────────────┤
│  Sources: Claude(123) · Codex(45) · OpenClaw(0) · OpenCode(12)  │
│  [r]efresh  [q]uit  [1-4]switch  [?]help                        │
╰─────────────────────────────────────────────────────────────────╯
```

键位（vim-like）：

| 键                | 行为                           |
| ----------------- | ------------------------------ |
| `1` `2` `3` `4`   | 切换 tab                       |
| `h` `l` / `←` `→` | 切换 tab（循环）               |
| `j` `k` / `↑` `↓` | 列表上下移                     |
| `g` `G`           | 跳到顶部 / 底部                |
| `/`               | 列表筛选（Sessions / Models）  |
| `[` `]`           | Trend 切换粒度（1h / 1d / 1w） |
| `r`               | 重新全量扫描                   |
| `R`               | 只重新拉价格（不扫文件）       |
| `?`               | 打开帮助浮层                   |
| `q` / `Ctrl-C`    | 退出                           |

---

## 11. Collector 调度流程

```
main
 └─ cli::run()
     ├─ logging::init()            # 写 app.log
     ├─ config::load()             # 可选 TOML
     ├─ Db::open_or_migrate()
     ├─ pricing::sync_or_fallback()  # 联网失败 → include_bytes! 兜底
     ├─ cli.subcommand match
     │   ├─ None  → run_scan + tui::run
     │   ├─ Scan  → run_scan
     │   ├─ Sync  → pricing::sync + recalc_costs
     │   └─ Ver   → print
     └─ run_scan:
         for c in [Claude, Codex, OpenClaw, OpenCode]:
             if cfg.enabled(c): c.scan(&db, &reporter).await
         storage::costs::recalc_costs(&db, &prices)
```

TUI 模式下 `run_scan` 在一个后台 tokio task 跑，`reporter` 把 `ScanProgress { source, files_done, files_total }` 通过 `mpsc` 推到 TUI 主循环，在状态栏或浮层里显示。

---

## 12. 错误处理 / 日志

- 顶层 `main` 返回 `anyhow::Result<()>`，失败时打印 `{err:#}`（带 cause chain）
- 库层用 `thiserror` 定义 `StorageError` / `CollectorError` / `PricingError`
- 单个 collector 失败 **不** 让 TUI 崩溃 — 记 `tracing::warn!` 继续下一家
- TUI 模式下 `stderr` 会破坏界面，所有日志用 `tracing-appender` 写到 `EXE_DIR/log/<YYYY-MM-DD>.log`（`rolling::daily`）
- CLI 非 TUI 子命令（scan / sync）日志同时输出 stderr
- 日志保留策略：默认保留最近 30 天，启动时自动清理旧日志

---

## 13. Windsurf Phase 2 方案（B：配套 VSCode 扩展导出器）

基于本次探测结论（见 `@C:\Users\Administrator\.windsurf\plans\agent-token-usage-tui-architecture-77d40b.md` §14）：Windsurf 的 cascade trajectory **不落盘**，仅存活于 Language Server 进程内存。

**方案**：fork / 新写一个轻量 VSCode 扩展 `agent-token-usage-tui-windsurf-exporter`：

1. 激活时用 `windsurf-token-usage` 的 CSRF hack 拿 port + token（`@C:\Users\Administrator\code\windsurf-token-usage\src\api.ts:111-212`）
2. 每 60 秒调 `GetAllCascadeTrajectories` + `GetCascadeTrajectorySteps`
3. 把结果**追加**写到 `EXE_DIR/windsurf-sessions/<cascadeId>.jsonl`，每行一条 `response_item` 或 `token_count` 事件（模仿 Codex JSONL 格式）
4. `agent-token-usage-tui` 的 `collector/windsurf.rs` 只读这个目录，复用 Codex-like 解析器

好处：

- 主 TUI 依旧是纯 Rust，零 VSCode runtime 依赖
- 扩展仅负责"把内存数据落盘"，逻辑简单
- Windsurf 没开时 → 老数据还在 JSONL 里，不影响看历史

开发量估计：扩展 ~300 行 TS，collector ~200 行 Rust，合计 1-2 天。

---

## 14. 里程碑

| Phase                    | 范围                                                   | 时间估   |
| ------------------------ | ------------------------------------------------------ | -------- |
| **M1 骨架**              | Cargo.toml + 目录 + 空模块编译通过 + main 打印 "hello" | 0.5 天   |
| **M2 Storage + Pricing** | Schema + migration + fallback JSON + sync              | 1 天     |
| **M3 Collectors 单测**   | Claude + Codex 两家 + fixtures 测试                    | 1.5 天   |
| **M4 Collectors 补全**   | OpenClaw + OpenCode + 集成                             | 1 天     |
| **M5 TUI Overview**      | Tabs 骨架 + Overview 视图 + 基础键位                   | 1 天     |
| **M6 TUI 其它三视图**    | Sessions / Models / Trend + 筛选                       | 1.5 天   |
| **M7 打磨**              | 错误处理 / 日志 / README / cargo release 配置          | 0.5-1 天 |
| **M8（可选）**           | Windsurf VSCode 导出器 + collector                     | +1.5 天  |

MVP（M1-M7）合计：**6-7 天**。

---

## 15. 待定默认（实现时再快速确认）

这些会以当前默认开工，但开发中任何一项如果你想改，随时说：

- **配置文件名**：`config.toml`（简洁）而不是 `agent-token-usage-tui.toml`（冗长）
- **数据库文件名**：`data.db`
- **日志文件名**：`log/YYYY-MM-DD.log`（每天一个文件，保留 30 天）
- **启动时是否默认扫描**：是（`--no-scan` 可跳过）
- **scan 进度**：TUI 底部状态栏显示 `Scanning claude: 12/45 files`
- **颜色方案**：深色为主，支持终端自动色彩回退（`NO_COLOR` 环境变量生效）
- **Claude / Codex / OpenClaw / OpenCode 默认路径**：和 agent-usage 完全一致
- **pricing 过期判断**：若 `updated_at` 距今 > 24h 则重拉
- **reqwest 超时**：30s
- **SQLite PRAGMA**：`journal_mode=WAL`, `busy_timeout=5000`
- **发布产物**：暂不集成 CI/GoReleaser（MVP 后补）

---

## 16. 参考源

| 参考                                                                              | 用于                                        |
| --------------------------------------------------------------------------------- | ------------------------------------------- |
| `@C:\Users\Administrator\code\agent-usage\internal\collector\claude_process.go`   | Claude JSONL 解析                           |
| `@C:\Users\Administrator\code\agent-usage\internal\collector\codex.go`            | Codex session_meta + turn_context 增量解析  |
| `@C:\Users\Administrator\code\agent-usage\internal\collector\openclaw_process.go` | OpenClaw message 解析                       |
| `@C:\Users\Administrator\code\agent-usage\internal\collector\opencode.go`         | OpenCode SQLite watermark                   |
| `@C:\Users\Administrator\code\agent-usage\internal\storage\sqlite.go`             | Schema + migration 模式                     |
| `@C:\Users\Administrator\code\agent-usage\internal\storage\costs.go`              | 模糊模型匹配（provider prefix + normalize） |
| `@C:\Users\Administrator\code\agent-usage\internal\pricing\pricing.go`            | litellm 拉取 + 费用公式                     |
| `@C:\Users\Administrator\code\windsurf-token-usage\src\api.ts`                    | Phase 2 VSCode 扩展的 CSRF/gRPC 逻辑        |
