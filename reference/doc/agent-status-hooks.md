# UFO Agent 状态 Hook 接入指南

UFO 从两条链路获取 agent 的生命周期事件：

- **Claude Code**：本地 CLI hook（本文档第 1 节）。hook 进程必须快速返回，只写入一个小事件文件，不能等待 GUI。
- **Codex**：**读 rollout**，不用 hook（见文末「Codex」节）。

## Claude Code

将 Claude Code 的 hook 命令配置为传入 `--status-hook --provider claude`，并将 hook JSON 通过管道传给 UFO 可执行文件。保留已有 hook，合并此命令而非覆盖用户配置。

命令格式：

```text
ufo --status-hook --provider claude
```

### Hook 配置示例（~/.claude/settings.json）

```json
{
  "hooks": {
    "SessionStart":      [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "UserPromptSubmit":  [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "PreToolUse":        [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "PostToolUse":       [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "PostToolUseFailure":[{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "PostToolBatch":     [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "Notification":      [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "PermissionRequest": [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "PermissionDenied":  [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "SubagentStart":     [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "SubagentStop":      [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "TaskCreated":       [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "TaskCompleted":     [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "Stop":              [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "StopFailure":       [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "TeammateIdle":      [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "Elicitation":       [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "ElicitationResult": [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }],
    "SessionEnd":        [{ "hooks": [{ "command": "ufo --status-hook --provider claude", "type": "command" }] }]
  }
}
```

### Claude Code Hook 事件一览

| 事件 | 触发时机 |
|---|---|
| `SessionStart` | 会话开始或恢复 |
| `Setup` | 使用 `--init-only`、`--init` 或 `--maintenance -p` 启动时 |
| `UserPromptSubmit` | 用户提交 prompt，Claude 处理之前 |
| `UserPromptExpansion` | 用户输入的命令展开为 prompt 之后 |
| `PreToolUse` | 工具调用执行之前（可阻止） |
| `PermissionRequest` | 工具调用需要权限决定时（等人操作） |
| `PermissionDenied` | 自动模式拒绝工具调用时 |
| `PostToolUse` | 工具调用成功之后 |
| `PostToolUseFailure` | 工具调用失败之后 |
| `PostToolBatch` | 一批并行工具调用全部完成后 |
| `Notification` | Claude Code 发送通知时（需看 `notification_type`） |
| `MessageDisplay` | 助手消息文本显示时 |
| `SubagentStart` | 子代理被创建时 |
| `SubagentStop` | 子代理完成时 |
| `TaskCreated` | 任务被创建时 |
| `TaskCompleted` | 任务被标记为完成时 |
| `Stop` | Claude 完成一轮回复，等待用户下一句（**≠ 会话结束**） |
| `StopFailure` | 因 API 错误导致本轮结束（会话通常仍活着） |
| `TeammateIdle` | 团队成员即将空闲时 |
| `InstructionsLoaded` | CLAUDE.md 或 rules 文件加载到上下文时 |
| `ConfigChange` | 会话中配置文件变更时 |
| `CwdChanged` | 工作目录变更时 |
| `DirectoryAdded` | 会话中添加工作目录时 |
| `FileChanged` | 监视的文件在磁盘上变更时 |
| `WorktreeCreate` | 创建 worktree 时 |
| `WorktreeRemove` | 移除 worktree 时 |
| `PreCompact` | 上下文压缩之前 |
| `PostCompact` | 上下文压缩完成后 |
| `Elicitation` | MCP 服务器请求用户输入时（等人操作） |
| `ElicitationResult` | 用户响应 MCP 请求后 |
| `SessionEnd` | 会话生命周期终止时（字段为 `reason`，**没有** success/error outcome） |

### Payload 字段

hook 通过 stdin 接收 JSON，UFO 从以下字段中提取信息：

| 字段 | 用途 | 备选字段名 |
|---|---|---|
| `session_id` | 会话标识 | `sessionId`, `conversation_id` |
| `hook_event_name` | 事件名称 | `event`, `event_name`, `event_type`, `type` |
| `pid` | 进程 ID | `process_id` |
| `event_id` | 事件去重 ID | `eventId`, `id` |
| `sequence` | 事件序号 | `seq` |
| `error_code` | 错误码 | `error`, `failure_reason` |
| `outcome` | 结果 / SessionEnd.`reason` | `reason`, `status`, `stop_reason` |
| `notification_type` | Notification 类型 | `notificationType` |
| `tool_name` | 工具名 | — |
| `tool_input` | 工具输入 | — |
| `agent_id` | **子代理**事件的来源标识；主线程事件**不带**此字段 | `agentId` |
| `prompt` | 用户提问原文（**仅** `UserPromptSubmit`） | `user_prompt`, `prompt_text` |

### 子代理事件与 `agent_id`

⚠️ **子代理（subagent）的事件与主会话共用同一个 `session_id`**（实测确认），只有 `agent_id` / `agent_type` 能把两者分开。子代理侧的事件包括 `SubagentStart`、`SubagentStop`、`PreToolUse`、`PostToolUse`、`PostToolBatch` 等 —— 全部带 `agent_id`；主线程事件一律没有。

因此内核对**已存活会话**收到的子代理事件只做一件事：刷新 `last_seen_at_ms`，**不参与灯色迁移**。

原因：主 agent 弹出提问框等你作答时（`PermissionRequest` → 黄灯），若后台还挂着子代理在跑，子代理的工具事件会把 `Waiting` 直接顶回 `Running`，表现为「Claude 已经在问我了，信号灯却一直是绿色」。这是实测抓到的真实时序：

| 顺序 | 事件 | tool_name | agent_id | 期望 |
|---|---|---|---|---|
| 1 | `PermissionRequest` | `AskUserQuestion` | — | 🟡 |
| 2 | `Notification`（`permission_prompt`） | — | — | 🟡 |
| 3 | `PreToolUse` | Grep | **子代理** | 不得改灯 ← 曾经的污染源 |
| 4 | `PostToolUse` | 用户作答后 `AskUserQuestion` | — | 🟢 |

⚠️ `AskUserQuestion`（选择框提问）**不走 `PreToolUse`**，走的是 `PermissionRequest` + `Notification(permission_prompt)`。因此不需要按工具名特判，靠既有的 Waiting 语义即可。

会话尚不存在时（UFO 中途启动）子代理事件仍会正常建档为 `Running`，保证指示灯能出现；但**不会**触发「新运行清除旧 Waiting」的规则，以免后台子代理误清掉其它会话的黄灯。

### base_url 的来源

`base_url` **不来自 hook payload**，而是 hook 按以下顺序解析：**Claude Code 配置文件优先，环境变量兜底**。

**1. 配置文件（仅 `claude`）** —— 依次读取下列 settings，**后者覆盖前者**：

| 顺序 | 路径 | 说明 |
|---|---|---|
| 1 | `~/.claude/settings.json` | 用户级 |
| 2 | `<cwd>/.claude/settings.json` | 项目级；`cwd` 取自 hook payload |
| 3 | `<cwd>/.claude/settings.local.json` | 项目本地 |

从 `env` 块中读取 `ANTHROPIC_BASE_URL`：

```json
{ "env": { "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721" } }
```

**只按名字取这一个 key** —— `env` 块通常同时存放 token，绝不整块读取或上报。文件缺失、JSON 非法、`env` 块缺失都静默跳过，不影响状态上报。

**2. 环境变量兜底** —— 配置文件未命中时，读 hook 自身的环境变量（hook 由 agent 进程派生，继承其环境）：

| provider | 环境变量（按优先级） |
|---|---|
| `claude` | `ANTHROPIC_BASE_URL` |
| `codex` | `OPENAI_BASE_URL`、`OPENAI_API_BASE` |

`codex` 的配置文件是 `$CODEX_HOME/config.toml`（默认 `~/.codex/config.toml`），TOML 格式，与 settings.json 不同，因此单独解析：按顶层 `model_provider` 取 `[model_providers.<name>].base_url`，`<cwd>/.codex/config.toml` 覆盖用户级。只读这一个 key，api key（`~/.codex/auth.json`）不读取。

⚠️ 用 cc-switch 时这个 base_url 是**本地代理地址**（如 `http://127.0.0.1:15721/v1`），你要的真实上游由「代理解析器」反查出来显示 —— 而 cc-switch 里 **codex 的 provider 上游藏在 `settings_config.config` 那段内嵌 TOML 里**（claude 才是 `env.ANTHROPIC_BASE_URL`）。两边用同一套 TOML 解析规则，缺了它 Codex 的 Endpoint 会一直停在 localhost。

只读 endpoint 类变量，不读取 token / api key。**按需求不做脱敏**：原样保留（含 userinfo / query），仅去首尾空白；长度上限 4096 字节。

### prompt 的来源

取自 `UserPromptSubmit` payload 的 **`prompt`** 字段（字段名已对 CC 2.1.269 实测确认）：

```json
{
  "session_id": "703896bf-…",
  "transcript_path": "C:\\Users\\…\\<session>.jsonl",
  "cwd": "C:\\…",
  "prompt_id": "c91785c8-…",
  "permission_mode": "default",
  "hook_event_name": "UserPromptSubmit",
  "prompt": "用户输入的原文"
}
```

- 上限 4000 字符，超出截断并加 `…`（按字符计数，不截断 UTF-8）。
- **只认 `UserPromptSubmit`**：同轮内后续事件（`PreToolUse` 等）本就不带 `prompt`，由内核继承该值；下一次 `UserPromptSubmit` 才覆盖。即提示窗始终显示**最近一次用户手输**的提问。
- ⚠️ `UserPromptExpansion` 紧随 `UserPromptSubmit` 触发、**同样携带 `prompt`**，但那是斜杠命令展开后的模板文本，不是用户输入。UFO 不放行它 —— 否则展开内容会立刻把用户真正的提问覆盖掉。

⚠️ `base_url` 与 `prompt` 都会明文写入 `status/inbox/*.json`（约 500ms 内被消费删除）。提问内容可能含密钥或私有代码。

### SessionEnd.reason

SDK 中 SessionEnd 使用 `reason`（UFO 归一化进 `outcome`），枚举大致为：

- `clear` / `resume` — 会话被清空或切换
- `logout` / `prompt_input_exit` — 用户退出类
- `other` / `bypass_permissions_disabled`

**没有** `success` / `error` / `cancelled` 这类 outcome。UFO 将任意 SessionEnd 视为「非错误结束」→ Completed（绿闪）；红灯只靠 `StopFailure` / `error_code` / 明确失败 outcome。

## 指示灯状态映射

| 指示灯 | 状态 | 含义 | 触发事件 |
|---|---|---|---|
| 🟢 常亮 | `Running` | Agent 正在执行 | `SessionStart`, `Setup`, `UserPromptSubmit`, `UserPromptExpansion`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PostToolBatch`, `SubagentStart`, `SubagentStop`, `TaskCreated`, `TaskCompleted`, `TeammateIdle`, `PermissionDenied`, `MessageDisplay`, `PreCompact`, `PostCompact`, `ElicitationResult`, `InstructionsLoaded`, `ConfigChange`, `CwdChanged`, `DirectoryAdded`, `FileChanged`, `WorktreeCreate`, `WorktreeRemove`，以及非等待类 `Notification` |
| 🟢 + 闪烁 → 🟡 | `Waiting` + `is_flashing` | 一轮执行完成（对话未结束） | `Stop`：先绿闪，`retention_ms` 后落成黄常亮 |
| 🟡 黄灯 | `Waiting` | 等待用户操作（对话未结束） | `PermissionRequest`；`Elicitation`；`Notification` 且 `notification_type` ∈ `idle_prompt` / `permission_prompt` / `elicitation_dialog` / `agent_needs_input` / `elicitation_url_dialog` / `worker_permission_prompt`；以及 Stop 绿闪结束后的稳态 |
| 🟢 + 闪烁 → 移除 | `Completed` | 对话已完成（非错误结束） | 任意 `SessionEnd`（不按 reason 分流）；绿闪 `retention_ms` 后移除 |
| 🔴 红灯 | `Error` | 错误 | `StopFailure`；任何带 `error_code` 的事件；outcome 为 `error`/`failed`/`cancelled`/`denied` 的事件 |

⚠️ **子代理事件不参与灯色迁移**（见上「子代理事件与 `agent_id`」）：带 `agent_id` 的事件对已存活会话只刷新存活时间，不改变阶段 / 闪烁 / 移除计时。

### `notification_type` 完整枚举

Claude Code 2.1.270 共 16 个（从 CLI 二进制常量提取），UFO 只把下面标注的 6 个当作「等人操作」：

| 值 | 是否「等人操作」 |
|---|---|
| `permission_prompt` | ✅ |
| `idle_prompt` | ✅ |
| `elicitation_dialog` | ✅ |
| `elicitation_url_dialog` | ✅ |
| `agent_needs_input` | ✅ |
| `worker_permission_prompt` | ✅ |
| `auth_success` / `agent_completed` / `push_notification` / `computer_use_enter` / `computer_use_exit` / `quota_auto_resume_fired` / `quota_auto_resume_stale` / `quota_auto_resume_disabled` / `elicitation_complete` / `elicitation_response` | ❌（按 Running 处理） |

⚠️ 漏列「等人操作」的值会退化成 `Running`，表现就是「该等用户了、灯还是绿色」。新增枚举值时对照此表补齐。

### Error 转灯（红灯不粘）

红灯不是死状态：只要事件本身能映射出别的阶段，就允许转灯。

- `Stop` → 黄灯（先绿闪再落黄）
- `SessionEnd` → 绿闪后移除
- `PreToolUse` / `PostToolUse` 等执行事件 → 绿灯
- `UserPromptSubmit` / `SessionStart` → 绿灯（用户重新开跑）

唯一例外是 `Unknown`：无法识别的事件不构成转灯信号，红灯保持不变。

## 清理策略

| 阶段 | 清理行为 |
|---|---|
| `Completed` | 按配置的 `retention_ms`（默认 30 秒）绿闪后**移除** |
| `Waiting` + Stop 绿闪 | 按 `retention_ms` 绿闪后**落成黄常亮**（不移除） |
| `Waiting` | **保留到同一 provider 的下次运行开始时清理**（30 分钟 failsafe 超时） |
| `Running` | 进程死亡 → 立即移除；无新事件超时由主窗口「执行中超时」配置（默认 **60 秒**，范围 60–3600） |
| `Error` / `Unknown` | 进程死亡 → 立即移除；60 秒无新事件 → 超时移除 |

关键行为：当同一 provider 的新 `Running` 事件到来时，自动清除该 provider 的所有旧 `Waiting` 会话。

## Codex

⚠️ **Codex 不用 hook，UFO 读它的 rollout。** 这一节解释为什么，以及怎么工作。

### 为什么事件源是 rollout 而不是 hook

Codex 确实有一套 hook（12 个事件，`HOOK_EVENT_NAMES` 实测自 `codex.exe` v0.155.0-alpha.2.6）：
`PreToolUse` / `PermissionRequest` / `PostToolUse` / `PreCompact` / `PostCompact` /
`SessionStart` / `SessionEnd` / `UserPromptSubmit` / `SubagentStart` / `SubagentStop` /
`Stop` / `Interrupt`。但实测**不足以支撑状态灯**：

| 问题 | 实测证据 |
|---|---|
| **没有错误类事件** | Claude 有 `StopFailure`，Codex 没有对等物。429/502 这类以 error 结束的轮次，`task_complete` 已写进 rollout，hook 侧零调用 |
| **中断路径也不触发** | 从 Desktop 停止按钮取消一轮（app-server 的 `turn/interrupt`），rollout 里 `turn_aborted` 已落盘，hook 侧仍是一次都没有 |
| **会静默失效** | hooks 要在设置页逐条信任、且按**内容哈希**记账（改一次要重新信任）。实测出现过「一段时间一次都不触发、四处日志无任何报错」 |

**所以 Codex 侧 `hooks.json` 只配一个事件**（`PermissionRequest`）—— rollout 覆盖不到的只有它：

```json
{ "hooks": { "PermissionRequest": [
  { "hooks": [{ "type": "command", "command": "ufo --status-hook --provider codex", "async": true, "timeout": 3 }] }] } }
```

多配一个事件就多起一份进程；适配器会静默忽略它们，等于白花。Claude 侧仍然走 hook —— 那是唯一途径，且工作正常。

### 「等你授权」只能靠 hook

rollout 里**没有**审批的任何记录（记录类型、`item_completed` 的 item 字段、命令 output 正文都查过），所以这一个状态只能由 hook 补。2026-09-18 用探针实测抓到的真实 payload：

```json
{
  "session_id": "01a0b060-f0b3-…", "turn_id": "01a0b063-35b6-…",
  "cwd": "E:\workspace\AIProjects",
  "hook_event_name": "PermissionRequest",
  "model": "deepseek-flash", "permission_mode": "default",
  "tool_name": "Bash",
  "tool_input": {
    "command": "curl.exe -L … https://developers.openai.com/sitemap.xml",
    "description": "是否允许我访问 OpenAI 官方网站，定位 Codex 沙盒配置说明"
  }
}
```

- 映射：**WaitingForUser**（黄灯不闪 —— 等人授权与「用户中断」同一语义，都不是错误）。
- `tool_input.description` 进 `waiting_reason`，显示在 tooltip 的 head 行（窄了省略号，全文在 title）；没有 description 时退回 `tool_input.command`。
- ⚠️ 适配器 `CodexPermissionAdapter` **刻意只认这一个事件**，其余一律 `Ok(None)` 静默忽略：多认一个就多一处与 rollout 抢同一盏灯的地方。
- ⚠️ hook 仍可能整段静默失效（实测过一次），所以它只是锦上添花：挂了就退化成绿灯，rollout 链路不受影响。

### rollout 在哪、长什么样

`$CODEX_HOME/sessions/<年>/<月>/<日>/rollout-<本地时间>-<会话 id>.jsonl`（归档的会话会被**移走**到 `archived_sessions/`）。
每行一条 JSON 记录，`type` 是外层记录名、`payload.type` 是内层。常用记录：

| 记录 | 含义 | UFO 的灯态 |
|---|---|---|
| `session_meta`（文件首行） | 会话身份：`id` / `cwd` / `originator` / `cli_version` / `model_provider` | 🟢 Running |
| `event_msg`/`task_started` | 一轮开始 | 🟢 Running |
| `event_msg`/`user_message` | 用户消息（带 `message` 正文） | 🟢 Running |
| `event_msg`/`agent_message`、`response_item`/`reasoning`、`function_call(_output)`、`item_completed`… | 执行过程中的活动 | 文件在长即视为 🟢（不逐条解析） |
| `event_msg`/`task_complete` | 一轮结束 | 无 `error` → 🟢 闪 → 🟡；**带 `error` → 🔴** |
| `event_msg`/`turn_aborted` | 用户中断（`reason: "interrupted"`） | 🟡 Waiting（不闪） |
| `event_msg`/`token_count` | 会话级**累计**用量 | 不改灯态，只更新 tooltip 的 token 行 |

### UFO 怎么读

1. **发现**：扫最近两天的日期目录（目录名字典序即时间序，不需要本地日期库）；会话 id 取文件名末尾的 uuid（实测 == `session_meta.id`）。
2. **播种**：注册时读一遍已有内容，只为拿 `cwd`（→ endpoint）与 token 累计，**不产出事件** —— 否则 UFO 一启动就会把当天所有历史会话复读成一串灯色动画。
3. **增量**：之后只读新增字节。**文件在长 = 有活动**（绿）；只有上表的生命周期记录才改灯态；同一灯态只产出一次事件。长任务期间（Codex 一条记录都不写）每 30 秒补一条保活，避免被「执行中超时」清掉。
4. **结束**：文件被移走（归档）且连续两轮读不到 → 会话结束（绿闪后移除）。
5. **性能**：97% 的字节是命令输出/消息正文（单条可达 100 KB），先做一次子串预筛，只对命中小标记的行做完整 JSON 解析；增量游标保证与文件总大小无关。

### 取不到的东西

- **「等你授权」**：批准/拒绝在 rollout 里**没有**结构化记录（记录类型、item 字段、output 正文都查过）。唯一线索是「`function_call` 很久没有 output」，而那与「命令跑得久」无法区分 —— 所以 Codex 上这个状态退化为绿灯。
- **子代理**：子线程有自己的 rollout（带 `parent_thread_id` + `source.subagent`），天然不会污染父会话的灯 —— 不需要 Claude 那条 `agent_id` 守卫。

## 状态目录

GUI 负责选择 app-local 状态目录。如需在不打包的情况下测试 hook，可设置 `UFO_STATUS_DIR` 指向一个受控目录。每个事件通过临时文件加原子 rename 写入 `inbox/*.json`；格式错误的事件会被移入隔离目录。

监测器按 `provider:session_id` 维护唯一的会话记录，在可选的悬浮窗模块中最多渲染 8 盏独立状态灯。
