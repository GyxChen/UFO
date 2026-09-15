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
    │      同时透传 transcript_path（hook 本身不解析 transcript）
    ▼
mod.rs: monitor_loop 每 500ms 扫描 inbox，消费事件，每 10s 执行 cleanup
    │
    ├─► tokens.rs: TokenTracker 增量解析 transcript → set_tokens
    ▼
reducer.rs: StatusRegistry 维护 HashMap<key, SessionSnapshot>
    │
    ▼
mod.rs: emit_snapshot → app.emit("agent-status", StatusSnapshot)
    │
    ├──────────────────────┬──────────────────────┐
    ▼                      ▼                      ▼
main WebView           floating WebView       tooltip WebView
    │                      │                      │
    ▼                      ▼                      ▼
agentStatus store      agentStatus store      agentStatus store
floatingModules store  floatingModules store  floatingModules store
    │                      │                      │
    ▼                      ▼                      ▼
MainWindow.vue         FloatingModuleHost.vue TooltipWindow.vue
（会话列表 + 配置）        │                    （悬停会话详情）
                           ▼
                     AgentStatusModule.vue
                     （状态灯渲染）
                           │ pointerenter → onSessionHover(key)
                           ▼
                     FloatingWindow.vue
                     （延迟 250ms → emitTo('tooltip', 'tooltip-hover')）
                           │
                           └──────────────► tooltip WebView 定位并 show
```

三个 WebView **各自**创建独立的 Vue/Pinia 实例，因此每个窗口都要在 `App.vue` 中启动 `agentStatus` 与 `floatingModules` 两个 store。提示窗曾经只订阅 `agent-status`，导致主窗口切换 token 单位后它不生效 —— 现已统一。

## 2. Rust 后端

### 2.1 文件结构

```text
src-tauri/src/
  lib.rs              ← setup、managed state、monitor 启动、command 注册
  main.rs             ← 入口，识别 --status-hook 后短路退出
  status/
    mod.rs            ← StatusState、monitor_loop、consume_inbox、emit_snapshot、attach_upstreams、get_proxy_resolvers
    model.rs          ← Provider、SessionPhase、AgentEvent、SessionSnapshot、StatusSnapshot
    reducer.rs        ← StatusRegistry、apply_event、cleanup、phase_for_event
    hook.rs           ← try_run_status_hook、stdin 读取、归一化、原子写入 inbox
    tokens.rs         ← TokenTracker：增量解析 transcript 统计 token 消耗
    test_support.rs   ← 测试用临时目录 TempDir（仅测试构建编译，proxy 测试亦复用）
  proxy/
    mod.rs            ← ProxyResolver trait、ResolverRegistry、ProxyState、build_registry、normalize_host_port
    ccswitch.rs       ← CcSwitchResolver：只读 cc-switch.db 反查上游，带 TTL 缓存
  module_config.rs    ← FloatingModuleConfig、TokenUnit、proxy_resolver、validate、get/set command
```

### 2.2 model.rs — 状态协议

| 类型 | 说明 |
|---|---|
| `Provider` | `Claude` / `Codex`，支持 `"claude-code"` / `"codex-cli"` 别名解析 |
| `SessionPhase` | `Running` / `Waiting` / `Error` / `Unknown` / `Completed` |
| `AgentEvent` | hook 写入的原始事件，含 schema_version、event_id、provider、session_id、event、sequence、pid、error_code、outcome、notification_type、base_url、prompt、transcript_path |
| `SessionSnapshot` | 注册表中的会话状态，含 phase、source、confidence、completed_at_ms、remove_at_ms、is_flashing、base_url、upstream_url、prompt、tokens、termination_confirmed |
| `TokenTotals` | 会话累计 token：input_tokens / cache_creation_input_tokens / cache_read_input_tokens / output_tokens |
| `StatusSnapshot` | 广播给前端的全量快照，含 revision、sessions[]、counts、source_health |

`AgentEvent` 的 `base_url` / `prompt` / `transcript_path` 和 `SessionSnapshot` 的 `tokens`、`upstream_url` 均为 `Option` + `#[serde(default)]`，只增字段、**不升 `SCHEMA_VERSION`**，否则旧 hook 二进制写出的事件会被静默丢弃。`upstream_url` 不是事件派生字段，由 status 侧生成快照时按选中的代理解析器现算（见 2.8）。

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
6. 已 completed 且未过 remove_at_ms → 拒绝后续事件。
7. 当前为 Error 且新事件非 Error → 保持 Error（错误态保护）；例外：`UserPromptSubmit` / `SessionStart` 允许恢复为 Running。
8. 通过 `phase_for_event` 映射事件名到 SessionPhase。

**phase_for_event 映射规则：**

优先检查 error_code / 失败 outcome 字段，然后精确匹配归一化后的事件名：

| 阶段 | 灯色 | 事件 |
|---|---|---|
| `Running` | 🟢 常亮 | `session_start`, `setup`, `user_prompt_submit`, `user_prompt_expansion`, `pre_tool_use`, `post_tool_use`, `post_tool_use_failure`, `post_tool_batch`, `subagent_start`, `subagent_stop`, `task_created`, `task_completed`, `teammate_idle`, `permission_denied`, `message_display`, `pre_compact`, `post_compact`, `elicit_result` / `elicitation_result`, `instructions_loaded`, `config_change`, `cwd_changed`, `directory_added`, `file_changed`, `worktree_create`, `worktree_remove`；以及非等待类 `notification` |
| `Waiting` | 🟡；Stop 额外先绿闪 | `stop`（先绿闪，retention 后黄常亮）；`permission_request`；`elicit` / `elicitation`；`notification` 且 `notification_type` ∈ `idle_prompt` / `permission_prompt` / `elicitation_dialog` / `agent_needs_input`（缺 type 不进 Waiting） |
| `Completed` | 🟢 + 闪烁后移除 | 任意 `session_end`（SessionEnd 只有 `reason`，不再按 success outcome 分流；红灯不靠 SessionEnd） |
| `Error` | 🔴 | `stop_failure`；任何带 error_code 的事件；outcome 为 error/failed/cancelled/denied 的事件 |
| 其他 | — | fallback 子串匹配 → `Unknown` |

事件名经过 `normalize_event_name` 预处理：CamelCase → snake_case，`-`/`.`/`:` → `_`，统一小写。

**灯色语义（已锁定）：**

- 🟢 常亮 = Running（正在执行）
- 🟢 + 闪烁 → 🟡 = Stop 一轮执行完成（Waiting + is_flashing，闪完落黄）
- 🟡 = Waiting 稳态 / 权限 / Elicitation 等弹窗等人
- 🔴 = Error（接口/不可抗力）
- 🟢 + 闪烁 → 移除 = Completed（对话已完成）

**apply_event 额外逻辑：**

当新事件映射为 `Running` 阶段时，自动清除同一 provider 的所有旧 `Waiting` 会话。这实现了"成功状态保留到下次运行"的语义。

**cleanup 逻辑（每 10 秒执行）：**

- `Completed` 阶段：到期（now ≥ remove_at_ms）则移除，未到期则保留。
- `Waiting` 阶段：保留到同一 provider 的下次运行开始时清理（30 分钟 failsafe 超时）。
- `Running` 阶段：进程死亡 → 立即移除；无新事件超时使用可配置的 `running_timeout_ms`（默认 60 秒，主窗口可改）。
- `Error` / `Unknown` 阶段：进程死亡 → 立即移除；60 秒无新事件 → 超时移除。

**状态灯更新全流程示例：**

```text
t=0    hook: SessionStart       → apply_event → Running  🟢常亮
t=1    hook: UserPromptSubmit   → apply_event → Running  🟢常亮
t=2    hook: PreToolUse(Bash)   → apply_event → Running  🟢常亮
t=3    hook: PostToolUse(Bash)  → apply_event → Running  🟢常亮
t=4    hook: PermissionRequest  → apply_event → Waiting  🟡（弹窗等人，不闪）
t=5    hook: PostToolUse(Write) → apply_event → Running  🟢常亮
t=6    hook: Stop               → apply_event → Waiting + is_flashing  🟢闪烁
                                   （一轮完成：先绿闪）
t=36   cleanup: retention 到期  → Waiting，关闪烁 → 🟡常亮
                                   （对话未结束，黄灯保留）
       ... 用户思考中 ...
t=60   hook: UserPromptSubmit   → apply_event → Running  🟢常亮
                                   （旧 Waiting 被自动清理）
t=61   hook: PreToolUse(Bash)   → apply_event → Running  🟢常亮
t=62   hook: SessionEnd(reason=prompt_input_exit)
                                → apply_event → Completed 🟢闪烁
                                   （对话结束：绿闪后移除）
t=92   cleanup: now ≥ remove_at → 删除 Completed 会话 → 灯消失
```

**错误转灯示例：**

```text
t=0    StopFailure              → Error     🔴
t=1    PreToolUse               → Running   🟢（红灯不粘，执行事件转绿）
t=2    Stop                     → Waiting   🟢闪烁 → 落黄
t=3    SessionEnd               → Completed 🟢闪烁 → 移除
```

唯一例外：`Unknown`（无法识别的事件）不构成转灯信号，红灯保持。

**apply_event 内部逐步解析：**

```text
事件进入 apply_event
    │
    ├─ ① 过滤层：schema/event_id/session_id 去重/sequence/Completed 保护
    │     任一不通过 → return false，状态不变
    │
    ├─ ② 查询 previous：sessions.get(key)
    │     提取 previous_phase、previous_started_at、previous_pid、previous_error_code
    │
    ├─ ③ phase_for_event 判定：
    │     error_code/失败 outcome → Error（红灯）
    │     精确匹配事件名 → Running/Waiting/Completed/Error
    │     Notification 看 notification_type 分流
    │     SessionEnd 一律 Completed（不看 reason）
    │     fallback 子串匹配 → Unknown
    │
    ├─ ④ 红灯不粘：
    │     事件映射出的 phase 即最终 phase，任何转灯信号都生效
    │     例外：previous==Error 且新 phase==Unknown → 保持 Error
    │       （无法识别的事件不构成转灯信号）
    │
    ├─ ⑤ 清理旧 Waiting：
    │     if phase == Running
    │       删除同一 provider 下所有 ≠ 当前 session 的 Waiting 会话
    │       （这就是"黄灯保留到下次运行"的核心机制）
    │
    ├─ ⑥ 构建 SessionSnapshot：
    │     phase → 决定灯的基础颜色
    │     Stop Waiting → is_flashing=true，remove_at=闪烁结束（不移除）
    │     Completed → is_flashing=true，remove_at=移除时间
    │     其它 Waiting → 不闪，直接黄
    │     last_seen_at_ms = now → 用于超时判断
    │     pid/error_code → 继承自 previous 或来自新事件
    │
    └─ ⑦ 写入 sessions 表，revision++
          return true → 触发 emit_snapshot → 前端更新
```

**cleanup 对 Stop 绿闪的额外处理：**

```text
Waiting && is_flashing && now ≥ remove_at
  → is_flashing=false，remove_at=None（落成黄常亮，会话保留）
Completed && now ≥ remove_at
  → 删除会话
```

**cleanup 定时清理决策树：**

```text
cleanup(now, running_timeout_ms, is_alive) 每 10 秒执行
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
    ├─ phase == Running？
    │   ├─ now - last_seen_at_ms > running_timeout_ms？ → 删除（可配置，默认 60s）
    │   └─ is_alive(session) == Some(false)？ → 删除（进程死亡）
    │      否则 → 保留
    │
    └─ phase == Error/Unknown？
        ├─ now - last_seen_at_ms > 60s？ → 删除（短超时）
        └─ is_alive(session) == Some(false)？ → 删除（进程死亡）
           否则 → 保留
```

**快照排序：**

`snapshot()` 输出的 `sessions` 按**创建时间 `started_at_ms` 升序**排列，最早的会话排在最前面；同一时间戳（或同毫秒）时用 `key` 字母序兜底，保证顺序确定、可复现。

用创建时间而非 `last_seen_at_ms`：后者会让任一会话每次活动都重排，物理格子里的指示灯会不停跳动。悬浮窗与主窗口的会话列表都直接消费这个顺序。

**前端渲染决策：**

```text
snapshot → store.applySnapshot（revision 检查，忽略旧快照）
         → sessions 响应式更新
         → AgentStatusModule.vue 遍历 sessions
         → 每个 session 根据 colorForSession 决定颜色：
             running/completed           → 绿色圆点（--ufo-status-green）
             waiting && is_flashing      → 绿色圆点（Stop 绿闪期间）
             waiting/unknown             → 黄色圆点（--ufo-status-yellow）
             error                       → 红色圆点（--ufo-status-red）
           根据 is_flashing 决定是否加闪烁动画
             （Completed 绿闪后移除；Stop Waiting 绿闪后落黄）
```

### 2.4 hook.rs — Hook Bridge

入口：`try_run_status_hook(args)` 在 `main.rs` 中调用，匹配 `--status-hook` 参数后短路退出，不启动 Tauri 窗口。

流程：

1. 从参数提取 `--provider`（支持 `=` 和空格两种格式）。
2. 从 stdin 读取 JSON，限制 64 KiB。
3. 从 payload 提取白名单字段（session_id、event、pid、event_id、sequence、error_code、outcome、notification_type 等），支持多种命名风格。
4. 归一化为 `AgentEvent`。
5. 写入 `inbox/<unique-id>.json`：先写临时文件 `.tmp`，`sync_all` 后原子 rename。
6. session_id 长度限制 256 字符。

状态目录优先级：`--status-dir` 参数 > `UFO_STATUS_DIR` 环境变量 > 默认 app-data 路径。

`outcome` 同时承接 SessionEnd.`reason` 等结果别名；`notification_type` 单独提取，避免与 outcome 混用污染状态判定。

**prompt 元标签剔除（`normalize_prompt` → `strip_excluded_tags`）：**

Claude Code 上报的 `UserPromptSubmit.prompt` 会混入自己注入的元信息标签（`<ide_opened_file>`、`<system-reminder>`、`<env>`、`<permission_mode>` 等），会淹没用户真正的提问。`normalize_prompt` 在 trim / 截断之前先调用 `strip_excluded_tags`，**连同标签包裹的整段内容一起移除**（按需求：不保留标签内文字），只留下用户手输的文本。

- 名单来自仓库根 `.env` 的 `UFO_STRIP_TAGS`（逗号分隔，尖括号会被剥掉，写 `<name>` 或 `name` 均可）。
- ⚠️ 该值在**构建期**由 `src-tauri/build.rs` 读取并 `cargo:rustc-env` 注入二进制（`option_env!`）。原因：hook 是独立短命进程、cwd 为**被监测 agent 的项目目录**，运行时读不到 UFO 仓库的 `.env`，且 hook 必须快进快出不做文件 IO。**改 `.env` 后必须重新构建才生效**；`build.rs` 已 `rerun-if-changed=../.env`。
- `.env` 缺失 / 留空时回落到 `hook.rs` 的 `DEFAULT_STRIP_TAGS`（与 `.env` 默认值一致）。
- 匹配规则：仅命中「`<` + 名单名 + 边界字符（`>` / `/` / 空白）」的开始标签，因此 `<environment>` 不会被名单里的 `env` 误吞；名单外的尖括号内容（代码里的 `<T>`、`<div>` 等）原样保留。
- 找不到显式 `</name>` 时退化为只吃掉起始标记（覆盖自闭合 `<tag/>`、prompt 被截断丢了闭合标签），不吞后续正文。
- 全部被剔除后为空 → `normalize_prompt` 返回 `None`，由 reducer 继承上一轮的值。

### 2.5 mod.rs — 监测器

`StatusState` 持有：

- `registry: Arc<Mutex<StatusRegistry>>`
- `status_dir: PathBuf`
- `retention_ms: Arc<Mutex<u64>>` — 与 module_config 联动，配置变更时同步更新。
- `running_timeout_ms: Arc<Mutex<u64>>` — Running 无新 hook 超时，与 Error 的 60s 分离；配置变更时同步。

`monitor_loop` 在独立线程运行：

1. 每 500ms 扫描 inbox 目录。
2. 对每个 `.json` 文件：解析成功则消费并删除，同时 emit `status-hook-event` 到前端 DevTools 日志；失败则移入 quarantine 目录并标记 source_health 为 degraded。
3. 每 10 秒执行一次 cleanup。
4. 任何状态变更后调用 `emit_snapshot` 广播 `agent-status` 事件。

`process_alive` 实现：

- Windows：`tasklist /FI "PID eq {pid}" /FO CSV /NH`，检查输出是否包含 PID。
- 非 Windows：`kill -0 {pid}`，检查退出码。

### 2.6 tokens.rs — transcript token 统计

悬浮窗要展示「本会话已用掉多少 token」，数据源是 CLI 自己写的 transcript（JSONL）。

**为什么不在 hook 里解析**：hook 每次事件都要新起一个进程，必须快进快出；而 transcript 会增长到 MB 级。所以 hook 只透传 `transcript_path`，解析放在常驻 monitor 线程里，并且是**增量**的（只读上次之后的追加字节）。

`TokenTracker` 由 monitor 线程独占持有，因此内部不加锁：

| 成员 | 作用 |
|---|---|
| `sessions: HashMap<String, SessionTokens>` | 按 `provider:session_id` 索引 |
| `main_path` | 主 transcript 路径，用于推导子代理目录 |
| `files: HashMap<PathBuf, FileCursor>` | 已登记增量读取的文件（主 transcript + 各子代理） |
| `seen_messages: HashSet<String>` | 已计入的 `message.id` —— 去重的关键 |
| `totals: TokenTotals` | 累计值 |
| `subagents_scanned` | 子代理目录只需扫描一次 |

`FileCursor` 记录 `offset`（已消费字节）与 `pending`（未以换行结尾的残行）。**`pending` 用 `Vec<u8>` 而非 `String`**：文件正在被追加，末尾可能停在 UTF-8 序列中间，提前转成字符串会破坏字符。

**read_appended 流程：**

1. `metadata.len() < offset` → 文件被截断或整体替换，offset 归零重读，**但已累计的 totals 保留**（宁可少算，也不要把用户已经看到的数字抹掉）。
2. `len == offset` → 无新增，直接返回。
3. 从 offset 读到文件末尾，与上次残留的 `pending` 拼接后按 `\n` 切分。
4. 末尾没有换行的部分存回 `pending`，留到下次（半行不参与统计）。

**consume_line 规则：**

只认 `type == "assistant"` 且带 `message.usage` 的记录，按 `message.id` 去重后累加四项：`input_tokens`、`cache_creation_input_tokens`、`cache_read_input_tokens`、`output_tokens`。

- ⚠️ **必须按 `message.id` 去重**：同一条 API 响应在 transcript 里会写成多条 assistant 记录，每条都带同一份 usage。实测不去重会让总量虚高约 2.6 倍（某会话 53 条带 usage 的记录 → 仅 20 个唯一 id）。
- `output_tokens` 已包含 `output_tokens_details.thinking_tokens`，**不能**再单独相加。
- 没有 usage 的记录**不占用**去重名额，避免把后续真正带 usage 的同 id 记录挡掉。
- 任何解析失败都静默跳过：transcript 不是稳定 API，格式变了最多统计不到，绝不能让 monitor 线程出错。

**子代理 transcript：** 路径由主 transcript 推导 —— `<project>/<session-id>.jsonl` → `<project>/<session-id>/subagents/*.jsonl`。首次 refresh 扫描一次并登记全部 `.jsonl`；目录不存在（该会话没用子代理）返回空，不是错误。子代理消耗**并入同一会话**，否则重度使用子代理的会话会明显偏低。

**与注册表的衔接（mod.rs）：**

- `observe(key, path)` 在消费 hook 事件时登记路径，幂等。
- 每轮循环 `refresh_all()`，只有累计值真的变化才返回，进而调用 `StatusRegistry::set_tokens`。
- `set_tokens` 在会话不存在时拒绝写入，避免为已消失的灯留下孤儿记录；会话被 cleanup 移除后由 `retain` 同步丢弃统计，防止长期运行下无限增长。
- `tokens` **不是事件派生字段**，由 `StatusRegistry::snapshot()` 时从 `token_totals` 合并进 `SessionSnapshot`。

### 2.7 module_config.rs — 模块配置

`FloatingModuleConfig` 字段：

| 字段 | 默认值 | 说明 |
|---|---|---|
| schema_version | 1 | 固定 |
| revision | 1 | 每次写入递增 |
| modules | `[{id:"agent-status",enabled:true,order:0}]` | 模块列表 |
| completed_retention_ms | 30000 | 完成灯保留时长，clamp 1000~86400000 |
| running_timeout_ms | 60000 | Running 无新 hook 超时，clamp 60000~3600000；旧配置缺字段回落默认 |
| max_cells_by_layout | 各形态 8 | 方形/四角/横条/竖条格子上限，各 clamp 4~8；旧 `max_visible_sessions` 加载时迁移后废弃 |
| token_unit | `tokens` | token 展示单位：`tokens` 原始计数、`m_tokens` 百万；无取值范围约束 |
| proxy_resolver | `none` | 选中的代理解析器 id（如 `cc-switch`）；`none` 不解析。旧配置缺字段或空串回落 `none`；写入时同步更新 `StatusState.proxy`，切换即时生效 |

`token_unit` 的解析是**宽松**的：序列化用 `#[serde(rename_all = "snake_case")]` 得到 `"tokens"` / `"m_tokens"`，但加载时经 `parse_token_unit` 只认这两个字符串，未知取值回落 `tokens`。

之所以不直接反序列化成枚举：`ModuleState::load` 一旦反序列化失败就整份配置 `.unwrap_or_default()`，手改配置把 `token_unit` 写错一个字符会连带把保留时长、超时、格子上限全部重置。宽松解析把这个影响限制在单位本身（有回归测试 `unknown_token_unit_does_not_discard_the_rest_of_the_config` 锁住）。

`validate` 规则：

- schema_version 必须为 1。
- module id 长度 ≤ 64，不允许 `/` 或 `\`。
- 缺少 `agent-status` 时自动补全。

配置文件路径：`<app-data>/floating-modules.json`。

写入策略：先写 `.tmp`，删除旧文件，rename 新文件（Windows 兼容）。

配置变更时同步更新 `StatusState.retention_ms`、`StatusState.running_timeout_ms` 与 `StatusState.proxy`（解析器选择），使运行中的监测器立即采用新值。

### 2.8 proxy/ — 代理解析器

某些代理工具（如 cc-switch）会先启一个本地代理（`127.0.0.1:15721`），Claude 配置的 `ANTHROPIC_BASE_URL` 指向本地代理，真实上游（`https://api.b.ai`）写在代理自己的数据库里。状态指示灯若只显示 `127.0.0.1:15721` 没有信息量，需要反查出上游。

**数据链路：**

```
~/.claude/settings.json  →  base_url = http://127.0.0.1:15721
                                  ↓（cc-switch proxy_config 匹配监听地址）
                         ~/.cc-switch/cc-switch.db（SQLite，只读打开）
                                  ↓（providers WHERE app_type='claude' AND is_current=1）
                         settings_config JSON → env.ANTHROPIC_BASE_URL = https://api.b.ai
                                  ↓
SessionSnapshot.upstream_url = https://api.b.ai（前端 Endpoint 行优先显示此值）
```

**ProxyResolver trait：**

每种代理工具实现一次 trait，在 `build_registry` 里注册一行即可接入：

```rust
pub trait ProxyResolver: Send + Sync {
    fn id(&self) -> &'static str;           // 存进配置、下拉 value
    fn label(&self) -> &'static str;        // 下拉显示文本
    fn resolve_upstream(&self, app: AgentApp, local_base_url: &str) -> Option<String>;
}
```

`AgentApp`（Claude/Codex）独立于 `Provider`，将来代理工具支持更多 agent app 时不改状态协议。实现必须只读、快速、不 panic；内部可自带 TTL 缓存控制读库频率。

**CcSwitchResolver 实现细节：**

- `root = ~/.cc-switch`（home 探测来自 `status::hook::home_dir`；`cc-switch.db` 不存在时 root 为 None，恒不解析）。
- 每次 `resolve_upstream`：先查 HashMap 缓存（5 秒 TTL）；miss 时以 `SQLITE_OPEN_READ_ONLY` 打开 db（`busy_timeout 200ms`）。
- 匹配条件：① `proxy_config` 里该 app 的**全局服务器开关 `proxy_enabled=1` 且本 app 接管开关 `enabled=1`**；② `base_url` 的 `normalize_host_port` 与 `listen_address:listen_port` 相等（localhost 与 127.0.0.1 等价，省略默认端口）。两列语义取自 cc-switch 源码（`schema.rs` / `services/proxy.rs` / `dao/proxy.rs`）：`proxy_enabled` 是代理服务器总开关（三行镜像同值，服务器 stop 时置 0），`enabled` 是该 app 是否被接管（每 app 独立）。只看 `proxy_enabled` 会在「服务器开着但该 app 未接管」时误命中，故取两者之与。
- ⚠️ `listen_port` 在 cc-switch 真实 schema 里是 **`INTEGER`**（`DEFAULT 15721`），必须按整数读：rusqlite 的 `String::column_result` 只接受 Text，用 `String` 读 INTEGER 列会 `InvalidType` **整行失败**，导致 `listen` 恒为 None、整个功能空转（历史 bug，现由 `seed_db` 用 INTEGER 列 + 全部 ccswitch 测试锁定）。`listen_address` 才是 TEXT。
- 命中条件只看 db 的这两列与会话实际 base_url，**不读 `settings.json` 的 `enableLocalProxy` 总开关**：cc-switch 关代理时会把 `ANTHROPIC_BASE_URL` 改回真实上游，会话 base_url 不再匹配本地监听地址，自然回落不解析。
- 查 `providers WHERE app_type=... AND is_current=1` 的 `settings_config`，`settings_config` 整块 JSON 会进内存解析，但**只提取 `env.ANTHROPIC_BASE_URL` 这一个 key**，同块里的 token / api key 一律不读取、不缓存、不上报。
- Codex 的 `settings_config` 里 base_url 走的是内嵌 TOML（`config` 字段），不是 `env.ANTHROPIC_BASE_URL`，因此 Codex 会话命中代理后 `upstream` 读不到、回落本地地址（优雅降级，目前只反查 Claude）。
- 结果（包括 None）都写进缓存；TTL 内重复调用不再读库。

**ProxyState（跨窗口共享）：**

`selected: Arc<Mutex<String>>` + `registry: Arc<ResolverRegistry>`。`set_selected` 由 `module_config::set_floating_module_config` 写入路径同步调用，切换立即生效。

**快照填充（`attach_upstreams`）：**

`StatusState::snapshot_with_upstream()` 在 `registry.snapshot()` 后调用 `attach_upstreams`：选中 `none` 时直接返回；否则对每个有 `base_url` 的会话调用 `proxy.resolve_upstream`，填 `upstream_url`。`get_status_snapshot` 与 `emit_snapshot` 都走这条路。

**Monitor 循环的 upstream_signature 监测：**

启用解析器时 monitor 每 500ms 调 `state.upstream_signature()`（对当前存活会话算 key→upstream 的有序列表）。签名变化时 `changed=true`，触发 `emit_snapshot`。这让 cc-switch 里手动切 provider（UFO 侧无新 hook 事件）在缓存 TTL 到期后（最多 5 秒）自动推送到前端。`upstream_signature` 先在 registry 锁内拷出 `(key, provider, base_url)` 三元组随即释放锁，再到锁外逐会话 `resolve_upstream`（可能触发读库），避免把 `apply_event` 阻塞在最坏 200ms 的 SQLite 读上。

**normalize_host_port 辅助函数：**

- 剥离 scheme、userinfo、path/query/fragment。
- IPv6 形式 `[::1]:port` 与普通 `host:port` 均处理。
- loopback 等价写法（`localhost`/`127.0.0.1`/`::1`/`[::1]`）统一成 `127.0.0.1`。
- 省略默认端口（http→80、https→443）。
- host 非数字端口 → None（不解析）。

**前端 Endpoint 展示（`displayEndpoint`）：**

`src/lib/agent-status.ts` 的纯函数 `displayEndpoint(session)`：`upstream_url ?? base_url`，两者都缺返回 null。TooltipWindow.vue 的 Endpoint 行直接用这个函数；当上游与原值不同时 `title` 标注本地代理地址（「经本地代理 xxx」）。

**主窗口下拉：**

`MainWindow.vue` 在「Token 单位」设置下方渲染 `<select class="ufo-input ufo-select">`，选项来自 `ProxyResolverInfo[]`（store 在 `start()` 里 invoke `get_proxy_resolvers` 拉取）。选项含 `none` 恒在首位。change 调 `modules.updateProxyResolver(id)` 持久化并即时同步给状态监测。

**接入新代理工具：**

只需：① 在 `src/proxy/` 下新建文件实现 `ProxyResolver`；② 在 `build_registry` 里 push 一行。前端无需任何改动——下拉、upstream 填充和持久化全部自动适配。

### 2.9 lib.rs — Tauri 集成

setup 阶段：

1. 获取 app-data 目录。
2. 加载 `ModuleState`（从 `floating-modules.json`）。
3. 构建 `ProxyState`，并用 `module_state.proxy_resolver()` 设入初值（重启后不回落 none）。
4. 用 module_config 的 retention_ms / running_timeout_ms + ProxyState 初始化 `StatusState`。
5. `app.manage(StatusState)` + `app.manage(ModuleState)`。
6. `spawn_monitor` 启动唯一监测线程。
7. 注册托盘菜单和 devtools。

注册的 commands：

- `get_status_snapshot` — 读取当前全量快照（含 upstream_url）。
- `get_proxy_resolvers` — 列出可用代理解析器（供主窗口下拉）。
- `get_floating_module_config` — 读取模块配置。
- `set_floating_module_config` — 写入模块配置（仅 main 窗口有权限）。

### 2.10 权限

`src-tauri/permissions/status.toml`：

- `allow-get-status-snapshot` → `get_status_snapshot`
- `allow-get-proxy-resolvers` → `get_proxy_resolvers`
- `allow-get-floating-module-config` → `get_floating_module_config`
- `allow-set-floating-module-config` → `set_floating_module_config`

`src-tauri/capabilities/default.json`（main + floating + tooltip）：

- core:default、core:event:default
- 窗口操作（hide/show/set-focus/set-position/set-size/start-dragging/unminimize）
- allow-get-status-snapshot、allow-get-proxy-resolvers、allow-get-floating-module-config

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
    agent-status.ts               ← 类型定义、runtime guard、listen/invoke 封装、颜色与 token 格式化
    agent-status-layout.ts        ← layoutFromDocked、格子数/网格/溢出切分
    floating-modules.ts           ← 模块定义、配置类型（含 TokenUnit）、isModuleEnabled
    tooltip.ts                    ← 提示窗 label/事件名、TooltipHoverPayload、placeTooltip 定位
    snap.ts                       ← 贴边算法（已有）
    window.ts                     ← showMainWindow（已有）
  stores/
    agentStatus.ts                ← Pinia store：snapshot、connection、start/stop
    floatingModules.ts            ← Pinia store：config、agentStatusEnabled、retention、tokenUnit
  components/
    FloatingWindow.vue            ← 拖拽、点击、贴边、layoutMode → FloatingModuleHost
    FloatingModuleHost.vue        ← 模块槽位
    AgentStatusModule.vue         ← 状态灯渲染
    TooltipWindow.vue             ← 独立提示窗：悬停会话详情
    MainWindow.vue                ← 主窗口：模块开关、保留时长、token 单位、会话列表
```

### 3.2 agent-status.ts — 类型与 API

类型定义与 Rust model.rs 完全对应：

- `Provider`: `'claude' | 'codex'`
- `SessionPhase`: `'running' | 'waiting' | 'error' | 'unknown' | 'completed'`
- `SessionSnapshot`: 含 key、provider、session_id、phase、source、confidence、started_at_ms、last_seen_at_ms、completed_at_ms、remove_at_ms、is_flashing、pid、error_code、base_url、upstream_url、prompt、tokens、last_sequence、termination_confirmed
- `TokenTotals`: input_tokens、cache_creation_input_tokens、cache_read_input_tokens、output_tokens
- `StatusSnapshot`: 含 schema_version、revision、generated_at_ms、sessions[]、counts、source_health

`isStatusSnapshot` runtime guard 校验：

- schema_version === 1
- revision 为非负安全整数
- generated_at_ms 为 number
- sessions 为数组且长度 ≤ 1000
- 每个 session 校验 key（string）、provider（claude/codex）、session_id（string）、phase（string）、termination_confirmed（boolean）

格式化辅助：

| 函数 | 作用 |
|---|---|
| `tokenTotal(tokens)` | 四项求和，与 Rust `TokenTotals::total()` 同口径；null 按 0 |
| `formatTokenCount(n)` | 千分位整数（`1,261,847`） |
| `formatTokenMillions(n)` | 百万（`1.26M`），不足 0.01M 显示 `<0.01M` |

| `displayEndpoint(session)` | Endpoint 展示值：`upstream_url ?? base_url`，两者都缺返回 null（提示窗据此优先显示反查出的上游） |

`colorForSession` 映射：

| phase | 颜色 | 表现 |
|---|---|---|
| running | green | 常亮 |
| completed | green | 配合 `is_flashing` 闪烁后移除 |
| waiting + `is_flashing` | green | Stop 绿闪期间 |
| waiting | yellow | 常亮（含 Stop 闪完后的稳态） |
| unknown | yellow | 常亮 |
| error | red | 常亮 |

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

`windowCellsForSessions(sessionCount, layoutMax)` — 悬浮窗壳应采用的物理格子数：不超过默认 4 时保持 4，超过后放大到该形态上限（clamp 到 4..8）。

`indicatorGrid(layout, cells)` — 指示灯网格列行数：横条 `n×1`、竖条 `1×n`、方形/四角 `side×side`（`side = ceil(sqrt(cells))`）。

`indicatorCapacity` / `partitionIndicatorSessions(sessions, layout, layoutMax, cells)` — 按形态容量切分：放得下则全显；超出时留出最后一格给 `+N`。方形/四角一旦超过上限，容量改按网格 `side²`，让 `+N` 占满最后一格，避免 3×3 空一格。

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
- `updateRunningTimeoutSeconds(seconds)` — 更新 Running 无新 hook 超时（60~3600 秒）。
- `updateMaxCellsByLayout(partial)` — 更新各形态格子上限（各 4~8）。
- `updateTokenUnit(unit)` — 更新 token 展示单位（`tokens` / `m_tokens`）。
- `stop()` — 释放 listener。

getters：`agentStatusEnabled`、`retentionSeconds`、`runningTimeoutSeconds`、`maxCellsByLayout`、`tokenUnit`。

`applyConfig` 用 revision 防旧覆盖新。

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

**三个 WebView（main / floating / tooltip）都要启动这两个 store**，顺序不能省任何一个：

- `agentStatus` —— 提示窗要读会话数据（`sessions`）。
- `floatingModules` —— 提示窗要读 token 展示单位；漏掉它会导致主窗口切换单位后提示窗不生效。

### 3.8 FloatingWindow.vue — 悬浮窗

职责：拖拽、点击打开主窗口、调用贴边算法、维护 layoutMode。

贴边流程：

1. 拖拽结束后 `scheduleSnap()`（140ms 防抖）。
2. `snapWindow()` 读取窗口物理坐标和尺寸，调用 `snapToWorkArea`。
3. 吸附后更新 `layoutMode = layoutFromDocked(result.docked)`。
4. layoutMode 传递给 FloatingModuleHost。

监听 `onMoved` 和 `onScaleChanged` 事件，自动重新吸附。

### 3.9 FloatingModuleHost.vue — 模块宿主

接收 `layoutMode` prop，从 `floatingModules` store 取已启用模块定义，渲染**单个** `__slot--status` 槽位，把 `layoutMode` 透传给模块组件。

模块只能来自编译期注册表（`createFloatingModuleDefinitions`），配置里的任意字符串无法变成可执行组件，避免不可信模块注入。

### 3.10 AgentStatusModule.vue — 状态灯

接收 `layoutMode` prop，从 `agentStatus` store 取 `sessions`。

**格子数与网格**：`windowCellsForSessions(会话数, 该形态上限)` 决定壳尺寸 —— 会话数不超过默认 4 时保持默认，超过后放大到该形态上限。再经 `indicatorGrid(layoutMode, cells)` 得到列行数（横条 `n×1`、竖条 `1×n`、方形/四角 `side×side`）。

模块通过 `floatingWindowKey` 上下文调用 `setWindowCells(cells)` 让悬浮窗壳按需调整窗口尺寸。

**溢出处理**：`partitionIndicatorSessions` 按容量切分，超出时留最后一格给 `+N`，不使用聚合灯。方形/四角超过上限时容量改按网格 `side²`，让 `+N` 占满最后一格，避免 3×3 空一格。

**无障碍**：只设 `aria-label`（provider、短 session id、phase）。

⚠️ **不要给指示灯加 `title`**：`title` 会触发 Windows 原生文字提示，与悬浮窗自己的 tooltip 重复且更简陋；`aria-label` 不产生可见提示，无障碍信息只走它。

### 3.11 MainWindow.vue — 主窗口设置

模块开关：checkbox 绑定 `modules.agentStatusEnabled`，change 时调用 `modules.updateAgentStatus`，失败时回滚 UI。

保留时长：number input 绑定 `retentionInput`，change 时调用 `modules.updateRetentionSeconds`，失败时回滚。

执行中超时：number input 绑定 `runningTimeoutInput`（60–3600 秒，默认 60），change / 保存时调用 `modules.updateRunningTimeoutSeconds`，失败时回滚；即时同步到 `StatusState.running_timeout_ms`。

Token 单位：分段选择控件（`.ufo-segmented`）绑定 `tokenUnitInput`（`tokens` / `m_tokens`），change 时调用 `modules.updateTokenUnit`，失败时回滚。选中态用 `:has(input:checked)` 跟随原生 radio，不需要额外 JS 状态。

代理解析器：`<select class="ufo-input ufo-select">` 绑定 `proxyResolverInput`，选项来自 `modules.resolvers`（store 在 `start()` 时 invoke `get_proxy_resolvers` 拉取，`none` 恒在首位），change 时调用 `modules.updateProxyResolver`，失败时回滚；后端写入路径即时同步到 `StatusState.proxy`。

会话列表：遍历 `agentStatus.sessions`，显示彩色圆点、provider、短 session id 和 phase。

### 3.12 CSS 关键样式

悬浮窗透明背景：

```css
html.is-floating-window,
html.is-floating-window body,
html.is-floating-window #app {
  background: transparent;
}
```

毛玻璃效果（悬浮窗，`--ufo-bg` = `rgba(61,61,61,0.8)`）：

```css
.floating-shell {
  background-color: var(--ufo-bg);
  backdrop-filter: blur(6px);
  border: 1px solid rgba(255, 255, 255, 0.18);
}
```

⚠️ 主窗不同：`.main-shell` 用**实色** `--ufo-bg-main`（`#3D3D3D`），**不加** backdrop-filter、不加 alpha。三窗口的透明策略见 `CLAUDE.md`。

闪烁动画（`is_flashing`：Completed 或 Stop 刚完成）：

```css
@keyframes agent-status-flash {
  0%, 100% { opacity: 1; transform: scale(1); }
  50% { opacity: 0.35; transform: scale(0.78); }
}
```

Running 为绿常亮，不加脉动。闪烁用于：
- Completed：绿闪后移除
- Waiting+Stop：绿闪后落成黄常亮

状态灯颜色走 CSS 变量（**不是** Tailwind 的 emerald/amber/rose 色阶）：green → `--ufo-status-green`（`#34d399`），yellow → `--ufo-status-yellow`（`#fbbf24`），red → `--ufo-status-red`（`#f87171`）。完整设计 token 见 `src/assets/main.css`。

### 3.13 TooltipWindow.vue + tooltip.ts — 悬停提示窗

提示窗是**第三个独立窗口**（label `tooltip`，300×190），在 `tauri.conf.json` 中声明并 `visible: false`，按需 show。它不是运行时创建的，因此不需要 `core:webview:allow-create-webview-window`。

**为什么拆成独立窗口**：提示内容比悬浮窗大得多，做成独立窗口可以不触发悬浮窗 resize、不影响拖拽与贴边。

**悬停 → 显示的完整链路：**

```text
指示灯 pointerenter
    │
    ▼
AgentStatusModule → floatingWindowKey 上下文的 onSessionHover(key)
    │
    ▼
FloatingWindow.vue：延迟 250ms（SHOW_DELAY_MS）后 emitTo('tooltip', 'tooltip-hover', payload)
    │
    ▼
TooltipWindow.vue：listen 该事件 → hover = payload
    │
    ▼
displayKey 变为非 null → present(payload)：先 setPosition 再 show
```

- **payload 只传「哪个会话 + 悬浮窗外框 + 贴边状态」**（`TooltipHoverPayload`），会话数据由提示窗自己订阅 `agent-status` 取。这样避免 payload 里的数据陈旧。
- `displayKey` 用**会话 key 而非布尔值**做 watch 源，保证从 A 灯移到 B 灯时也会重新定位。
- `pointerleave` 后延迟 120ms（`HIDE_DELAY_MS`）才隐藏，避免在相邻指示灯之间移动时反复开关。

**定位**：纯函数 `placeTooltip(anchor, tooltipSize, workArea, docked)`，优先放在悬浮窗**未贴边**的一侧，越界自动换侧；全部放不下则退回首选侧并夹进工作区。以 anchor 中心调 `monitorFromPoint` 取屏 —— 多显示器下提示窗自身所在屏未必与悬浮窗同屏。

**必须 `dismissTooltip()` 的时机**：拖拽开始（`startDragging()` 后指针交给系统，`pointerleave` 可能不再触发）、单击、贴边重算。

**展示字段：**

| 字段 | 说明 |
|---|---|
| provider / phase | 顶部行：状态点 + provider 图标 + 名称 + phase 徽标 |
| Endpoint | `displayEndpoint` = `upstream_url ?? base_url`，未知显示 `—`；命中解析出的上游时 `title` 标注「经本地代理 <base_url>」 |
| 已使用 token | 会话累计总量，单位由主窗口配置决定（见下）；四项明细挂在 `title` 上 |
| 提问 | `prompt`，最多 4 行后省略，完整内容挂 `title` |

**不展示 session id。**

**token 单位**：`modules.tokenUnit`（来自模块配置）决定写法。

| 单位 | 显示 | 格式化函数 |
|---|---|---|
| `tokens` | `1,261,847` | `formatTokenCount` |
| `m_tokens` | `1.26M` | `formatTokenMillions` |

- `formatTokenMillions` 保留两位小数后去掉末尾的 0（`1.00M` → `1M`）；**不足 0.01M 时显示 `<0.01M`**，否则小数位不足以表达，会退化成没有信息量的 `0M`。
- 不使用 `toLocaleString`：其输出依赖运行环境的 ICU 数据，会让测试结果不稳定。
- 未统计到（`tokens` 为 `null`）或总量为 0 时**不渲染该行**。
- 明细（`title`）**始终用原始计数**，保证切到 M 单位后仍能查到精确值。

⚠️ **提示窗窗口必须在 `App.vue` 中 `floatingModules.start()`**：它的 token 单位来自模块配置，若不订阅就永远停留在 `fallbackConfig` 的默认单位，主窗口切换后不生效。

**点击穿透**：Rust 侧 `setup` 中 `set_ignore_cursor_events(true)`，避免遮挡或拦截下方窗口的点击。Rust 调用不经过 capability 校验，无需额外权限。

⚠️ 但点击穿透（Win32 的 `WS_EX_TRANSPARENT`）会让页面收不到鼠标移动，配合配置里的 `focusable: false`（`WS_EX_NOACTIVATE`），两者叠加会让 devtools 的元素检查选取器失效。因此**调试构建下放开这两个限制**（`set_focusable(true)` 且不设穿透），仅 release 保持穿透。

**尺寸**：由 `tauri.conf.json` 给出且 `resizable: false`，外壳 `h-full` + `overflow-hidden`，内容再多也只能裁切、不会改变窗口高度。它**不适用**方形/横条/竖条三形态适配。

## 4. 测试覆盖

### 4.1 Rust 测试（143 个）

按模块：`reducer` 39、`hook` 43、`module_config` 26、`tokens` 12、`model` 10、`proxy` 13（`proxy::tests` 4 + `ccswitch::tests` 9）。

**model.rs：**

- provider_parse_known_values — claude/claude-code/CLAUDE/codex/codex-cli/CODEX
- provider_parse_rejects_unknown — 空字符串、unknown、gpt
- session_key_includes_provider_namespace — claude:abc / codex:abc
- now_ms_returns_positive_value
- agent_event_serialization_roundtrip — 含 notification_type / base_url / prompt / transcript_path
- agent_event_missing_notification_type_deserializes_as_none — 旧 inbox 兼容
- agent_event_missing_base_url_deserializes_as_none — 旧配置兼容
- token_totals_total_sums_all_four_fields / token_totals_default_is_zero
- session_snapshot_missing_tokens_deserializes_as_none — 旧快照兼容

**tokens.rs（12 个）：**

- counts_a_single_assistant_record — 四项求和，thinking 不重复计入
- deduplicates_repeated_message_ids — 同 message.id 只计一次
- sums_distinct_messages
- ignores_non_assistant_and_usage_less_records
- usage_less_record_does_not_block_a_later_one — 无 usage 的记录不占去重名额
- malformed_lines_are_skipped
- only_reads_appended_bytes_on_refresh — 增量只读追加字节
- waits_for_the_trailing_newline — 残行不计入
- truncated_file_resets_offset_without_losing_totals
- counts_subagent_transcripts_into_the_same_session
- missing_transcript_is_not_an_error
- retain_drops_sessions_that_are_gone

**reducer.rs 关键断言（重排后）：**

- stop_means_waiting_with_green_flash_then_settles_yellow
- permission_waiting_does_not_flash
- permission_request_and_elicitation_mean_waiting
- waiting_notification_types_map_to_waiting
- non_waiting_or_missing_notification_type_maps_to_running
- permission_denied_and_task_completed_stay_running
- session_end_is_the_completed_signal_regardless_of_reason
- stop_failure_maps_to_error
- unknown_event_does_not_clear_error_state — 红灯不粘的唯一例外
- user_prompt_submit_recovers_from_error
- waiting_sessions_cleared_when_new_run_starts
- waiting_sessions_not_cleared_by_other_provider
- completed_sessions_flash_until_retention_expires
- prompt_is_kept_across_the_turn_and_replaced_on_next_submit
- sessions_are_ordered_earliest_created_first — 按 started_at_ms 升序，最近活动的会话不改变排位
- keeps_multiple_provider_sessions_separate — 跨 provider 也按创建先后，Claude 先建排前
- 以及既有去重 / sequence / schema / retention / liveness 等测试

**hook.rs（43 个）：**

- argument_value_from_equals_syntax — `--provider=claude`
- argument_value_from_separate_args — `--provider claude`
- argument_value_returns_none_for_missing
- argument_value_rejects_flag_as_value — `--provider --other` → None
- string_field_reads_first_matching_name / number_field_*
- try_run_status_hook_returns_none_without_flag
- default_status_dir_contains_app_identifier / unique_event_ids_are_unique
- read_settings_base_url_* — 配置文件优先级、缺失/损坏文件跳过、later file overrides earlier
- provider_base_url_prefers_settings_over_env / provider_base_url_ignores_claude_settings_for_codex
- normalize_base_url_* — 原样保留（含 userinfo/query）、trim、空值与超长拒绝
- normalize_prompt_* — trim、空值拒绝、按**字符**而非字节截断（避免截断 UTF-8）
- normalize_prompt_drops_meta_tags / normalize_prompt_returns_none_when_only_tags_remain — 端到端剔除元标签
- strip_removes_tag_and_its_wrapped_content — 标签 + 包裹内容一起删
- strip_handles_multiple_and_interleaved_tags — 多个标签交错
- strip_leaves_unlisted_tags_intact — 名单外的 `<T>` / `<div>` 不动
- strip_requires_boundary_so_prefix_does_not_over_match — `<environment>` 不被 `env` 误命中
- strip_handles_self_closing_and_unclosed_open_tag — 自闭合 / 缺闭合的兜底
- strip_is_safe_on_multibyte_content — `<` 后紧跟多字节字符不 panic
- parse_tag_names_strips_brackets_and_whitespace / parse_tag_names_empty_yields_nothing
- user_prompt_accepts_only_user_prompt_submit — UserPromptExpansion / PreToolUse / Stop 一律不认
- user_prompt_accepts_alternate_field_names
- transcript_path_reads_and_trims / transcript_path_rejects_missing_blank_and_oversized

**module_config.rs（26 个）：**

- default_config_has_agent_status_enabled / default_retention_is_thirty_seconds
- default_max_cells_are_eight / default_running_timeout_is_sixty_seconds
- default_token_unit_is_plain_tokens / default_proxy_resolver_is_none
- token_unit_serializes_as_snake_case / token_unit_roundtrips_through_config
- parse_token_unit_accepts_known_values / parse_token_unit_falls_back_on_unknown_or_missing
- unknown_token_unit_does_not_discard_the_rest_of_the_config — 非法单位不得牵连其它设置
- migrates_legacy_config_without_token_unit / migrates_legacy_config_without_proxy_resolver — 旧配置缺字段回落
- proxy_resolver_roundtrips_and_empty_falls_back — 空串回落 none
- validate_clamps_retention_to_range — 0 → 1000, u64::MAX → 86400000
- validate_clamps_running_timeout_to_range — 0 → 60000, u64::MAX → 3600000
- validate_clamps_max_cells_by_layout — 各形态 0/3 → 4，255 → 8
- migrates_legacy_max_visible_sessions / prefers_max_cells_by_layout_over_legacy_field
- validate_rejects_wrong_schema_version / validate_adds_agent_status_if_missing
- validate_rejects_module_id_with_slash / backslash / overlong（65 字符）
- config_serialization_roundtrip / module_state_loads_default_on_missing_file

**proxy（12 个）：**

`proxy::tests`（4 个）：

- normalize_host_port_handles_common_forms — localhost/默认端口/IPv6/userinfo 归一化
- normalize_host_port_rejects_host_with_nonnumeric_port
- matches_listen_address_compares_normalized — 端口匹配/不匹配
- options_list_none_first_then_resolvers — `none` 恒在首位，cc-switch 已注册

`proxy::ccswitch::tests`（9 个）：

- base_url_from_settings_reads_only_env_base_url — 只取 base_url、不碰 token
- base_url_from_settings_rejects_missing_or_blank_or_bad_json
- resolve_maps_local_proxy_to_current_upstream — 临时 SQLite 造样例库（INTEGER 端口 + 两开关都为 1），端到端解析出上游
- resolve_returns_none_when_listen_port_mismatches — 会话连的端口不是代理监听端口
- resolve_returns_none_when_proxy_disabled_for_app — 本 app 未接管（enabled=0）不解析
- resolve_returns_none_when_global_server_off_even_if_app_enabled — 全局服务器关（proxy_enabled=0）但 app 残留 enabled=1，仍不解析
- resolve_is_app_scoped_codex_proxy_disabled — 按 app 分流
- cache_holds_stale_value_within_ttl — 改库后 TTL 内仍返回旧值（缓存的是代理状态，非单地址结果）
- new_without_db_disables_resolution — 无 cc-switch.db 时 root=None、恒不解析

### 4.2 TypeScript 测试（76 个）

按文件：`agent-status` 35、`snap` 14、`tooltip` 11、`floating-modules` 8、`agent-status-layout` 7、`example` 1。

**agent-status.spec.ts（35 个）：**

- colorForSession 5 个 phase 各自映射正确 / 不返回聚合颜色
- displayEndpoint 上游优先 / 无上游回落 base_url / 两者都缺返回 null
- isStatusSnapshot 接受有效快照；拒绝 null / non-object / wrong schema / negative revision / non-integer revision / missing generated_at_ms / non-array sessions / 1001 sessions / missing key / invalid provider / missing termination_confirmed；接受 codex provider / 空 sessions；拒绝 missing counts
- tokenTotal 四项求和含缓存读 / null 与 undefined 归 0 / 全零会话
- formatTokenCount 千分位分组 / 小数截断与负数归零 / NaN 与 Infinity 兜底
- formatTokenMillions 两位小数 / 去尾零 / 不足 0.01M 显示 `<0.01M` / 非有限值

**agent-status-layout.spec.ts（7 个）：**

- 4 种 layoutFromDocked 映射
- 颜色映射与 phase 对应
- 格子数 / 网格 / 溢出切分（方形按 side² 占满、边条按上限）

**tooltip.spec.ts（11 个）：**

- placeTooltip 各贴边方向的首选侧、越界换侧、全部放不下时夹进工作区

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
| 阶段二：模块基础设施 | ✅ | FloatingModuleHost 单槽、编译期 registry |
| 阶段三：Rust 状态核心 | ✅ | model/reducer/hook/tokens + 测试 |
| 阶段四：Hook bridge | ✅ | main.rs --status-hook、原子写入、quarantine |
| 阶段五：Tauri 集成 | ✅ | managed state、monitor、commands、permissions |
| 阶段六：前端状态与主窗口 | ✅ | stores、App.vue 初始化、MainWindow 配置（含 token 单位） |
| 阶段七：浮窗 UI | ✅ | 4 种布局、状态灯、悬停提示窗、闪烁动画 |
| 阶段八：fallback 和发布验证 | ⬜ | 需手工打包验证 |

## 6. 已知限制

1. **进程 liveness 检查依赖 tasklist/kill** — 在某些受限环境下可能失败，此时返回 None，会话保留到超时。
2. **inbox 扫描为轮询（500ms）** — 首版实现，延迟在可接受范围；如需更低延迟可改为命名管道或 socket。
3. **Codex 尚未做 capability probe** — 当前 Codex adapter 使用与 Claude 相同的 hook 格式，需在实际 Codex CLI 上验证。**token 统计也只覆盖 Claude 的 transcript 格式**（`type: assistant` + `message.usage`）。
4. **配置文件原子写入** — Windows 上先删除再 rename，非真正的原子替换；在极端并发下可能短暂不可读。
5. **悬浮窗会话灯上限** — 各形态上限可配（4..8）。放不下的部分在最后一格显示 `+N`，完整列表仍只在主窗口可见。
6. **Waiting 超时为 30 分钟** — 正常情况下 Waiting 在下次运行时清理；若 hook 未触发（如 Claude Code 异常退出），需等待30 分钟超时。
7. **token 统计依赖 transcript 格式** — 格式变化时静默跳过（宁可统计不到，也不能让 monitor 线程出错），因此数字可能偏低而不会报错。
8. **子代理统计存在盲区** — 子代理目录只在首次 refresh 时扫描一次，**该会话首次统计之后新建的子代理文件不会被发现**。
9. **prompt / base_url 明文落盘** — 见 `CLAUDE.md` 的已知代价说明。
