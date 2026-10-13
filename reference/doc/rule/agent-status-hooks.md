# UFO Agent 状态 Hook 接入指南

> **本篇是「已接入平台」的实录**：Claude Code 与 Qoder 的 hook 配置及 payload 字段、Codex 的 rollout 读法、指示灯语义与子代理红线，以及 UFO 内部怎么解析它们。
> **要接入一个新平台，从 `../guide/agent-status-platform-onboarding-guide.md` 开始** —— 那里有跨平台通用的 payload 契约、最小事件集、兼容层的文件边界，以及 UFO 侧的完整改动清单。本篇的字段表只描述已接入的这几家，别名表各平台自己写，不要照抄成通用约定。
> **与 AGENTS.md 的关系**：`AGENTS.md` 已收敛成不超过 100 行的稳定规则 + 指向本目录的链接，平台与灯态的细则**只在本文**。文档树是 `reference/doc/guide/`（指南）与 `reference/doc/rule/`（规范，本篇所在）；观测侧的账见 `../guide/agent-status-log-guide.md`，把灯推到手机见 `../guide/agent-status-push-guide.md`。**冲突一律以代码为准**（行号与数字会漂，符号名不会）。

UFO 从两条链路获取 agent 的生命周期事件：

- **Claude Code / Qoder**：本地 CLI hook（本文档同名两节）。hook 进程必须快速返回，只写入一个小事件文件，不能等待 GUI。
- **Codex**：**读 rollout**，不用 hook（见文末「Codex」节）。

## 安装前提：`ufo` 这个命令名从哪来

下面所有示例写的都是裸命令 `ufo --status-hook …`，它能被 spawn 靠的是 PATH 里能找到 `UFO.exe`。这条 PATH 记录**由安装器写、由安装器删**（MSI 走 `src-tauri/windows/wix/path-fragment.wxs` 的 WiX `Environment` 组件，NSIS 走 `src-tauri/windows/nsis/path-hooks.nsh` 的 installer hooks），App 运行期完全不碰环境变量与注册表里的环境变量。因此：

- **只有装过 UFO（MSI 或 setup.exe）之后，裸 `ufo` 才存在**；从源码跑没有这一步。
- ⚠️ **装完必须重启 agent 宿主进程**，hook 才会开始生效。原因：hook 是 agent 进程 spawn 的子进程，子进程的环境块在 `CreateProcess` 那一刻从父进程**复制**一份、此后不再刷新；安装器改的是注册表里的环境变量，改不到一个已经在跑的 node 进程内存里。**只新开终端不管用**，要看的是那个 agent 进程本身有没有重启。
- **开发期没有 `ufo`**：`pnpm tauri build` 会按 `tauri.conf.json` 根级 `mainBinaryName` 把 cargo 产物改名成 `UFO.exe`，但 `pnpm tauri dev` / `cargo build` 不改名，产物仍是 `src-tauri/target/debug|release/tauri-app.exe`。本地验 hook 只能写绝对路径，见 `../guide/agent-status-platform-onboarding-guide.md`。
- 两条通道都装过会得到**两条** PATH 记录（MSI 装在 Program Files，NSIS 装在 `$LOCALAPPDATA\UFO`），`ufo` 命中哪一条由 PATH 顺序决定；卸载只摘自己那一条，不会互删。
- 用户 PATH 只写到**装它的那个账户**名下：MSI 是机器级安装，但 PATH 落 `HKCU\Environment`，同机其他账户仍然没有 `ufo`。

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

因此内核为每个会话维护一份**活跃子代理集合**（`kernel.rs` 的 `subagents`：`SubagentStart` 加入、`SubagentStop` 移除，工具类子代理事件同时刷新 `last_seen_at_ms`）。全部裁决都走在 `kernel.rs::apply_event_with_seed` → `reduce_state` 这一条路上，**改它时下面七条要一起保住**（下面是内核内部状态机的规则，对外仍投影为五阶段）：

1. **主线程已 `Stop`（等 prompt）后子代理开始** → 灯拉回「后台运行」（`BackgroundRunning`，对外仍是绿常亮）；最后一个活跃子代理结束时立即落黄，**不再绿闪**。
2. ⚠️ **主线程正在等用户操作**（`PermissionRequest` / `Elicitation` → 黄灯）时，子代理事件**不得**把黄灯顶回绿 —— 人在等你，灯就该是黄的。曾经的表现就是「Claude 已经在问我了，灯却一直是绿色」。
3. **会话已结束（`Completed`）或已出错（`Failed`）时，子代理事件只维护后台记账**（把该 `agent_id` 加入 / 移出活跃集合、刷新 `last_seen_at_ms`），**灯色与计时原样保留** —— `is_flashing` / `remove_at_ms` / `error_code` / `waiting_reason` 一个都不动。
4. **带 `agent_id` 的 `Failed`（子代理自己的工具失败）不代表父会话出错**：既不得把父灯染红，**也不得清空其它仍在跑的子代理**（`PostToolUseFailure` 带 `error` 且带 `agent_id` 是常见输入组合，不是罕见路径）。
5. **主线程出错时活跃子代理集合不清空**：后台事实要留到恢复之后，否则最后一个子代理收尾时灯无处可退、会误落黄。
6. **UFO 中途启动、只先收到子代理活动时，灯的起点是「后台运行」而不是「主线程在跑」**（`Unknown` / `WaitingForPrompt` + `SubagentStarted` → `BackgroundRunning`，而不是 `Running`）—— 起点若是 `Running`，最后一个子代理结束后就没有可退的地方，灯会一直绿。同一轮里也**不触发**「同 provider 的新 `Running` 清掉旧 `Waiting`」，以免后台子代理误清掉别的会话的黄灯。
7. **子代理的 `sequence` 有自己独立的水位**：`sequence_watermarks` 按「会话 → 执行者」两层记账（主线程占一个固定的 `MAIN_ACTOR` 键），子代理序号**不与主线程的互相比较**，也**不写进对外快照的 `last_sequence`**（对外只暴露主线程那条）。

第二条是实测抓到的真实时序（子代理工具事件曾把黄灯污染成绿灯）：

| 顺序 | 事件 | tool_name | agent_id | 期望 |
|---|---|---|---|---|
| 1 | `PermissionRequest` | `AskUserQuestion` | — | 🟡 |
| 2 | `Notification`（`permission_prompt`） | — | — | 🟡 |
| 3 | `PreToolUse` | Grep | **子代理** | 不得改灯 ← 曾经的污染源 |
| 4 | `PostToolUse` | 用户作答后 `AskUserQuestion` | — | 🟢 |

⚠️ `AskUserQuestion`（选择框提问）**不走 `PreToolUse`**，走的是 `PermissionRequest` + `Notification(permission_prompt)`。因此不需要按工具名特判，靠既有的 Waiting 语义即可。

上面第 6 条是这条链路上最容易写错的一处：**把「只见到子代理」当成「主线程在跑」**，灯就没有回落的地方（外部表现为会话永不落黄）。建档时对外仍显示绿灯，所以「灯出现了」不等于「起点是对的」。

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
- **先过通用标签引擎再裁剪**：注入内容按规则表处理，`drop` = 删标签连同内文（本平台的 `system-reminder` / `ide_opened_file` 等），`extract` = 只留标签内正文（别家平台的形态，见下方 Qoder 节）。规则表**一平台一份**：本平台读自己的键 `UFO_TAG_RULES_CLAUDE`（默认清一色 `drop`），没配就用 `claude.rs` 里的 `DEFAULT_TAG_RULES`；Qoder 那侧是 `UFO_TAG_RULES_QODER`，两个文件互不引用。
  - 语法是逗号分隔的 `name` / `name:drop` / `name:extract`（只写 `name` 等同 `drop`，会剥掉误写的尖括号；**动作名写错的项整项丢弃**，不影响其它项）。值为空或整表都非法 → 回落该文件自己的 `DEFAULT_TAG_RULES`。
  - ⚠️ 两个键由 `src-tauri/build.rs` **构建期**注入（`option_env!` → `cargo:rustc-env`）：hook 进程的 cwd 是被监测 agent 的项目目录，运行时读不到仓库的 `.env`，所以**改完 `.env` 必须重新构建**才生效。
  - 扫描与边界（成对 / 自闭合 / 未闭合 / 多字节安全 / 同名前缀不误命中）是不认识平台的公共机制，在 `compat/mod.rs`（`apply_tag_rules` + `build_tag_rules`）；**「删哪些标签、用哪种动作」才是各平台的策略**。新平台该选哪个方向见 `../guide/agent-status-platform-onboarding-guide.md`。
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
| 🟢 常亮 | `Running` | Agent 正在执行（含后台子代理运行中） | `SessionStart`, `Setup`, `UserPromptSubmit`, `UserPromptExpansion`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PostToolBatch`, `TaskCreated`, `TaskCompleted`, `TeammateIdle`, `PermissionDenied`, `MessageDisplay`, `PreCompact`, `PostCompact`, `ElicitationResult`, `InstructionsLoaded`, `ConfigChange`, `CwdChanged`, `DirectoryAdded`, `FileChanged`, `WorktreeCreate`, `WorktreeRemove`，以及非等待类 `Notification` |
| 🟢 + 闪烁 → 🟡 | `Waiting` + `is_flashing` | 一轮执行完成（对话未结束） | `Stop`：先绿闪，`retention_ms` 后落成黄常亮 |
| 🟡 黄灯 | `Waiting` | 等待用户操作（对话未结束） | `PermissionRequest`；`Elicitation`；`Notification` 且 `notification_type` ∈ `idle_prompt` / `permission_prompt` / `elicitation_dialog` / `agent_needs_input` / `elicitation_url_dialog` / `worker_permission_prompt`；以及 Stop 绿闪结束后的稳态 |
| 🟢 + 闪烁 → 移除 | `Completed` | 对话已完成（非错误结束） | 任意 `SessionEnd`（不按 reason 分流）；绿闪 `retention_ms` 后移除 |
| 🔴 红灯 | `Error` | 错误 | `StopFailure`；任何带 `error_code` 的事件；outcome 为 `error`/`failed`/`cancelled`/`denied` 的事件 |

⚠️ **子代理事件走独立的后台运行语义**（见上「子代理事件与 `agent_id`」）：`SubagentStart` / `SubagentStop` 维护活跃子代理集合，`Stop` 之后仍有子代理运行则保持绿灯、最后一个结束时落黄；但「等用户操作」优先级更高，该情形下的子代理事件不改灯色。

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
| `Running` | 进程死亡 → 立即移除；无新事件超时由主窗口「无响应超时」配置（旧名「执行中超时」，配置字段仍是 `running_timeout_ms`；默认 **60 秒**，范围 60–3600） |
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
- ⚠️ 适配器 `CodexPermissionAdapter` **刻意只认这一个事件**，其余一律 `Ok(None)` 静默忽略：多认一个就多一处与 rollout 抢同一盏灯的地方。**别「顺手」把它补回来** —— 两条源同时驱动同一会话会让灯态互顶。这条由两条命名测试锁着：`status/compat/mod.rs` 的 `codex_adapter_only_accepts_permission_request`（适配器只收这一个）与 `status/hook.rs` 的 `codex_hook_writes_only_permission_request`（写进 `~/.codex/hooks.json` 的也只有这一个）。
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
3. **增量**：之后只读新增字节。**文件在长 = 有活动**（绿）；只有上表的生命周期记录才改灯态；同一灯态只产出一次事件。长任务期间（Codex 一条记录都不写）每 30 秒补一条保活，否则会被「无响应超时」（旧名「执行中超时」，`RUNNING_KEEPALIVE`）清掉。⚠️ 去抖基线要跟着**实际灯态**走，见下面「批准后必须立刻回绿」那节。
4. **结束**：文件被移走（归档，`archived_sessions/`）且连续两轮读不到（`MISSING_POLLS_BEFORE_END`）→ 会话结束（绿闪后移除）。
5. **性能**：97% 的字节是命令输出/消息正文（单条可达 100 KB），`compat::codex::cares()` 先做一次子串预筛，只对命中小标记的行做完整 JSON 解析；增量游标保证与文件总大小无关。

### 批准后必须立刻回绿：去抖基线要回灌（`note_light`）

⚠️ **去抖基线必须跟着实际灯态走**，而 hook 把灯抬成黄的那一刻巡检器并不知情。所以 monitor 对**来源是 hook** 的 Codex 事件调用 `codex_rollout.rs::note_light()` 把新灯态回灌成基线 —— `apply_status_event` 的 `source: EventSource`（`Hook` / `Rollout`）参数就是为这件事存在的。

不回灌的后果很具体：审批期间 rollout 一条记录都不写（实测），批准后命令一跑、文件重新在长，巡检器算出的「绿」与它心里那份旧基线**相同** → 被当成重复事件整个吃掉，灯卡在黄直到 30 秒保活才回来。转灯日志里这一对是 `BaselineReflow`（`baseline_reflow`）顶住了 `same_as_last_round_baseline`。

### 每轮的处理顺序：rollout 先、inbox 后

⚠️ **别调回去**。hook 事件代表**更晚发生**的事实（弹审批框之前 rollout 已经写过 `function_call` 记录），所以它必须放后手，才能在同一轮里压过由「文件在长」推出的绿；反过来（inbox 先、rollout 后）会让刚抬起来的黄灯被同一批旧字节顶回绿。通用口径见 `../guide/agent-status-platform-onboarding-guide.md` §9 的接线纪律。

### endpoint 与 token 怎么取

- **Endpoint**：`$CODEX_HOME/config.toml`（默认 `~/.codex/config.toml`）→ `<cwd>/.codex/config.toml` 覆盖；取顶层 `model_provider` 指向的 `[model_providers.<name>].base_url`；环境变量 `OPENAI_BASE_URL` / `OPENAI_API_BASE` 兜底。**只读这一个 key**，`~/.codex/auth.json` 一律不碰。解析用 `basic-toml`（只读；刻意不引官方 `toml`，免得把 `toml_edit` 一起拖进来）。同一套规则被「代理解析器」复用（`status::base_url_from_config_text`），保证 tooltip 显示的 endpoint 与反查出的上游**同源**——采集链路与解析器口径的整体约定见 `agent-status-session-data.md`。
- ⚠️ **token 口径与 Anthropic 相反**：`event_msg`/`token_count` 里的 `info.total_token_usage` 是**会话级累计值**（不是增量）—— 同一文件内逐字段取 max，同一会话的多份 rollout 文件各自相加。它的 `input_tokens` 是 prompt 总量、**已经包含** `cached_input_tokens` 与 `cache_write_input_tokens`（实测 `total_tokens == input + output`），映射到 UFO 的模型时**必须把这两项减出来**，否则前端 `tokenTotal()` 会把缓存算两遍（实测缓存占比可达 1/3）。`output_tokens` 已含 `reasoning_output_tokens`，不重复加。
- ⚠️ **Codex 的用量不在 `status/tokens.rs`**：那个模块只管 Claude 的 transcript（按 `message.id` 去重累加）。

### 取不到的东西

- **「等你授权」**：批准/拒绝在 rollout 里**没有**结构化记录（记录类型、item 字段、output 正文都查过）。唯一线索是「`function_call` 很久没有 output」，而那与「命令跑得久」无法区分 —— 所以 Codex 上这个状态退化为绿灯。
- **子代理**：子线程有自己的 rollout（带 `parent_thread_id` + `source.subagent`），天然不会污染父会话的灯 —— 不需要 Claude 那条 `agent_id` 守卫。

## Qoder

CLI 与 IDE 共用同一套 hook（**同一份配置同时点亮两端**），payload 与 Claude Code 同构，所以走完整 hook 链路、不需要第二个事件源。命令格式：

```text
ufo --status-hook --provider qoder
```

配置写在 `~/.qoder/settings.json` 的 `hooks` 块（项目级 `<项目>/.qoder/settings.json`、`<项目>/.qoder/settings.local.json` 同样有效）。建议每个事件带 `async: true` 与短 `timeout`：

```json
{
  "hooks": {
    "SessionStart":      [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider qoder", "async": true, "timeout": 3 }] }],
    "UserPromptSubmit":  [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider qoder", "async": true, "timeout": 3 }] }],
    "PreToolUse":        [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider qoder", "async": true, "timeout": 3 }] }],
    "PostToolUse":       [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider qoder", "async": true, "timeout": 3 }] }],
    "PostToolUseFailure":[{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider qoder", "async": true, "timeout": 3 }] }],
    "PermissionRequest": [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider qoder", "async": true, "timeout": 3 }] }],
    "Notification":      [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider qoder", "async": true, "timeout": 3 }] }],
    "SubagentStart":     [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider qoder", "async": true, "timeout": 3 }] }],
    "SubagentStop":      [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider qoder", "async": true, "timeout": 3 }] }],
    "Stop":              [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider qoder", "async": true, "timeout": 3 }] }],
    "StopFailure":       [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider qoder", "async": true, "timeout": 3 }] }],
    "SessionEnd":        [{ "hooks": [{ "type": "command", "command": "ufo --status-hook --provider qoder", "async": true, "timeout": 3 }] }]
  }
}
```

### Qoder Hook 事件一览（实测）

下表的事件都在真实会话里抓到过 payload（`~/.qoder/logs/runs/*/qodercli.log` 的 `hook_input=` 行），除 `PermissionRequest` / `StopFailure` —— 它们可配但本次未触发，语义按事件名推。

| 事件 | 触发时机 | 灯 |
|---|---|---|
| `SessionStart` | 会话开始（`source: "startup"`） | 🟢 Running |
| `UserPromptSubmit` | 用户提交提问（带 `prompt`） | 🟢 Running |
| `PreToolUse` / `PostToolUse` | 工具调用前后 | 🟢 Running |
| `PostToolUseFailure` | **工具**调用失败（带 `error` / `error_type`） | 🟢 Running（见下「三处差异」①） |
| `PermissionRequest` | 工具调用待授权（等人操作） | 🟡 Waiting（不闪） |
| `Notification` | 通知（看 `notification_type`） | 等待类 → 🟡，其余 🟢 |
| `SubagentStart` / `SubagentStop` | 子代理起止 | 记账，按主线程态分流 |
| `Stop` | 一轮回复跑完、等下一句 | 🟢 闪 → 🟡 |
| `StopFailure` | 本轮 API/模型出错 | 🔴 Error |
| `SessionEnd` | 会话终止（字段 `reason`，实测 `"other"`） | 🟢 闪后移除 |

### Payload 字段

与 Claude Code **同名**，所以 UFO 侧的取法完全一致：`session_id`、`hook_event_name`、`transcript_path`、`cwd`、`prompt`（仅 `UserPromptSubmit`）、`agent_id`、`notification_type`、`reason`。Qoder 另带的字段 UFO 不采：`model`、`permission_mode`、`tool_use_id`、`tool_input`、`tool_response`、`stop_hook_active`、`last_assistant_message`、`request_set_id`、`details`、`agent_transcript_path`。

- **子代理**：与 Claude 同 —— 子代理事件与主会话**共用 `session_id`**，只有 `agent_id`（实测形如 `aExplore-d2ed3f773d36e2d1`、`aPlan-a495c2600931ca7f`）分得开，主线程事件不带。内核靠它维护活跃集合。
- **`prompt` 绝大多数情况就是用户原文，但 `/goal` 那一轮不是**：实测本机 18 条真实 `UserPromptSubmit` payload，17 条与用户输入逐字符相同（Qoder 的 reminder 走别的通道，不混进这个字段）；剩下那条是**设置目标**的轮次 —— `prompt` 被整个换成 3282 字的 harness 模板（`A goal has been set.` 开头、结尾还带 `Goal state: turns used 0/20`），用户真正的目标写在 `<untrusted_objective>…</untrusted_objective>` 里。
  UFO 侧因此在裁剪前先过通用标签引擎（见上文「prompt 的来源」），本平台取 `untrusted_objective:extract`：**只上报标签内的正文**，模板一个字都不外泄（推送与提示窗都受影响）。该规则带「文本以 `A goal has been set.` 开头」的命中门槛 —— 普通提问没有那个标签，没门槛就会被判成「无正文」，提问从此不再更新；门槛命中却仍找不到标签（模板改版 / 截断到只剩脚手架）时也判「无正文」，由内核沿用上一条用户提问，而不是把平台模板当提问推给用户。
  ⚠️ 两个平台的适配器仍互不引用：共用的是**不认识平台的扫描机制**（`compat/mod.rs`），「删哪些标签、用什么动作」这份策略留在各自文件里。
- **`Notification.message`**：等人时那句理由（实测如「Tool AskUserQuestion requires confirmation」），UFO 走 `waiting_reason` 显示在信息卡 head 行，**不**塞进 `prompt`。
- **`SubagentStop.agent_transcript_path`**：子代理自己的 transcript（`<项目>/<session-id>/subagents/agent-<id>.jsonl`）。UFO 不看这个字段 —— transcript 按 `<session-id>/subagents/` 目录扫，与 Claude 同一套布局。

### 三处与 Claude 的差异

1. **工具失败不是会话失败**。`PostToolUseFailure` 的 `error` 是普通工具报错正文（实测如「Path does not exist: …」「Exit code 2 …」）。通用规则「有 `error_code` 即判失败」会让用户每敲错一次路径就看见一次红灯，而 agent 只是重试一次 —— 所以 UFO 的 Qoder 适配器对**这一个事件**不读错误字段，灯保持绿；真正的本轮出错仍由 `StopFailure` 转红。
2. **拿不到真实 endpoint**。`~/.qoder/settings.json` 没有 `env` 块，模型目录（`~/.qoder/.models/*/catalog-v6`）是加密的，BYOK 走 `/model` 向导且不落明文配置；环境变量里也没有 endpoint 类变量。实测模型流量发往 `api3.qoder.sh/algo/api/v2/service/pro/sse/agent_chat_generation`，但那个 host 由服务端按区域下发（`center.qoder.sh/api/*/service/region/endpoints`），换区域 / 换版本就会变，不适合当档案键。因此 UFO 给 Qoder 会话送一个**稳定身份常量** `compat::qoder::QODER_ENDPOINT`：它不可请求、不代表任何用户配置，只为让「信息卡拓展」的 endpoint 档案有键可用。⚠️ 代价是 tooltip 的 Endpoint 行显示的就是这个常量本身。
3. **transcript 的 token 数值恒为 0**。记录结构与 Claude 同构（`type` = assistant、`message.usage` 的 `input_tokens` / `cache_creation_input_tokens` / `cache_read_input_tokens` / `output_tokens` 四个键名都在，实测 326 个 usage 块），但网关不回填数值（只有 `credits` / `original_credits` / `context_usage_ratio` 非 0）。所以 Qoder 与 Claude 共用同一个 transcript 解析器（`status/tokens.rs` 的 `TokenTracker`）：解析出 0，前端 `tokenCount > 0` 自然不渲染那一行；哪天网关开始回填，这里就自动出数。

## 状态目录

GUI 负责选择 app-local 状态目录。如需在不打包的情况下测试 hook，可设置 `UFO_STATUS_DIR` 指向一个受控目录。每个事件通过临时文件加原子 rename 写入 `inbox/*.json`；格式错误的事件会被移入隔离目录。

监测器按 `provider:session_id` 维护唯一的会话记录，在可选的悬浮窗模块中最多渲染 8 盏独立状态灯。
