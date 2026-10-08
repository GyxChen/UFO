# /draw

生成一份可离线打开的 Agent 状态机 HTML 说明页。用户补充输出路径：$ARGUMENTS

只在用户显式调用 `/draw` 时生成文档；不修改运行时代码，不提交，不启动 dev server。

## 输出路径

- `$ARGUMENTS` 为空：写入 `reference/doc/agent-status-state-machine.html`。
- `$ARGUMENTS` 以 `.html` 结尾：写入该文件路径。
- 其它情况：把 `$ARGUMENTS` 当作目录，写入 `<目录>/agent-status-state-machine.html`。
- 路径可以是相对当前仓库的路径，也可以是绝对路径；父目录不存在时先创建。

## 先读源码

必须先从当前仓库读取事实，不能只凭本命令里的摘要生成。至少检查：

- `src-tauri/src/modules/agent_status/status/kernel.rs`
- `src-tauri/src/modules/agent_status/status/model.rs`
- `src-tauri/src/modules/agent_status/status/compat/claude.rs`
- `src-tauri/src/modules/agent_status/status/compat/codex.rs`
- `reference/doc/rule/agent-status-hooks.md`

用 `rg` 定位并阅读这些关键点：

- `AgentEventKind`
- `SessionState`
- `SessionPhase`
- `to_phase`
- `from_phase`
- `reduce_state`
- `apply_event_with_seed`
- Claude 的 `classify`
- Codex 的 `classify_rollout_line`

如果源码与预期不一致，以源码为准，并在页面里如实呈现。

## HTML 内容

页面用中文，面向想快速理解状态机的开发者。必须包含：

1. 总览
   - 兼容层如何把 Claude / Codex 原始事件归一化成 `AgentEventKind`。
   - 内核如何用 `SessionState` 做精确状态机，再投影成对外 `SessionPhase`。
   - 状态灯语义：绿常亮、绿闪落黄、黄常亮、红、绿闪移除。

2. 状态表
   - 内部状态：`Running`、`WaitingForPrompt`、`WaitingForUser`、`BackgroundRunning`、`Completed`、`Failed`、`Unknown`。
   - 对外投影：`Running | BackgroundRunning -> Running`，`WaitingForPrompt | WaitingForUser -> Waiting`，`Completed -> Completed`，`Failed -> Error`，`Unknown -> Unknown`。
   - 每个状态说明它代表的行为、颜色、是否闪烁、是否会自然移除。

3. 事件表
   - `Running`、`TurnFinished`、`WaitingForUser`、`SessionFinished`、`SubagentStarted`、`SubagentStopped`、`Failed`、`Unknown`。
   - 每个事件写清楚：来源含义、如何驱动状态、是否受 `agent_id` 区分、是否产生绿闪。
   - 明确 `Unknown` 是保守处理，保持上一状态，不驱动灯色。

4. 状态流向图
   - 用自包含 HTML/CSS/SVG 绘制，不能依赖外部库、CDN、字体或图片。
   - 至少展示：主线程运行、主线程结束、等待用户、子代理开始、子代理结束、会话结束、失败、未知事件。
   - 图上标出优先级：`WaitingForUser > 子代理后台运行 > 空闲等待`。
   - 支持点击状态或事件时高亮相关路径；没有 JavaScript 时也必须能看懂。
   - 状态机图线不能缠绕：边线必须走预留轨道，优先正交路由，避开节点、标签和其它边；禁止交叉重叠、穿节点、压文字，同一状态的多条转移要分流；复杂转移必须拆成独立小图或分层子图。
   - 每条转移边必须在预留标签位标出触发事件名；多个事件要全部列出。事件名或事件标记需要支持悬停、键盘聚焦和触屏点击查看说明，说明至少包含“发生时会怎样 / 不变量 / 常见误解”；弹窗要避开节点、边线和当前焦点，并在视口内自动换侧。

5. 子代理后台运行
   - 子代理与主会话共用 `session_id`，靠 `agent_id` 区分。
   - `SubagentStarted` 和带 `agent_id` 的 `Running` 加入活跃集合。
   - `SubagentStopped` 从集合移除。
   - 集合非空时，主线程 `TurnFinished` 应保持 `BackgroundRunning`，对外仍是绿灯。
   - 最后一个子代理结束后进入 `WaitingForPrompt`，直接黄灯，不重新绿闪。
   - `Completed` / `Failed` 不接受迟到子代理事件复活。

6. 关键时序
   - 主线程 Run -> TurnFinished 且无子代理：先 `WaitingForPrompt` + 绿闪，retention 后落在黄灯。
   - 主线程 TurnFinished 但已有子代理：进入 `BackgroundRunning`，保持绿灯，清除等待落黄计时。
   - 子代理运行中主线程 WaitingForUser：保持 `WaitingForUser`，子代理事件不能把黄灯顶回绿色。
   - SessionEnd：进入 `Completed`，绿闪，到期移除。
   - Failed：进入 `Failed`，红灯，不可被迟到子代理事件覆盖。
   - Unknown：保持上一状态。

7. Claude / Codex 映射
   - Claude：`subagent_start` -> `SubagentStarted`；`subagent_stop` -> `SubagentStopped`；`stop` -> `TurnFinished`；权限、提问、等待类通知 -> `WaitingForUser`；`SessionEnd` -> `SessionFinished`；错误事件 -> `Failed`。
   - Codex：至少覆盖 `session_meta`、`task_started`、`user_message`、`task_complete`、`turn_aborted`、`token_count` 的真实当前行为。
   - 如果当前 Codex 没有子代理事件，明确写出只是兼容层尚未产生，而不是内核不支持。

8. 事件功能说明
   - 每条事件用“发生时会怎样 / 不变量是什么 / 常见误解”三段式说明。
   - 特别解释：为什么子代理事件不能直接等价于主线程 Running。
   - 特别解释：为什么最后一个子代理停止不重新绿闪。

9. 源码索引
   - 页脚列出本次读取的事实来源路径。
   - 可以列主要函数名，但不要写死脆弱行号。

## 交互与视觉

- 单文件 HTML，内嵌 CSS 和 JavaScript。
- 深色哑光控制台风格，使用扁平表面、细边框、清晰层级；不要做营销落地页。
- 桌面和移动端都要可读；窄屏时表格允许横向滚动。
- 图表优先用语义化 HTML + CSS 连线；需要 SVG 时也内嵌在页面。
- 交互只做局部增强：事件/状态高亮、过滤、展开说明。默认打开也能完整阅读。
- 事件名默认始终可见；悬停或聚焦事件名/事件标记时弹出事件说明，触屏点击也能打开。弹窗不能遮挡触发点，靠近视口边缘时要自动换侧并夹在可视区域内。
- 颜色要与状态语义一致：绿 `#34D399`、黄 `#FBBF24`、红 `#F87171`，不要用装饰性渐变或光斑。

## 完成标准

- 生成前事实来自源码检查。
- HTML 能在浏览器直接打开，不请求网络资源。
- 页面覆盖状态、事件、投影、优先级、子代理集合、关键时序和平台映射。
- 生成后报告实际绝对路径和读取过的源码文件。
- 不修改任何运行时代码，不提交。
