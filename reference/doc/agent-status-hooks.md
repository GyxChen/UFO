# UFO Agent 状态 Hook 接入指南

UFO 通过本地 CLI hook 接收归一化的生命周期事件。hook 进程必须快速返回，只写入一个小事件文件，不能等待 GUI。

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

因此 reducer 对**已存活会话**收到的子代理事件只做一件事：刷新 `last_seen_at_ms`，**不参与灯色迁移**。

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
- **只认 `UserPromptSubmit`**：同轮内后续事件（`PreToolUse` 等）本就不带 `prompt`，由 reducer 继承该值；下一次 `UserPromptSubmit` 才覆盖。即提示窗始终显示**最近一次用户手输**的提问。
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

Codex（CLI / 桌面版）的 hook 与 Claude Code 同构：命令配一次、由 Codex 在生命周期事件发生时自动调用，**不需要手动运行**。事件清单与字段已对本机 `codex.exe` v0.155.0-alpha.2.6 实测确认。

命令格式与 Claude 完全一致，只是 provider 换成 `codex`：

```text
ufo --status-hook --provider codex
```

### 配置 hook

Codex 从 `$CODEX_HOME/hooks.json`（默认 `~/.codex/hooks.json`）、`~/.codex/config.toml` 的 `[hooks]` 表、以及 `<repo>/.codex/` 下的同名文件里发现 hooks（多层**叠加**，不互相覆盖）。把下面内容写进 `~/.codex/hooks.json`，**再去信任它** —— 未信任的 hook 不会执行：

- **Codex Desktop**：`设置 → Hooks`。该页按来源分组列出已发现的 hook（我们这份会标 `From Config`），未信任的条目显示 `Disabled until hook is trusted` / `New hook`，点条目上的 **Trust**，或顶部 **Trust all** 一次全信任。页面顶部还有一行汇总计数（**`N needs review`**），看到它没归零就说明还有没信任的 hook。页面里另有 **Open config file** 可直接打开 `hooks.json`。
- **Codex CLI（TUI）**：用 `/hooks` 查看并信任。

⚠️ 信任是**按 hook 内容哈希**记账的：改过 `hooks.json` 之后会显示 `Hook changed since last trusted`，**需要重新 Trust**（改一次要再点一次）。

```json
{
  "hooks": {
    "SessionStart":     [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider codex", "async": true, "timeout": 3 }] }],
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider codex", "async": true, "timeout": 3 }] }],
    "PreToolUse":       [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider codex", "async": true, "timeout": 3 }] }],
    "PostToolUse":      [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider codex", "async": true, "timeout": 3 }] }],
    "PermissionRequest":[{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider codex", "async": true, "timeout": 3 }] }],
    "PreCompact":       [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider codex", "async": true, "timeout": 3 }] }],
    "PostCompact":      [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider codex", "async": true, "timeout": 3 }] }],
    "SubagentStart":    [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider codex", "async": true, "timeout": 3 }] }],
    "SubagentStop":     [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider codex", "async": true, "timeout": 3 }] }],
    "Stop":             [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider codex", "async": true, "timeout": 3 }] }],
    "Interrupt":        [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider codex", "async": true, "timeout": 3 }] }],
    "SessionEnd":       [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider codex", "async": true, "timeout": 3 }] }]
  }
}
```

若 `ufo` 不在 PATH，`command` 需写绝对路径；Windows 的 JSON 里反斜杠要双写：

```json
"command": "\"D:\\path\\to\\ufo.exe\" --status-hook --provider codex"
```

**信任机制的绕过（仅在确有需要时用）**：Codex 提供 `BYPASS_HOOK_TRUST` 环境变量与 `--dangerously-bypass-hook-trust` 命令行开关，含义是「本次调用不要求 hook 已被信任」。官方措辞是 DANGEROUS、只给「已经自己审查过 hook 来源」的自动化用 —— 常规做法仍然是在设置页点一次 Trust，**不建议**为此长期设环境变量。

### 事件与灯色

共同 stdin 字段：`session_id`、`transcript_path`（可为 null）、`cwd`、`hook_event_name`、`model`（+ 轮内的 `turn_id` / `permission_mode`）。

| 事件 | 灯态 | 说明 |
|---|---|---|
| `SessionStart` | 🟢 Running | 会话开始 |
| `UserPromptSubmit` | 🟢 Running | 提交提问（payload 的 `prompt` 进 tooltip） |
| `PreToolUse` / `PostToolUse` | 🟢 Running | 工具调用前后 |
| `PreCompact` / `PostCompact` | 🟢 Running | 上下文压缩前后 |
| `SubagentStart` / `SubagentStop` | 🟢 Running | 子代理起止 |
| `PermissionRequest` | 🟡 Waiting | 工具调用要你批准 |
| `Interrupt` | 🟡 Waiting | 你按 Esc / Ctrl-C 取消本轮；**刻意不落红灯**（红灯专指接口/不可抗力错误） |
| `Stop` | 🟢 闪烁 → 🟡 | 一轮正常跑完 |
| `SessionEnd` | 🟢 闪烁 → 移除 | 会话结束（归档 / 关闭 / 空闲 30 分钟） |

⚠️ **子代理事件带 `agent_id`（+`agent_type`）、主线程不带，且与主会话共用 `session_id`** —— 与 Claude 同理，内核会把已存活会话的子代理事件拦成「只刷存活时间、不驱灯色」，避免黄灯被顶回绿。

### 错误态（红灯）来自 rollout，不是 hook

⚠️ Codex 的 12 个 hook 事件里**没有错误类事件**（Claude 有 `StopFailure`），而且实测：**「重试耗尽」这类以 error 结束的轮次连 `Stop` 都不触发**（公开 issue [#22858](https://github.com/openai/codex/issues/22858) 描述的历史行为，v0.150 补的 `Interrupt` 只覆盖用户 Esc 主动取消）。也就是说，只靠 hook 的话 Codex 会话的红灯**永远不可达** —— 灯会停在绿灯，直到「执行中超时」把它清掉，看起来就是「绿着绿着消失了」。

UFO 因此对 Codex 额外巡检它自己写的 rollout：读到 `event_msg` / `task_complete` **带非空 `error`** 就补一条红灯事件（`error_code` 里带错误正文，如 `exceeded retry limit, last status: 429`）。

- 只巡检**已登记会话**（hook 的 `transcript_path` 带过的那些），agent 全停即零 I/O。
- 只认 `task_complete` 带 error：中间的重试不写 `task_complete`，所以**「重试后成功」不会被误报成错误**。
- 按 `turn_id` 去重，同一轮只报一次。
- 已知时序窗口约 500ms（rollout 落盘晚于真实发生），期间若用户已发下一条 prompt，红灯可能短暂盖住新绿灯，下一条 hook 事件会纠正。

### token 用量

取自该会话 rollout JSONL 里的 `event_msg` / `token_count` 事件（`info.total_token_usage`）。那是**会话级累计值**，同一文件内取最大值、同一会话的多个 rollout 各自相加。

⚠️ Codex（OpenAI 口径）与 Claude（Anthropic 口径）的 input 含义相反：Codex 的 `input_tokens` 是 prompt **总量**、已含 `cached_input_tokens` / `cache_write_input_tokens`（实测 `total_tokens == input + output`），而 UFO 的 `TokenTotals` 沿用 Anthropic 口径（input 不含缓存）。因此解析时要把缓存从 input 里减出去，否则总量会重复计算缓存部分。`output_tokens` 已含 `reasoning_output_tokens`，不重复相加。

## 状态目录

GUI 负责选择 app-local 状态目录。如需在不打包的情况下测试 hook，可设置 `UFO_STATUS_DIR` 指向一个受控目录。每个事件通过临时文件加原子 rename 写入 `inbox/*.json`；格式错误的事件会被移入隔离目录。

监测器按 `provider:session_id` 维护唯一的会话记录，在可选的悬浮窗模块中最多渲染 8 盏独立状态灯。
