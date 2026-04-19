# Phase M3 · Collectors — Claude + Codex

两家主采集器 + fixture 驱动的 sidecar 测试；共 6 个 commit，累计 4–5 小时。

## 目标

- Claude Code（`~/.claude/projects/**/*.jsonl`）与 Codex（`~/.codex/sessions/**/*.jsonl`）均可增量扫描入库
- 建立可复用的 `Collector` trait、`Reporter` 进度通道、共享解析工具

## 范围

- **做**：`Collector` trait / `Reporter` + `NoopReporter` + `ChannelReporter` / `util::{has_tool_result_block, is_real_user_prompt}` / Claude collector / Codex collector / fixture 驱动的集成测试
- **不做**：OpenClaw / OpenCode / pipeline 调度 / TUI 进度展示

## 验收

- [ ] `cargo test --test collector_claude_test` / `--test collector_codex_test` 全绿
- [ ] fixture 重跑 10 次，DB 行数不变（幂等）
- [ ] fixture 里 streaming chunk（无 usage 的 assistant）与 `<synthetic>` 被跳过
- [ ] Codex 的 `input_tokens = 源值 - cached`（非重叠语义）

## Commits

- **C1 · ✨ feat(collector): Collector trait 与进度通道** — `trait Collector: Send + Sync` + `ScanSummary` + `trait Reporter` + `NoopReporter` + `ChannelReporter`
- **C2 · ✨ feat(collector): 共享 JSONL 解析工具** — `util.rs`：`has_tool_result_block` / `is_real_user_prompt` / `read_jsonl_from_offset`（懒迭代，大文件友好）；sidecar 单测覆盖 string / array / tool_result 三种 content 形态
- **C3 · ✨ feat(collector/claude): Claude JSONL 采集器** — `ClaudeCollector`；默认路径 `~/.claude/projects` + `~/.config/claude/projects`（XDG 兼容）；按 file_state offset 增量读；区分 真实 user prompt / tool_result / streaming chunk / `<synthetic>` 模型
- **C4 · ✅ test(collector/claude): fixture + 幂等回归** — `tests/fixtures/claude/simple.jsonl`（3 真实 prompt + 1 tool_result + 3 assistant usage + 1 streaming + 1 synthetic）；跑 `full_scan` / `idempotent` 两个用例
- **C5 · ✨ feat(collector/codex): Codex JSONL 采集器** — `CodexCollector`；处理 `session_meta` / `turn_context` / `response_item(user)` / `event_msg.token_count` 四种 type；关键修正 `input_tokens -= cached_input_tokens`；scan_context 持久化 session_id + cwd + version + model
- **C6 · ✅ test(collector/codex): fixture + 非重叠语义回归** — fixture 覆盖 session_meta / turn_context / user prompt / token_count；断言 `input_tokens = 70`（源值 100 - cached 30）；验证分两段写入文件时的增量续扫正确性

## 执行时拆分提示

| commit | 拆分触发条件 | 建议子拆分 |
|---|---|---|
| C3 | 增量骨架 + 消息解析 > 200 行 | C3a 扫描骨架 + 路径探测 / C3b 消息解析 + token 规整 |
| C5 | 四种消息类型处理 > 200 行 | C5a `session_meta + turn_context`（scan_context 建立） / C5b `response_item + token_count`（记账） |
