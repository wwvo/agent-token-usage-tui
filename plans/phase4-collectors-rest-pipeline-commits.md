# Phase M4 · OpenClaw + OpenCode + Pipeline

补齐剩余 2 家采集器 + 主调度闭环；共 6 个 commit，累计 3–4 小时。

## 目标

- 四家采集器可被 pipeline 统一调度，单家失败不影响其他
- `agent-token-usage-tui scan` 子命令 end-to-end 可用，完成后打印汇总并回填费用
- Windsurf 占位，为 Phase 2 VSCode 扩展导出 JSONL 预留接口

## 范围

- **做**：OpenClaw collector / OpenCode SQLite 只读 collector / Windsurf 占位 / `pipeline::run_scan` / cli `scan` + `sync-prices` 实装 / 多家 fixture 集成测试
- **不做**：TUI（M5）/ Windsurf 真实采集（Phase 2）

## 验收

- [ ] `cargo test collector` / `--test pipeline_test` 全绿
- [ ] `cargo run -- scan` 对 4 家执行：有数据入库、目录缺失跳过、异常只 `warn` 不中断
- [ ] 故意让一家 collector `panic`，其他 3 家仍完成
- [ ] scan 后 `cost_usd != 0` 比例 > 80%

## Commits

- **C1 · ✨ feat(collector/openclaw): OpenClaw JSONL 采集器** — 目录 `<base>/<agentId>/sessions/*.jsonl`；agentId 写入 `project` 字段；跳过 `model == "delivery-mirror"`；sidecar 单测
- **C2 · ✨ feat(collector/opencode): OpenCode SQLite 只读采集器** — 跨平台路径探测（Windows `%LOCALAPPDATA%` / macOS `~/Library/Application Support` / Linux `~/.local/share`）；`SQLITE_OPEN_READ_ONLY` + WAL；水位线存在 file_state.last_offset；过滤零 token 行
- **C3 · 🔧 chore(collector/windsurf): Phase 2 采集器占位** — 空 `WindsurfCollector { export_dir }`；`scan` 返回空 summary；文件头注释指向 Phase 2 VSCode 扩展方案；默认 `export_dir = exe_dir/windsurf-sessions`
- **C4 · ✨ feat(collector): pipeline 调度与容错** — `pipeline::run_scan(db, cfg, reporter)` 按启用开关串行跑各 collector；单家 `Err` 只 `tracing::warn`；结束后调 `recalc_costs`；sidecar 单测用 mock collector 验证失败隔离
- **C5 · ✨ feat(cli): 打通 scan 子命令** — 实装 `Scan` 分支（logging → config → Db → pricing sync → pipeline → recalc → tabled 汇总）；`SyncPrices` 分支（仅跑 pricing）
- **C6 · ✅ test(collector): 多家 fixture 集成测试** — `tests/fixtures/openclaw/<agent>/sessions/s1.jsonl` + 测试 setUp 内构造 opencode sample.db；`pipeline_test` 跑 4 家 fixture 合一验证总行数与失败隔离

## 执行时拆分提示

| commit | 拆分触发条件 | 建议子拆分 |
|---|---|---|
| C2 | 跨平台探测 + 水位线查询 > 200 行 | C2a 路径探测 + 只读连接 / C2b 增量查询 + 入库 |
| C4 | pipeline + mock 测试 > 200 行 | C4a pipeline 骨架 / C4b 失败隔离单测 |
| C5 | scan 分支 + tabled 格式化 > 150 行 | C5a 流程编排 / C5b 汇总表输出 |
