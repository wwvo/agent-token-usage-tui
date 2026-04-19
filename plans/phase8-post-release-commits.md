# Phase M8 · 发布后的四个 Sprint

`v0.1.0` 已上线 cnb.cool。本文把 CHANGELOG 里的 known limitations、架构
§13 的 Windsurf Phase 2、Phase7 未完成的 C7 发布 workflow、以及 Phase7 未
完成的 C5 justfile 拆成四个互相独立的 Sprint；每个小任务完成即 commit，
便于回滚与评审。

估时合计 **2-2.5 天**。

---

## 调研结论（归档，避免重查）

### 1. cnb.cool CI 形态

- 配置文件：仓库根 `.cnb.yml`
- 结构：`<branch>.<event>: - [pipeline...]`
- 常用触发事件：`push` / `pull_request` / `tag_deploy.<name>`
- Runner：**Docker 容器，只 Linux**；通过 `runner.tags` 选架构
  - `cnb:arch:amd64` / `cnb:arch:arm64:v8`
- **macOS / Windows 原生 runner 不支持** → 要么在 Linux 交叉编译
  （`cargo xwin` / `cargo zigbuild`），要么走 GitHub Actions 镜像仓库
- 文档：<https://docs.cnb.cool/zh/build/grammar.html>

### 2. Windsurf cascade API（源：`C:\Users\Administrator\code\windsurf-token-usage\src\api.ts`）

- 凭证获取：
  1. `vscode.extensions.getExtension("codeium.windsurf").exports.devClient()`
  2. Monkey-patch `http.ClientRequest.prototype.{end,write}` 截获
     `x-codeium-csrf-token` + `host:{port}`
- 调用：`POST http://127.0.0.1:{port}/exa.language_server_pb.LanguageServerService/{Method}`
  - 头：`x-codeium-csrf-token: {csrf}` + `Connect-Protocol-Version: 1`
- 关键 RPC：
  - `GetAllCascadeTrajectories({include_user_inputs: false})`
    → `{trajectorySummaries: Record<cascadeId, TrajectorySummary>}`
  - `GetCascadeTrajectorySteps({cascade_id})` → `{steps: Step[]}`
- 用量字段：`step.type === "CORTEX_STEP_TYPE_USER_INPUT"`；
  `step.metadata.responseDimensionGroups[*].dimensions[*]` 里 `uid` 为
  `input_tokens` / `output_tokens` / `cached_input_tokens` 的
  `cumulativeMetric.value`
- 模型：`step.metadata.requestedModelUid ?? summary.lastGeneratorModelUid`
- 激活条件：`vscode.env.appName.includes("windsurf")`

### 3. ratatui 0.29 滚动与分页

- `TableState { selected, offset }` 已能自动跟随 selected 滚动
- 新增 `Scrollbar` + `ScrollbarState::new(len).position(offset)` 右侧
  垂直条；渲染到 `area.inner(Margin { vertical: 1, horizontal: 0 })`
- 0.29 新方法：`scroll_up_by(n)` / `scroll_down_by(n)` / `scroll_{right,left}_by`
- PageUp / PageDown：`page_size = area.height.saturating_sub(2) as usize`
- Scrollbar 必须 `content_length` 非零才渲染

---

## Sprint A · TUI Polish（估 3-5 h，0 新依赖，先做）

### A1 · ✨ feat(tui/sessions): 滚动条 + PageUp/PageDown + 放宽行数

- `src/tui/app.rs`
  - `SESSIONS_PAGE: 200` → `2000`（保留上限避免首次 fetch 无限增大）
  - `App` 加 `sessions_scroll: ScrollbarState`（按需重建也行，但显式字段
    更清晰）
  - `on_key`：`PageDown` / `PageUp` 按视口高度滚；`Home` / `End` 复用
    已有 `g` / `G`
- `src/tui/render.rs`
  - `draw_sessions` 在 Table 右侧叠 `Scrollbar::new(VerticalRight)`；
    同步 `scroll.position(selected_sessions)`
- 测试：
  - PageDown / PageUp 在大列表上跳跃 = 视口高度
  - 2000 行 `fetch + refresh` 不 panic

### A2 · ✨ feat(tui/models): Enter drill-down 到 Sessions 按 model 过滤

- `src/storage/queries.rs`
  - 新 enum `SessionFilter { All, BySource(Source), ByModel(String) }`
  - `fetch_recent_sessions(filter, limit)` 合并现有两签名
  - 或新增独立方法 `fetch_recent_sessions_by_model(&str, limit)`（更
    小改动），看后续是否还会扩
- `src/tui/app.rs`
  - Models 视图下 `Enter` → 按高亮行 `ModelTally::model` 过滤进 Sessions
  - 新增 footer `"filter: model={model}"`
- 测试：
  - queries：model 过滤只返回命中行；未知 model 空结果
  - app：models → Enter → View::Sessions + footer 带 "filter:"

### A3 · 🎨 style(tui): NO_COLOR 降级 + panic hook 清屏

- `src/tui/render.rs`
  - `fn no_color() -> bool { std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) }`
  - `fn color(c: Color) -> Color { if no_color() { Color::Reset } else { c } }`
  - 所有 `.fg(Color::X)` 改走 helper；`Style::default().add_modifier(...)`
    不动（`Modifier::BOLD` 等 NO_COLOR 下仍有价值）
- `src/tui/mod.rs`（或 `tui::run` 入口）
  - `std::panic::set_hook(Box::new(|info| { disable_raw_mode(); LeaveAlternateScreen;
default_hook(info) }))`
  - 在 `run` 开头 set，`run` 出错 / 正常返回前复原默认 hook（否则会污染
    非 TUI 子命令）
- 测试：
  - `no_color()` 单测（set_var / remove_var；std::env::set_var 要用
    `#[serial_test]` 或跑前后手动复原）
  - panic hook 可选不测（直接测难且噪声）

---

## Sprint B · justfile + CLI --help 对齐（估 1-2 h）

### B1 · 🔧 chore: justfile 集中常用命令

- 新文件 `justfile` 根目录
- recipes：
  - `default` — list 所有 recipe
  - `fmt` / `fmt-check`
  - `clippy` — `cargo clippy --all-targets -- -D warnings`
  - `test` / `test-quiet`
  - `run *ARGS` — `cargo run -- {{ARGS}}`（传参透传）
  - `doc` — `cargo doc --no-deps`
  - `ci` = `fmt-check` + `clippy` + `test` + `doc`（等价 CI pipeline）
  - `release` — `cargo build --profile dist`
- README `## Development` 节加一行指向 `just`

### B2 · 📝 docs(cli): --help 文案审校 + 用例

- `src/cli.rs` 每个子命令 `#[command(long_about = "...")]` 加 2-3 行
  example（`Examples:` section）
- `Cli` 顶层 `long_about` 写项目一句话定位 + portable 约束 + config 默认
  路径
- 英文为准（clap 的 `--help` 默认英文），长描述要能让 `atut scan --help`
  本身即可读

---

## Sprint C · CI / Release（估 2-3 h）

### C1 · 👷 ci(cnb): push/PR 触发 fmt + clippy + test

- 新文件 `.cnb.yml`
- 两条 pipeline：
  - amd64 lint+test：`docker.image: rust:1.85-slim` → cache cargo →
    `apt-get install -y libssl-dev pkg-config git` → `cargo fmt -- --check`
    → `cargo clippy --all-targets -- -D warnings` → `cargo test`
  - arm64 test（只 test，跳 clippy 避免重复跑）：`runner.tags: cnb:arch:arm64:v8`
- 环境变量 `AGENT_TUI_DISABLE_LITELLM_DOWNLOAD=1` 全局

### C2 · 👷 ci(cnb): tag 触发产出 Linux amd64+arm64 release 产物

- 追加 `tag_deploy.release` 到 `.cnb.yml`
- stages：
  - `cargo build --profile dist --locked`
  - `strip target/dist/atut`（Linux 上 double-check strip 生效）
  - `tar czf atut-<tag>-<arch>-linux.tar.gz atut README.md LICENSE CHANGELOG.md`
  - `sha256sum *.tar.gz > SHA256SUMS`
  - `cnb:upload` 到 release artifact（或 cnb 产物仓库，看平台文档）
- 放弃 macOS / Windows 交叉编译（维护成本高、收益有限）—— Sprint C3 作为可选

### C3 · (可选) 👷 ci(release): GitHub Actions 跨 OS 发布

- **前置**：需要在 GitHub 建一个镜像（`github.com/<user>/agent-token-usage-tui`）
  - cnb push 时同步
- `.github/workflows/release.yml` matrix：
  - `x86_64-pc-windows-msvc`
  - `x86_64-apple-darwin` + `aarch64-apple-darwin`
  - `x86_64-unknown-linux-gnu`（冗余，但给 GitHub 用户用）
- tag 触发；产物上传到 GitHub Release
- **本 Sprint 的 go/no-go 由用户决定**；若 no-go，跳过 C3

---

## Sprint D · Windsurf Phase 2（估 1-1.5 天）

**目录约定**：同仓库子目录 `tools/windsurf-exporter/`，避免另立 repo；
发布时给 `.vsix`。

### D1 · 📦 feat(windsurf-exporter): VSCode 扩展脚手架

- 新目录 `tools/windsurf-exporter/`
- 文件：
  - `package.json`（name: `agent-token-usage-tui-windsurf-exporter`，
    engines.vscode `^1.85.0`，activationEvents `onStartupFinished`，
    无 dependencies；devDeps: `@types/node` `@types/vscode` `typescript`）
  - `tsconfig.json`（ES2022 / NodeNext / strict）
  - `.vscodeignore` + `.gitignore`
  - `README.md`（说明定位：只落盘，不做展示）
  - `src/extension.ts` 空 activate/deactivate

### D2 · ✨ feat(windsurf-exporter): CSRF + devClient + RPC

- `src/api.ts` 从 `windsurf-token-usage/src/api.ts` 裁剪移植：
  - `getCredentials` / `clearCredentials` / `extractCsrf` / `httpPost`
  - `apiCall(creds, method, body)`
  - `listCascades()` → `GetAllCascadeTrajectories`
  - `fetchCascadeSteps(cascadeId)` → `GetCascadeTrajectorySteps`
- 只导出两个 public fn；pricing 表不移植（TUI 侧已有 litellm 管价格）
- `src/types.ts` 复用 `WindsurfCredentials` / `TrajectorySummary` /
  Step 子字段接口

### D3 · ✨ feat(windsurf-exporter): JSONL writer (Codex-like)

- `src/writer.ts`
- 输出目录：优先 `process.env.ATUT_WINDSURF_SESSIONS_DIR`；缺省
  `<homedir>/.atut/windsurf-sessions/`（不能用 exe-dir，扩展不在 atut exe
  旁；改为让 TUI 配置里指向这个固定目录）
- 文件：每 cascadeId 一个 `<cascadeId>.jsonl`
- 行类型（我们自定义，`collector/windsurf.rs` 成对实现）：
  - `{ "type":"session_meta", "cascade_id":"...", "created_time":"...",
"summary":"...", "last_model":"...", "workspace":"..." }`
  - `{ "type":"turn_usage", "step_id":"...", "timestamp":"...",
"model":"...", "input_tokens":N, "output_tokens":N,
"cached_input_tokens":N }`
- 幂等：写前读旧文件一遍，收集 step_id set，跳过已存在的
- 原子写：append-only，`fs.appendFileSync` + trailing `\n`

### D4 · ✨ feat(windsurf-exporter): 激活 + 轮询 + status bar

- `src/extension.ts`
  - 检测 `vscode.env.appName.includes("windsurf")`，非 Windsurf 直接
    `return`
  - activate 后 `setTimeout 8000 → refresh()`；然后 `setInterval 60_000`
  - `refresh()`：`listCascades` → 逐个 `fetchCascadeSteps` → writer 写
    incremental 行 → 更新 status bar `$(pulse) atut: {N} cascades`
  - deactivate 清 interval
- commands: `atut-windsurf-exporter.export-now`（手动触发）
- `package.json` contributes.commands 相应声明

### D5 · ✨ feat(collector/windsurf): 读 JSONL 入库替换占位

- `src/collector/windsurf.rs` 全量重写
  - walk `windsurf_bases[*]/*.jsonl`（若 config 未配，fallback
    `<homedir>/.atut/windsurf-sessions/`）
  - 按行 parse JSON：`session_meta` → upsert SessionRecord；
    `turn_usage` → insert UsageRecord（`source=Source::Windsurf`）
  - 增量：file_state `last_offset` 记录文件字节偏移
  - error：单行 parse 失败 → `warn!` + 计入 `ScanSummary::errors`，不
    abort
- `src/config.rs` 加 `windsurf_bases: Vec<PathBuf>`（已有字段，确认即可）

### D6 · ✅ test(collector/windsurf): fixture 测试

- `tests/fixtures/windsurf/cascade-a.jsonl` / `cascade-b.jsonl`
- 单测 + 集成测试：
  - 解析 2 cascade → 2 session + N usage rows
  - 再跑一次：0 new rows（幂等）
  - 追加行后再跑：只 ingest 新行

### D7 · 📝 docs: README + CHANGELOG 更新

- README「Windsurf」章节从“Phase 2 计划”升级为“Install the VSCode
  exporter”，附 `.vsix` 打包命令
- `CHANGELOG.md` 加 `[Unreleased]` section：feat Windsurf exporter +
  collector 接入
- `tools/windsurf-exporter/README.md` 写安装 / 配置 / FAQ

---

## 执行顺序建议

先 A（Polish，零新依赖、马上见效）→ B（justfile，开发体验） → C1/C2
（cnb CI，长期价值）→ D（最大块，独立推进）→ C3（如果要做 GitHub
镜像，最后做）。

## 约束复查

| 约束                                                               | 保持                           |
| ------------------------------------------------------------------ | ------------------------------ |
| 每个子任务一个 commit                                              | 是，共 20 个 commit            |
| 每次改动跑 `cargo fmt` + `cargo clippy -D warnings` + `cargo test` | 是                             |
| 每 commit message 遵 emoji + type(scope): + body + Why             | 是                             |
| 不改动现有测试以迁就新代码                                         | 是，除非 API 语义本身变了      |
| 仅在 TUI 里新写 stderr 裸输出（用户已启用的规则）                  | 是，`startup_ui.rs` 是唯一例外 |
