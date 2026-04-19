# Phase M2 · Storage + Pricing

数据层完整落地 + litellm 价格拉取 / 内嵌 / 模糊匹配 / 费用回填；共 8 个 commit，累计 4–5 小时。

## 目标

- 所有上层模块（collector / tui）能直接对 `Db` 操作
- pricing 三重保障：在线拉取 → 本地缓存 → 编译期内嵌 fallback
- 模糊匹配让费用覆盖率 > 80%

## 范围

- **做**：`domain/*` 全量 / `storage/*` 全部模块（schema SQL 用 `include_str!` 从 `migrations/` 挂载） / `pricing/*` 全部模块 / `build.rs` 真实拉取 JSON / 核心回归测试（使用 `pretty_assertions`）
- **不做**：collector / TUI / 多 DB / pricing 覆盖配置

## 验收

- [ ] `cargo test storage` / `cargo test pricing` 全绿
- [ ] 手写冒烟程序能 `Db::open` → `sync_or_fallback` → `pricing` 表 ≥ 100 行
- [ ] 断网时 `sync_or_fallback` 仍完成（落 fallback 进 DB）
- [ ] `recalc_costs` 回填后 `cost_usd != 0` 比例 > 80%

## Commits

- **C1 · ✨ feat(domain): 定义核心数据模型** — `Source` / `UsageRecord` / `SessionRecord` / `PromptEvent` / `ModelPrice`；derive `Debug / Clone / Serialize / Deserialize`；sidecar 单测
- **C2 · ✨ feat(storage): Db 封装与 schema 迁移** — `Db::open` + `Arc<Mutex<Connection>>` + WAL；`migrations/001_init.sql` 用 `include_str!` 挂载；`migrate()` 走 `meta` 表的 `migration_<id>=done`；sidecar 单测
- **C3 · ✨ feat(storage): file_state + records/sessions/prompts CRUD** — `FileScanContext` / `get/set_file_state` / 批量 insert（单事务 + `prepare_cached`） / `upsert_session`
- **C4 · ✨ feat(storage): pricing 读写与新鲜度判断** — `upsert_pricing` / `get_pricing` / `get_all_pricing` / `pricing_is_fresh(Duration)`
- **C5 · ✨ feat(storage): 模糊匹配与费用回填** — `match_pricing`（直接命中 → provider-prefix → 归一化子串 + 最短键）、`recalc_costs`；sidecar 测试覆盖 provider-prefix / version-dash / reseller-path 分支
- **C6 · ✨ feat(pricing): build.rs 拉取 litellm + 内嵌 fallback** — `build.rs` reqwest blocking 拉 JSON 到 `assets/litellm-prices.fallback.json`（失败写 `{}` + `cargo:warning`，不中断编译）；`src/pricing/fallback.rs` 通过 `include_bytes!` 嵌入
- **C7 · ✨ feat(pricing): 运行时同步与费用公式** — `sync_from_github` / `calc_cost` 非重叠公式；`sync_or_fallback` 三层策略（fresh → 网络 → fallback）
- **C8 · ✅ test(storage,pricing): 核心逻辑回归用例** — `tests/storage_dedup_test.rs` / `pricing_match_test.rs` / `pricing_cost_test.rs`；使用 `pretty_assertions::assert_eq`，整对象比对而非逐字段

## 执行时拆分提示

| commit | 拆分触发条件 | 建议子拆分 |
|---|---|---|
| C5 | 算法 + recalc + 单测 > 200 行 | C5a 模糊匹配算法（含单测） / C5b `recalc_costs` 主流程 |
| C6 | build.rs 网络分支 + fallback 合计 > 200 行 | C6a build.rs 下载 / C6b fallback.rs 内嵌读取 |
