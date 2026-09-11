# 悬浮窗 Agent 状态监测 — 实现细节

> 本文档记录方案各阶段的实际实现细节，与 `悬浮窗Agent状态监测与模块化实施方案.md` 对照使用。

## 1. 整体数据流

```text
CLI hook（stdin JSON）
    │
    ▼
main.rs --status-hook 识别
    │
    ▼
hook.rs: 归一化为 AgentEvent → 写入 inbox/<id>.json（原子 rename）
    │
    ▼
mod.rs: monitor_loop 每 500ms 扫描 inbox，消费事件，每 10s 执行 cleanup
    │
    ▼
reducer.rs: StatusRegistry 维护 HashMap<key, SessionSnapshot>
    │
    ▼
mod.rs: emit_snapshot → app.emit("agent-status", StatusSnapshot)
    │
    ├──────────────────────┐
    ▼                      ▼
main WebView           floating WebView
    │                      │
    ▼                      ▼
agentStatus store      agentStatus store
    │                      │
    ▼                      ▼
MainWindow.vue         FloatingModuleHost.vue
（会话列表 + 配置）         │
                           ▼
                     AgentStatusModule.vue
                     （状态灯渲染）
```

## 2. Rust 后端

### 2.1 文件结构

```text
src-tauri/src/
  lib.rs              ← setup、managed state、monitor 启动、command 注册
  main.rs             ← 入口，识别 --status-hook 后短路退出
  status/
    mod.rs            ← StatusState、monitor_loop、consume_inbox、emit_snapshot
    model.rs          ← Provider、SessionPhase、AgentEvent、SessionSnapshot、StatusSnapshot
    reducer.rs        ← StatusRegistry、apply_event、cleanup、phase_for_event
    hook.rs           ← try_run_status_hook、stdin 读取、归一化、原子写入 inbox
  module_config.rs    ← FloatingModuleConfig、validate、get/set command
```

### 2.2 model.rs — 状态协议

| 类型 | 说明 |
|---|---|
| `Provider` | `Claude` / `Codex`，支持 `"claude-code"` / `"codex-cli"` 别名解析 |
| `SessionPhase` | `Running` / `Waiting` / `Error` / `Unknown` / `Completed` |
| `AgentEvent` | hook 写入的原始事件，含 schema_version、event_id、provider、session_id、event、sequence、pid、error_code、outcome |
| `SessionSnapshot` | 注册表中的会话状态，含 phase、source、confidence、completed_at_ms、remove_at_ms、is_flashing、termination_confirmed |
| `StatusSnapshot` | 广播给前端的全量快照，含 revision、sessions[]、counts、source_health |

关键函数：

- `session_key(provider, session_id)` → `"claude:abc123"`，确保不同 provider 的相同 session id 不互相覆盖。
- `now_ms()` → 毫秒时间戳，溢出时返回 0。

### 2.3 reducer.rs — 状态机核心

**StatusRegistry** 内部维护：

- `sessions: HashMap<String, SessionSnapshot>` — 按 key 索引的会话表。
- `seen_events: HashSet<String>` — 按 `"key:event_id"` 去重，防止同一事件重复消费。
- `revision: u64` — 每次有效变更递增，前端用 revision 忽略旧快照。

**apply_event 逻辑：**

1. schema_version 不匹配 → 拒绝。
2. event_id 或 session_id 为空 → 拒绝。
3. 已见事件（key:event_id 去重）→ 拒绝。
4. sequence 乱序（incoming ≤ last）→ 拒绝。
5. 已 completed 且未过 remove_at_ms → 拒绝后续事件。
6. 当前为 Error 且新事件非 Error → 保持 Error（错误态保护）。
7. 通过 `phase_for_event` 映射事件名到 SessionPhase。

**phase_for_event 映射规则：**

优先检查 error_code / outcome 字段，然后精确匹配归一化后的事件名：

| 阶段 | 事件 |
|---|---|
| `Running` | `session_start`, `setup`, `user_prompt_submit`, `user_prompt_expansion`, `pre_tool_use`, `post_tool_use`, `post_tool_use_failure`, `post_tool_batch`, `permission_request`, `subagent_start`, `subagent_stop`, `task_created`, `message_display`, `pre_compact`, `post_compact`, `pre_model_switch`, `post_model_switch`, `elicit`, `elicit_result`, `instructions_loaded`, `config_change`, `cwd_changed`, `directory_added`, `file_changed`, `worktree_create`, `worktree_remove` |
| `Waiting` | `stop`, `notification`, `permission_denied`, `task_completed`, `teammate_idle` |
| `Completed` | `session_end`（outcome 为 success/completed/ok/done） |
| `Error` | `stop_failure`, 任何带 error_code 的事件, outcome 为 error/failed/cancelled/denied 的事件 |
| 其他 | fallback 子串匹配 → `Unknown` |

事件名经过 `normalize_event_name` 预处理：CamelCase → snake_case，`-`/`.`/`:` → `_`，统一小写。

**apply_event 额外逻辑：**

当新事件映射为 `Running` 阶段时，自动清除同一 provider 的所有旧 `Waiting` 会话。这实现了"成功状态保留到下次运行"的语义。

**cleanup 逻辑（每 10 秒执行）：**

- `Completed` 阶段：到期（now ≥ remove_at_ms）则移除，未到期则保留。
- `Waiting` 阶段：保留到同一 provider 的下次运行开始时清理（30 分钟 failsafe 超时）。
- `Running` / `Error` / `Unknown` 阶段：进程死亡 → 立即移除；60 秒无新事件 → 超时移除。

**状态灯更新全流程示例：**

```text
t=0    hook: SessionStart       → apply_event → Running  🟢+动画
t=1    hook: UserPromptSubmit   → apply_event → Running  🟢+动画
t=2    hook: PreToolUse(Bash)   → apply_event → Running  🟢+动画
t=3    hook: PostToolUse(Bash)  → apply_event → Running  🟢+动画
t=4    hook: PreToolUse(Write)  → apply_event → Running  🟢+动画
t=5    hook: PostToolUse(Write) → apply_event → Running  🟢+动画
t=6    hook: Stop               → apply_event → Waiting  🟡（绿灯变黄灯）
                                   （黄灯保留在屏幕上，不消失）
       ... 用户思考中 ...
t=60   hook: UserPromptSubmit   → apply_event → Running  🟢+动画
                                   （旧 Waiting 被自动清理，黄灯变绿灯）
t=61   hook: PreToolUse(Bash)   → apply_event → Running  🟢+动画
t=62   hook: SessionEnd         → apply_event → Completed 🟢无动画
                                   （绿灯保留 30 秒后消失）
t=92   cleanup: now ≥ remove_at → 删除 Completed 会话 → 灯消失
```

**apply_event 内部逐步解析：**

```text
事件进入 apply_event
    │
    ├─ ① 过滤层：schema/event_id/session_id 去重/sequence/Completed 保护
    │     任一不通过 → return false，状态不变
    │
    ├─ ② 查询 previous：sessions.get(key)
    │     提取 previous_error、previous_started_at、previous_pid、previous_error_code
    │
    ├─ ③ phase_for_event 判定：
    │     error_code/outcome → Error（红灯）
    │     精确匹配事件名 → Running/Waiting/Completed/Error
    │     fallback 子串匹配 → Unknown
    │
    ├─ ④ 错误态保护：
    │     previous_error && 新 phase ≠ Error → 保持 Error
    │     （红灯一旦亮起，只有 session 被清理后重建才能恢复）
    │
    ├─ ⑤ 清理旧 Waiting：
    │     if phase == Running
    │       删除同一 provider 下所有 ≠ 当前 session 的 Waiting 会话
    │       （这就是"黄灯保留到下次运行"的核心机制）
    │
    ├─ ⑥ 构建 SessionSnapshot：
    │     phase → 决定灯的颜色
    │     last_seen_at_ms = now → 用于超时判断
    │     is_flashing = (Completed && retention > 0) → 闪烁动画
    │     remove_at_ms = completed_at + retention → 闪烁结束时间
    │     pid/error_code → 继承自 previous 或来自新事件
    │
    └─ ⑦ 写入 sessions 表，revision++
          return true → 触发 emit_snapshot → 前端更新
```

**cleanup 定时清理决策树：**

```text
cleanup(now, is_alive) 每 10 秒执行
    │
    对每个 session：
    │
    ├─ phase == Completed？
    │   └─ now ≥ remove_at_ms？ → 删除 : 保留
    │
    ├─ phase == Waiting？
    │   └─ now - last_seen_at_ms > 30min？ → 删除 : 保留
    │       （正常情况下由下次 Running 事件清理，30 分钟是 failsafe）
    │
    └─ phase == Running/Error/Unknown？
        ├─ now - last_seen_at_ms > 60s？ → 删除（超时）
        └─ is_alive(session) == Some(false)？ → 删除（进程死亡）
           否则 → 保留
```

**前端渲染决策：**

```text
snapshot → store.applySnapshot（revision 检查，忽略旧快照）
         → sessions 响应式更新
         → AgentStatusModule.vue 遍历 sessions
         → 每个 session 根据 phase 决定颜色：
             running/completed → 绿色圆点（bg-emerald-400）
             waiting/unknown   → 黄色圆点（bg-amber-300）
             error             → 红色圆点（bg-rose-500）
           根据 is_flashing 决定是否加闪烁动画
```

### 2.4 hook.rs — Hook Bridge

入口：`try_run_status_hook(args)` 在 `main.rs` 中调用，匹配 `--status-hook` 参数后短路退出，不启动 Tauri 窗口。

流程：

1. 从参数提取 `--provider`（支持 `=` 和空格两种格式）。
2. 从 stdin 读取 JSON，限制 64 KiB。
3. 从 payload 提取白名单字段（session_id、event、pid、event_id、sequence、error_code、outcome 等），支持多种命名风格。
4. 归一化为 `AgentEvent`。
5. 写入 `inbox/<unique-id>.json`：先写临时文件 `.tmp`，`sync_all` 后原子 rename。
6. session_id 长度限制 256 字符。

状态目录优先级：`--status-dir` 参数 > `UFO_STATUS_DIR` 环境变量 > 默认 app-data 路径。

### 2.5 mod.rs — 监测器

`StatusState` 持有：

- `registry: Arc<Mutex<StatusRegistry>>`
- `status_dir: PathBuf`
- `retention_ms: Arc<Mutex<u64>>` — 与 module_config 联动，配置变更时同步更新。

`monitor_loop` 在独立线程运行：

1. 每 500ms 扫描 inbox 目录。
2. 对每个 `.json` 文件：解析成功则消费并删除，同时 emit `status-hook-event` 到前端 DevTools 日志；失败则移入 quarantine 目录并标记 source_health 为 degraded。
3. 每 10 秒执行一次 cleanup。
4. 任何状态变更后调用 `emit_snapshot` 广播 `agent-status` 事件。

`process_alive` 实现：

- Windows：`tasklist /FI "PID eq {pid}" /FO CSV /NH`，检查输出是否包含 PID。
- 非 Windows：`kill -0 {pid}`，检查退出码。

### 2.6 module_config.rs — 模块配置

`FloatingModuleConfig` 字段：

| 字段 | 默认值 | 说明 |
|---|---|---|
| schema_version | 1 | 固定 |
| revision | 1 | 每次写入递增 |
| modules | `[{id:"agent-status",enabled:true,order:0}]` | 模块列表 |
| completed_retention_ms | 30000 | 完成灯保留时长，clamp 1000~86400000 |
| max_cells_by_layout | 各形态 8 | 方形/四角/横条/竖条格子上限，各 clamp 4~8；旧 `max_visible_sessions` 加载时迁移后废弃 |

`validate` 规则：

- schema_version 必须为 1。
- module id 长度 ≤ 64，不允许 `/` 或 `\`。
- 缺少 `agent-status` 时自动补全。

配置文件路径：`<app-data>/floating-modules.json`。

写入策略：先写 `.tmp`，删除旧文件，rename 新文件（Windows 兼容）。

配置变更时同步更新 `StatusState.retention_ms`，使运行中的监测器立即采用新保留时长。

### 2.7 lib.rs — Tauri 集成

setup 阶段：

1. 获取 app-data 目录。
2. 加载 `ModuleState`（从 `floating-modules.json`）。
3. 用 module_config 的 retention_ms 初始化 `StatusState`。
4. `app.manage(StatusState)` + `app.manage(ModuleState)`。
5. `spawn_monitor` 启动唯一监测线程。
6. 注册托盘菜单和 devtools。

注册的 commands：

- `get_status_snapshot` — 读取当前全量快照。
- `get_floating_module_config` — 读取模块配置。
- `set_floating_module_config` — 写入模块配置（仅 main 窗口有权限）。

### 2.8 权限

`src-tauri/permissions/status.toml`：

- `allow-get-status-snapshot` → `get_status_snapshot`
- `allow-get-floating-module-config` → `get_floating_module_config`
- `allow-set-floating-module-config` → `set_floating_module_config`

`src-tauri/capabilities/default.json`（main + floating）：

- core:default、core:event:default
- 窗口操作（hide/show/set-focus/set-position/set-size/start-dragging/unminimize）
- allow-greet
- allow-get-status-snapshot、allow-get-floating-module-config

`src-tauri/capabilities/main-module-settings.json`（仅 main）：

- allow-set-floating-module-config

## 3. 前端

### 3.1 文件结构

```text
src/
  App.vue                         ← 统一初始化 agentStatus + floatingModules store
  main.ts                         ← 每个 WebView 独立 Vue/Pinia 实例
  store.ts                        ← 原有基础 store
  lib/
    agent-status.ts               ← 类型定义、runtime guard、listen/invoke 封装
    agent-status-layout.ts        ← layoutFromDocked、visibleSessionCount
    floating-modules.ts           ← 模块定义、配置类型、isModuleEnabled、enabledModuleDefinitions
    snap.ts                       ← 贴边算法（已有）
    window.ts                     ← showMainWindow（已有）
  stores/
    agentStatus.ts                ← Pinia store：snapshot、connection、start/stop
    floatingModules.ts            ← Pinia store：config、agentStatusEnabled、updateRetention
  components/
    FloatingWindow.vue            ← 拖拽、点击、贴边、layoutMode → FloatingModuleHost
    FloatingModuleHost.vue        ← 双槽布局（状态模块 + 未来模块）
    AgentStatusModule.vue         ← 状态灯渲染
    MainWindow.vue                ← 主窗口：模块开关、保留时长、会话列表
```

### 3.2 agent-status.ts — 类型与 API

类型定义与 Rust model.rs 完全对应：

- `Provider`: `'claude' | 'codex'`
- `SessionPhase`: `'running' | 'waiting' | 'error' | 'unknown' | 'completed'`
- `SessionSnapshot`: 含 key、provider、session_id、phase、source、confidence、started_at_ms、last_seen_at_ms、completed_at_ms、remove_at_ms、is_flashing、pid、error_code、last_sequence、termination_confirmed
- `StatusSnapshot`: 含 schema_version、revision、generated_at_ms、sessions[]、counts、source_health

`isStatusSnapshot` runtime guard 校验：

- schema_version === 1
- revision 为非负安全整数
- generated_at_ms 为 number
- sessions 为数组且长度 ≤ 1000
- 每个 session 校验 key（string）、provider（claude/codex）、session_id（string）、phase（string）、termination_confirmed（boolean）

`colorForSession` 映射：

| phase | 颜色 |
|---|---|
| running | green |
| completed | green |
| waiting | yellow |
| unknown | yellow |
| error | red |

### 3.3 floating-modules.ts — 模块系统

`FloatingModuleDefinition`：id、label、version、defaultEnabled、supportedLayouts、component（编译期绑定的 Vue 组件）。

`createFloatingModuleDefinitions(component)` 返回 agent-status 模块定义，component 参数在 FloatingModuleHost.vue 中传入 AgentStatusModule。

`isModuleEnabled(config, id)` — 从配置中查找模块启用状态，null config 返回 false。

`enabledModuleDefinitions(config, definitions)` — 过滤已启用模块，按 config order 排序，order 相同时按 id 字母序。

### 3.4 agent-status-layout.ts — 布局辅助

`layoutFromDocked(docked)` — 将贴边状态转为布局模式：

| docked 条件 | layoutMode |
|---|---|
| null 或无贴边 | square |
| 上或下贴边（不含左右） | horizontal |
| 左或右贴边（不含上下） | vertical |
| 上下+左右同时贴边 | corner |

`visibleSessionCount(total, maxVisible=8)` — 返回 `min(total, maxVisible)`，用于限制悬浮窗显示数量。

### 3.5 stores/agentStatus.ts — 状态 Store

state：snapshot、connection（starting/connected/degraded）、lastRevision、started。

actions：

- `start()` — 幂等。先 listen("agent-status")，再 invoke get_status_snapshot 初始化，同时 listen("status-hook-event") 输出原始 hook 数据到 DevTools console。
- `stop()` — 释放两个 listener。
- `applySnapshot(snapshot)` — 仅接受 revision ≥ lastRevision 的快照。

getters：sessions（排序后的会话列表）、sessionColor、hasSessions。

### 3.6 stores/floatingModules.ts — 模块配置 Store

state：config、connection、started。

fallbackConfig：当 invoke 失败时使用的默认配置（agent-status 启用，30 秒保留，各形态上限 8）。

actions：

- `start()` — 幂等。listen("floating-module-config") + invoke get_floating_module_config。
- `updateAgentStatus(enabled)` — 更新 agent-status 模块启用状态。
- `updateRetentionSeconds(seconds)` — 更新完成灯保留时长（1~86400 秒）。
- `stop()` — 释放 listener。

### 3.7 App.vue — 统一初始化

```typescript
const isFloatingWindow = getCurrentWindow().label === 'floating'
const agentStatus = useAgentStatusStore()
const floatingModules = useFloatingModulesStore()

onMounted(() => {
  void agentStatus.start()
  void floatingModules.start()
})
```

两个 WebView（main 和 floating）都在 onMounted 中启动 store，确保都能接收状态事件和模块配置。

### 3.8 FloatingWindow.vue — 悬浮窗

职责：拖拽、点击打开主窗口、调用贴边算法、维护 layoutMode。

贴边流程：

1. 拖拽结束后 `scheduleSnap()`（140ms 防抖）。
2. `snapWindow()` 读取窗口物理坐标和尺寸，调用 `snapToWorkArea`。
3. 吸附后更新 `layoutMode = layoutFromDocked(result.docked)`。
4. layoutMode 传递给 FloatingModuleHost。

监听 `onMoved` 和 `onScaleChanged` 事件，自动重新吸附。

### 3.9 FloatingModuleHost.vue — 模块宿主

接收 `layoutMode` prop，根据布局模式选择 CSS grid：

- square/corner/horizontal：两列单行（左半区 + 右半区）。
- vertical：单列两行（上半区 + 下半区）。

两个槽位：

- `__slot--status`：渲染第一个已启用模块（agent-status）。
- `__slot--future`：显示 "U" 标记，保留给未来模块。

### 3.10 AgentStatusModule.vue — 状态灯

接收 `layoutMode` 和 `maxVisible` props。

从 agentStatus store 取 sessions，截取前 maxVisible 个。

每个 session 渲染为一个圆点：

- 颜色由 `colorForSession` 决定。
- `is_flashing` 为 true 时添加闪烁动画 class。
- aria-label/title 显示 provider、短 session id 和 phase。
- provider 缩写：Claude → "C"，Codex → "X"。

布局 grid：

- horizontal：4 列 × 2 行。
- 其他（square/corner/vertical）：2 列 × 4 行。

### 3.11 MainWindow.vue — 主窗口设置

模块开关：checkbox 绑定 `modules.agentStatusEnabled`，change 时调用 `modules.updateAgentStatus`，失败时回滚 UI。

保留时长：number input 绑定 `retentionInput`，change 时调用 `modules.updateRetentionSeconds`，失败时回滚。

会话列表：遍历 `agentStatus.sessions`，显示彩色圆点、provider、短 session id 和 phase。超过 8 个时由前端限制显示，后端保留完整数据。

### 3.12 CSS 关键样式

悬浮窗透明背景：

```css
html.is-floating-window,
html.is-floating-window body,
html.is-floating-window #app {
  background: transparent;
}
```

毛玻璃效果：

```css
.floating-shell {
  backdrop-filter: blur(14px);
  background: rgba(15, 23, 42, 0.7); /* bg-slate-900/70 */
}
```

闪烁动画：

```css
@keyframes agent-status-flash {
  0%, 100% { opacity: 1; transform: scale(1); }
  50% { opacity: 0.35; transform: scale(0.78); }
}
```

状态灯颜色：green → `bg-emerald-400`，yellow → `bg-amber-300`，red → `bg-rose-500`。

## 4. 测试覆盖

### 4.1 Rust 测试（54 个）

**model.rs（5 个）：**

- provider_parse_known_values — claude/claude-code/CLAUDE/codex/codex-cli/CODEX
- provider_parse_rejects_unknown — 空字符串、unknown、gpt
- session_key_includes_provider_namespace — claude:abc / codex:abc
- now_ms_returns_positive_value
- agent_event_serialization_roundtrip — 序列化/反序列化往返

**reducer.rs（29 个）：**

- keeps_multiple_provider_sessions_separate
- completed_sessions_flash_until_retention_expires
- dead_error_sessions_are_removed_by_liveness_check
- stop_and_notification_mean_waiting_not_success
- session_end_is_the_completed_signal
- event_ids_are_scoped_to_the_session
- stale_sequences_and_completed_sessions_are_ignored
- duplicate_events_do_not_change_revision
- wrong_schema_version_is_rejected
- empty_event_id_is_rejected
- empty_session_id_is_rejected
- custom_retention_affects_remove_at
- liveness_check_preserves_completed_green_lights
- no_pid_unknown_liveness_keeps_confirmed_sessions
- stale_error_sessions_are_removed_after_timeout
- error_state_is_protected_from_non_error_events
- source_health_starts_healthy
- custom_retention_zero_disables_flashing
- concurrent_sessions_same_provider_different_ids（8 个并发）
- normalize_event_name_handles_camel_case — TurnCompleted → turn_completed
- normalize_event_name_handles_separators — session-end / session.end / session:end
- normalize_event_name_lowercases — STOP → stop, Start → start
- waiting_sessions_cleared_when_new_run_starts — Waiting 状态在新 Running 事件到来时清理
- waiting_sessions_not_cleared_by_other_provider — 不同 provider 的 Waiting 互不影响
- stop_failure_maps_to_error — StopFailure 映射为 Error
- notification_maps_to_waiting — Notification 映射为 Waiting

**hook.rs（11 个）：**

- argument_value_from_equals_syntax — `--provider=claude`
- argument_value_from_separate_args — `--provider claude`
- argument_value_returns_none_for_missing
- argument_value_rejects_flag_as_value — `--provider --other` → None
- string_field_reads_first_matching_name
- string_field_returns_none_for_missing
- number_field_reads_u64_value
- number_field_returns_none_for_missing
- try_run_status_hook_returns_none_without_flag
- default_status_dir_contains_app_identifier
- unique_event_ids_are_unique

**module_config.rs（9 个）：**

- default_config_has_agent_status_enabled
- default_retention_is_thirty_seconds
- default_max_cells_are_eight
- validate_clamps_retention_to_range — 0 → 1000, u64::MAX → 86400000
- validate_clamps_max_cells_by_layout — 各形态 0/3 → 4，255 → 8
- migrates_legacy_max_visible_sessions
- validate_rejects_wrong_schema_version
- validate_adds_agent_status_if_missing
- validate_rejects_module_id_with_slash / backslash
- validate_rejects_overlong_module_id（65 字符）
- config_serialization_roundtrip
- module_state_loads_default_on_missing_file

### 4.2 TypeScript 测试（45 个）

**agent-status.spec.ts（17 个）：**

- colorForSession 5 个 phase 各自映射正确
- colorForSession 不返回聚合颜色
- isStatusSnapshot 接受有效快照
- isStatusSnapshot 拒绝 null / non-object / wrong schema / negative revision / non-integer revision / missing generated_at_ms / non-array sessions / 1001 sessions / missing key / invalid provider / missing termination_confirmed
- isStatusSnapshot 接受 codex provider / 空 sessions / 拒绝 missing counts

**agent-status-layout.spec.ts（3 个）：**

- 4 种 layoutFromDocked 映射
- visibleSessionCount 上限为 8
- 颜色映射与 phase 对应

**floating-modules.spec.ts（7 个）：**

- isModuleEnabled true / false / missing / null config
- enabledModuleDefinitions 过滤 / 排序 / null config / 等 order 字母序

**snap.spec.ts（原有，18 个）：**

- 左/右/上/下边吸附
- 角落吸附
- 阈值外最近边
- 等距优先级
- 展开窗口轴向约束
- 脱离贴边后回弹
- 负坐标多显示器
- 超大窗口 clamp

## 5. 与方案对照

| 方案阶段 | 状态 | 说明 |
|---|---|---|
| 阶段一：协议和 CLI 探测 | ✅ | schema v1、phase、颜色、清理规则已锁定 |
| 阶段二：模块基础设施 | ✅ | FloatingModuleHost 双槽、编译期 registry |
| 阶段三：Rust 状态核心 | ✅ | model/reducer/hook + 50 测试 |
| 阶段四：Hook bridge | ✅ | main.rs --status-hook、原子写入、quarantine |
| 阶段五：Tauri 集成 | ✅ | managed state、monitor、commands、permissions |
| 阶段六：前端状态与主窗口 | ✅ | stores、App.vue 初始化、MainWindow 配置 |
| 阶段七：浮窗 UI | ✅ | 4 种布局、半区模块、闪烁动画 |
| 阶段八：fallback 和发布验证 | ⬜ | 需手工打包验证 |

## 6. 已知限制

1. **进程 liveness 检查依赖 tasklist/kill** — 在某些受限环境下可能失败，此时返回 None，会话保留到超时。
2. **inbox 扫描为轮询（500ms）** — 首版实现，延迟在可接受范围；如需更低延迟可改为命名管道或 socket。
3. **Codex 尚未做 capability probe** — 当前 Codex adapter 使用与 Claude 相同的 hook 格式，需在实际 Codex CLI 上验证。
4. **配置文件原子写入** — Windows 上先删除再 rename，非真正的原子替换；在极端并发下可能短暂不可读。
5. **悬浮窗最多显示 8 个会话** — 超出部分仅在主窗口可见，悬浮窗无 +N 提示。
6. **Waiting 超时为 30 分钟** — 正常情况下 Waiting 在下次运行时清理；若 hook 未触发（如 Claude Code 异常退出），需等待30 分钟超时。
