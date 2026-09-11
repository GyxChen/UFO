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
| 🟡 黄灯 | `Waiting` | 等待用户操作（对话未结束） | `PermissionRequest`；`Elicitation`；`Notification` 且 `notification_type` ∈ `idle_prompt` / `permission_prompt` / `elicitation_dialog` / `agent_needs_input`；以及 Stop 绿闪结束后的稳态 |
| 🟢 + 闪烁 → 移除 | `Completed` | 对话已完成（非错误结束） | 任意 `SessionEnd`（不按 reason 分流）；绿闪 `retention_ms` 后移除 |
| 🔴 红灯 | `Error` | 错误 | `StopFailure`；任何带 `error_code` 的事件；outcome 为 `error`/`failed`/`cancelled`/`denied` 的事件 |

### Error 粘性与恢复

- 进入 Error 后，普通非 Error 事件（如 `PreToolUse`）不会冲掉红灯。
- `UserPromptSubmit` / `SessionStart` 视为用户重新开跑，允许 `Error → Running`。

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

如果已安装的 Codex CLI 提供了兼容的 hook 或通知回调，使用相同的命令格式并传入 `--provider codex`。启用前需验证 provider 的实际 payload 和生命周期语义；无法确认的生命周期状态显示为黄色/未知，不会猜测为绿色。

## 状态目录

GUI 负责选择 app-local 状态目录。如需在不打包的情况下测试 hook，可设置 `UFO_STATUS_DIR` 指向一个受控目录。每个事件通过临时文件加原子 rename 写入 `inbox/*.json`；格式错误的事件会被移入隔离目录。

监测器按 `provider:session_id` 维护唯一的会话记录，在可选的悬浮窗模块中最多渲染 8 盏独立状态灯。
