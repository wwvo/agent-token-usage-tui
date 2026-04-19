# Phase M5 · TUI — Overview

TUI 骨架跑通 + Overview 视图完整；共 6 个 commit，累计 5–6 小时。

## 目标

- `cargo run` 默认进 TUI，4 个 tab 头可切换（其他 3 个显示 WIP 占位）
- Overview：4 卡片 + 模型分布 BarChart + 7 日费用 Sparkline
- 后台 scan task + 实时进度、panic hook 保证终端不留 raw mode、错误态显示错误条而非崩溃

## 范围

- **做**：`storage::queries::get_overview_snapshot` / app state + Elm 事件循环 + 终端接管 + panic hook / tabs / summary_cards / status_bar widgets / Overview 视图 / 后台 scan task + 进度回显 / cli 默认入口
- **不做**：Sessions / Models / Trend 实装（M6）/ 按天滚动日志（M7）

## 验收

- [ ] `cargo run` 进 TUI，Overview 立即渲染；DB 为空时显示"No data — run 'r' to scan"
- [ ] `1/2/3/4` / `h/l` / `←/→` 切换 tab；M6 之外 tab 显示 `(work in progress — M6)`
- [ ] `r` 触发重扫，状态栏显示 `Scanning claude: 12/45`
- [ ] 故意让 scan 抛错，顶部显示错误条，内容保留
- [ ] `q` / `Ctrl-C` / panic 后终端恢复（测试：退出后 `ls` 显示正常）
- [ ] 窗口 resize 布局正确
- [ ] `?` 弹出帮助浮层

## Commits

- **C1 · ✨ feat(storage/queries): Overview 聚合查询** — `get_overview_snapshot(db) -> OverviewSnapshot`（total_cost / tokens / session_count / api_call_count / cost_by_model 前 10 / cost_by_day_last7 含 0 值 / source_counts）；SQL 用 CTE 单次查询；sidecar 单测
- **C2 · ✨ feat(tui): app state + Elm 事件循环 + 终端接管** — `app.rs`（`struct App` + `AppMsg` + `update`）；`event.rs`（crossterm EventStream → AppMsg）；`theme.rs`（色集 + `NO_COLOR` 降级）；`tui::run` 做 `enable_raw_mode` + alternate screen + panic hook 恢复终端
- **C3 · ✨ feat(tui/widgets): tabs / summary_cards / status_bar** — 通用组件：顶部 tab bar（高亮 + 数字前缀）/ 等宽 4 卡片 / 底部双行（sources 统计 + 键位提示 + 扫描进度）；参数全部 `&State` 只读借用
- **C4 · ✨ feat(tui/views): Overview 视图** — 三区布局（4 卡片 / 模型分布 BarChart / 7 日费用 Sparkline）；空数据态友好提示；sessions / models / trend 视图先渲染 `(work in progress — M6)`
- **C5 · ✨ feat(tui): 后台 scan task 与进度回显** — `mpsc::channel::<AppMsg>(64)` 统一事件 / 进度 / 完成消息；按 `r` spawn tokio task 跑 `pipeline::run_scan` + `ChannelReporter`；`Option<JoinHandle>` 防止并发重扫；task panic 捕获为 error state
- **C6 · ✨ feat(cli): 默认入口进入 TUI** — 无子命令分支：初始化 → Db → `pricing::sync_or_fallback` → 可选首次 scan → `tui::run`；`--no-scan` / `--no-prices` 支持；启动失败展示友好错误并退出

## 执行时拆分提示

| commit | 拆分触发条件 | 建议子拆分 |
|---|---|---|
| C2 | 终端接管 + panic hook + 事件循环 > 250 行 | C2a 终端接管 + panic hook + Terminal 封装 / C2b AppState + update + 事件循环 |
| C4 | Overview 视图 + 3 个 placeholder > 250 行 | C4a 布局 + theme + 静态卡片 / C4b BarChart + Sparkline 数据绑定 |
| C5 | mpsc 交互 + JoinHandle 容错 > 200 行 | C5a 后台 task + channel 骨架 / C5b 失败捕获 + 重扫锁 |
