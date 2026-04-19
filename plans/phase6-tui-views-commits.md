# Phase M6 · TUI — Sessions / Models / Trend

补齐剩余 3 个视图 + 筛选 / 详情浮层 / 粒度切换；共 5 个 commit，累计 5–6 小时。

## 目标

- TUI 功能闭环：四个 tab 都有真实内容
- 高频交互齐备：`/` 筛选、`Enter` 详情、`[` `]` 切粒度、`H` `L` 平移窗口

## 范围

- **做**：`list_sessions` / `stats_by_model` / `trend_series` 聚合查询；Sessions Table + 筛选 + 详情浮层；Models BarChart + Table；Trend Chart + 粒度切换
- **不做**：CSV 导出（Phase 2）/ pricing overrides / 多 DB 切换

## 验收

- [ ] 3 个视图在有数据时正常渲染；空数据显示友好提示
- [ ] Sessions 的 `/claude` 筛选能缩到只剩 Claude 记录
- [ ] Sessions 选中行按 `Enter` 弹详情浮层，`Esc` 关闭
- [ ] Models 按 cost 降序 BarChart + 详情 Table
- [ ] Trend `[` `]` 切粒度（1h / 1d / 1w / 1mo），数据明显变化
- [ ] Trend `H` `L` 平移时间窗，超出数据范围钳位

## Commits

- **C1 · ✨ feat(storage/queries): sessions / models / trend 聚合查询** — `list_sessions(filter)` / `stats_by_model()` / `trend_series(granularity, window)`；Granularity 枚举 Hour/Day/Week/Month；按粒度 `strftime` 分组并填 0 值保证连续序列；sidecar 单测
- **C2 · ✨ feat(tui/views): Sessions 视图 + `/` 筛选** — `Table` 渲染 `SessionRow`（列：Source / Model / Started / Prompts / Tokens / Cost）；`j/k` 上下、`g/G` 跳顶底、`/` 进入 filter 模式（底部 `filter_input` widget）、`Esc` 取消、`Enter` 选中；`TableState::select` 维护滚动
- **C3 · ✨ feat(tui/widgets): 会话详情浮层** — `detail_popup` widget：中央浮层显示选中 session 的 prompts / records / 分模型 cost；`?` / `Esc` 关闭；窗口过小时自适应折叠
- **C4 · ✨ feat(tui/views): Models 视图** — 上半 BarChart 按 cost 降序；下半 Table 列：Model / Calls / Tokens / Sources（多源 fanout）/ Cost；`j/k` 列表滚动
- **C5 · ✨ feat(tui/views): Trend 视图与粒度切换** — `Chart` 渲染双 dataset（tokens + cost 双 Y 轴）；`[` `]` 切粒度；`H` `L` 平移窗口并钳位；底部显示当前粒度 / 窗口范围

## 执行时拆分提示

| commit | 拆分触发条件 | 建议子拆分 |
|---|---|---|
| C2 | Table + filter_input + 事件路由 > 250 行 | C2a Table 静态渲染 / C2b `/` filter_input 交互 |
| C5 | Chart + 粒度 + 平移 > 250 行 | C5a 图表静态渲染 / C5b 粒度切换 + 窗口平移钳位 |
