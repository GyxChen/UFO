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
| `PermissionRequest` | 工具调用需要权限决定时 |
| `PermissionDenied` | 自动模式拒绝工具调用时 |
| `PostToolUse` | 工具调用成功之后 |
| `PostToolUseFailure` | 工具调用失败之后 |
| `PostToolBatch` | 一批并行工具调用全部完成后 |
| `Notification` | Claude Code 发送通知时 |
| `MessageDisplay` | 助手消息文本显示时 |
| `SubagentStart` | 子代理被创建时 |
| `SubagentStop` | 子代理完成时 |
| `TaskCreated` | 任务被创建时 |
| `TaskCompleted` | 任务被标记为完成时 |
| `Stop` | Claude 完成一轮回复时 |
| `StopFailure` | 因 API 错误导致轮次结束时 |
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
| `PreModelSwitch` | 模型切换之前（可阻止） |
| `PostModelSwitch` | 模型切换完成后 |
| `Elicitation` | MCP 服务器请求用户输入时 |
| `ElicitationResult` | 用户响应 MCP 请求后 |
| `SessionEnd` | 会话终止时 |

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
| `outcome` | 结果 | `reason`, `status`, `stop_reason` |
| `tool_name` | 工具名 | — |
| `tool_input` | 工具输入 | — |

## 指示灯状态映射

| 指示灯 | 状态 | 含义 | 触发事件 |
|---|---|---|---|
| 🟢 + 加载动画 | `Running` | Agent 正在工作 | `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PostToolBatch`, `PermissionRequest`, `SubagentStart`, `SubagentStop`, `TaskCreated`, `MessageDisplay`, `PreCompact`, `PostCompact`, `PreModelSwitch`, `PostModelSwitch`, `Elicitation`, `ElicitationResult`, `InstructionsLoaded`, `ConfigChange`, `CwdChanged`, `DirectoryAdded`, `FileChanged`, `WorktreeCreate`, `WorktreeRemove`, `Setup` |
| 🟡 黄灯 | `Waiting` | 等待用户输入 | `Stop`, `Notification`, `PermissionDenied`, `TaskCompleted`, `TeammateIdle` |
| 🟢 无动画 | `Completed` | 会话成功结束 | `SessionEnd`（outcome 为 success/completed/ok/done） |
| 🔴 红灯 | `Error` | 错误 | `StopFailure`, 任何带 `error_code` 的事件, outcome 为 error/failed/cancelled/denied 的事件 |

## 清理策略

| 阶段 | 清理行为 |
|---|---|
| `Completed` | 按配置的 `retention_ms`（默认 30 秒）闪烁后移除 |
| `Waiting` | **保留到同一 provider 的下次运行开始时清理**（30 分钟 failsafe 超时） |
| `Running` | 进程死亡 → 立即移除；60 秒无新事件 → 超时移除 |
| `Error` / `Unknown` | 进程死亡 → 立即移除；60 秒无新事件 → 超时移除 |

关键行为：当同一 provider 的新 `Running` 事件到来时，自动清除该 provider 的所有旧 `Waiting` 会话。这实现了"成功状态保留到下次运行"的语义。

## Codex

如果已安装的 Codex CLI 提供了兼容的 hook 或通知回调，使用相同的命令格式并传入 `--provider codex`。启用前需验证 provider 的实际 payload 和生命周期语义；无法确认的生命周期状态显示为黄色/未知，不会猜测为绿色。

## 状态目录

GUI 负责选择 app-local 状态目录。如需在不打包的情况下测试 hook，可设置 `UFO_STATUS_DIR` 指向一个受控目录。每个事件通过临时文件加原子 rename 写入 `inbox/*.json`；格式错误的事件会被移入隔离目录。

监测器按 `provider:session_id` 维护唯一的会话记录，在可选的悬浮窗模块中最多渲染 8 盏独立状态灯。
