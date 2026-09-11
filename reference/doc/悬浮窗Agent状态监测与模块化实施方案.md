# 悬浮窗 Agent 状态监测与模块化实施方案

> 本文是可脱离 `.claude/plans` 使用的实施交接文档。将仓库复制到其他电脑后，可直接根据本文继续实现、调试和验收。

## 1. 目标与已确认规则

为 UFO 悬浮窗增加 Claude Code / Codex CLI 执行状态感知能力，并将状态灯实现为可选的悬浮窗内容模块。

已确认的产品规则：

- 一个 CLI 会话对应一盏独立状态灯。
- Claude 和 Codex 的会话必须隔离；同一工具也支持多个并发会话。
- 每个会话单独显示红、黄、绿状态，不使用一盏聚合灯替代多个会话。
- 首版最多保证同时显示 8 个会话灯；8 个以内必须全部显示，不能用 `+N` 或聚合颜色替代。
- 超过 8 个时不能静默丢弃数据；主窗口仍保留完整会话列表，并明确提示超过悬浮窗首版显示上限。
- Claude/Codex 状态灯只是一个可选模块，主窗口可以启用或停用。
- 状态模块只占悬浮窗可用空间的一半，另一半保留给现有内容或未来模块。
- 悬浮窗必须继续兼容四种实际形态：未贴边方形、角落方形、上下贴边横条、左右贴边竖条。
- 成功完成的会话显示绿色并闪烁，默认保留/闪烁 30 秒，时长可在主窗口配置。
- 绿色完成灯即使被 10 秒终止检查发现 CLI 已退出，也必须先完整保留到配置时长结束后再移除。
- 红色错误灯保持红色，不转为绿色；每 10 秒检查一次对应 agent 是否终止，确认终止后移除。
- 黄色等待/未知状态同样由终止检查负责清理；没有可靠终止证据时继续保留。
- `Stop`、单轮 turn complete 或普通通知不能直接等价为成功完成；只有明确的 session-end/success 语义才进入绿色完成态。
- 监测目标是外部 CLI 会话，不把 UFO 改造成 CLI 启动器或 CLI 托管器。

## 2. 当前代码基础

项目是 Vue 3 + TypeScript + Pinia + Tauri 2/Rust。

关键现有文件：

- `src/components/FloatingWindow.vue`：悬浮窗拖拽、点击打开主体和贴边触发点。
- `src/lib/snap.ts`：根据显示器工作区计算贴边位置、尺寸和 `docked` 边缘状态。
- `src/components/MainWindow.vue`：主体窗口。
- `src/App.vue`：按 Tauri 窗口 label 分流 `floating` 和 `main`。
- `src/main.ts`：每个 WebView 独立创建 Vue/Pinia 实例。
- `src/store.ts`：现有基础 Pinia store。
- `src/assets/main.css`：悬浮窗透明背景和基础样式。
- `src-tauri/src/lib.rs`：Rust 生命周期、托盘、窗口关闭行为和 Tauri command 注册。
- `src-tauri/src/main.rs`：Rust 二进制入口。
- `src-tauri/tauri.conf.json`：静态定义 `floating` 与 `main` 两个窗口。
- `src-tauri/capabilities/default.json`：前端 WebView 权限。
- `src-tauri/Cargo.toml`：Rust 依赖。

现有悬浮窗尺寸由 `src/lib/snap.ts` 约定：

- 默认方形：基础尺寸 56x56。
- 上下贴边：横条，宽度为基础宽度的 4 倍。
- 左右贴边：竖条，高度为基础高度的 4 倍。
- 角落贴边：方形，宽高为基础尺寸的 2 倍。

不要用 CSS 的宽高反推形态；应直接使用 `snapToWorkArea` 返回的 `docked` 边缘状态。

## 3. 推荐总体架构

使用以下数据链路：

```text
Claude Code / Codex hook
        |
        | stdin JSON -> UFO hook bridge
        v
app-local-data/status/inbox/<event-id>.json
        |
        | Rust monitor 校验、归一化、消费
        v
StatusRegistry + provider adapter + reducer
        |
        | 每 10 秒执行 liveness / termination 检查
        v
Tauri core process 中的全量 StatusSnapshot
        |
        | agent-status 事件 + revision
        v
main WebView / floating WebView
        |
        +--> agent-status Pinia store
        +--> FloatingModuleConfig Pinia store
        +--> FloatingModuleHost
                  |
                  +--> AgentStatusModule（可选）
                  +--> 未来模块槽位
```

监测器只允许在 Tauri core process 中启动一次。不能让 `main` 和 `floating` 两个 WebView 各自启动监测线程，否则会产生重复扫描、重复事件和状态竞争。

前端初始化顺序：

1. `listen("agent-status")`。
2. `listen("floating-module-config")`。
3. `invoke("get_status_snapshot")` 获取当前状态。
4. `invoke("get_floating_module_config")` 获取当前模块配置。
5. 使用 `revision` 忽略旧事件。

这样可以避免 WebView 尚未完成监听时错过后端已经发出的事件。

## 4. 状态协议

Rust 与 TypeScript 使用同一份版本化 JSON 语义。建议字段如下。

### 4.1 Provider

```text
claude
codex
```

### 4.2 SessionPhase

```text
running
waiting
error
unknown
completed
```

`completed` 是可展示的终态，不是立即删除标记。它必须带完成时间、移除时间和闪烁标志。

### 4.3 AgentEvent

```text
schema_version
 event_id
provider
session_id
event
sequence?
occurred_at_ms?
received_at_ms
pid?
error_code?
outcome?
```

事件数据只保留状态所需的元数据，不应包含：

- prompt 或用户原文。
- transcript 内容。
- 工具参数和工具输出。
- API key、token 或环境变量。
- 不必要的完整命令行。

### 4.4 SessionSnapshot

```text
key                  = provider + ":" + session_id
provider
session_id
phase
source               = hook | process_fallback
confidence
started_at_ms
last_seen_at_ms
completed_at_ms?
remove_at_ms?
is_flashing
pid?
error_code?
last_sequence?
termination_confirmed
```

`key` 必须包含 provider 命名空间。例如：

```text
claude:abc123
codex:abc123
```

这样相同的 session id 不会互相覆盖。

### 4.5 StatusSnapshot

```text
schema_version
revision
generated_at_ms
sessions[]
counts
source_health
```

`sessions[]` 是浮窗和主窗口的真实数据源。可以提供 aggregate/count 作为主窗口摘要，但不能用 aggregate 替代逐会话灯。

## 5. 单灯颜色与生命周期

每个会话独立执行以下映射：

- `running`：绿色常亮，表示正在执行 turn/tool。
- `waiting`：黄色，表示等待输入、等待权限或回到下一轮输入。
- `unknown`：黄色，表示 hook 数据不足、状态过期或监测源降级。
- `error`：红色常亮，表示明确错误、失败、取消或权限拒绝。
- `completed`：绿色闪烁，表示明确成功完成。

完成规则：

1. 收到明确成功 session-end/success 事件。
2. 将 phase 设为 `completed`。
3. 设置 `completed_at_ms = now`。
4. 设置 `remove_at_ms = completed_at_ms + configured_retention_ms`。
5. 设置 `is_flashing = true`。
6. 在 `remove_at_ms` 之前始终保留该灯。
7. 到期后移除会话并广播新快照。

错误规则：

1. 收到明确错误事件后设置 phase 为 `error`。
2. 红灯保持红色，即使后续收到普通 Stop 或 turn 事件，也不能覆盖错误态。
3. Rust monitor 每 10 秒执行终止检查。
4. 只有确认对应 agent 已终止后才移除红灯。
5. 没有可靠 PID 或终止证据时继续保留，不得误删。

重要边界：

- Claude Code 的 `Stop` 一般表示本轮停止，不一定代表整个 session 成功结束，因此默认映射为 `waiting`。
- `turn_completed`/`task_completed` 默认映射为 `waiting`，不能直接触发绿色完成灯。
- session-end 携带 `error`、`failed`、`cancelled` 或 `denied` 时，保持红灯。
- 进程存在只能辅助判断 liveness，不能单独证明 agent 正在运行。

## 6. Hook 接入

### 6.1 Claude Code

Claude Code 使用 hook/状态输入作为主来源。第一步必须针对目标 Claude Code 版本收集真实 stdin fixture，确认：

- 实际事件字段名。
- session id 字段。
- hook 事件名和顺序。
- Stop 与 SessionEnd 的语义。
- 错误原因字段。
- 是否提供 PID。
- hook 的超时和失败行为。

UFO hook bridge 的调用形状：

```text
ufo --status-hook --provider claude
```

hook bridge：

1. 从 stdin 读取 JSON。
2. 限制输入最大长度，建议 64 KiB。
3. 只提取白名单字段。
4. 归一化为 `AgentEvent`。
5. 写入状态 inbox 的独立事件文件。
6. 快速返回，不等待 GUI，不启动 Tauri 窗口。

首版不要自动覆盖用户已有 Claude 配置。提供手动配置片段或设置页复制说明。未来自动注册时必须：

- 备份原配置。
- 合并已有 hooks，而不是覆盖。
- 原子写入。
- 提供卸载和恢复。

### 6.2 Codex

Codex 必须先做 capability probe：

- 当前安装版本是否公开提供 hook/notify。
- stdin 或通知 payload 是否稳定。
- 是否有可靠 session id。
- 是否能区分 running、waiting、error 和 session end。

如果 Codex 只有不完整的 notify/turn-complete 事件，只映射可证明的状态，无法证明时显示黄色 `unknown`/`waiting`，不要伪造绿色。

Claude 与 Codex 应使用独立 provider adapter，但复用同一 `AgentEvent`、`StatusRegistry` 和 `StatusSnapshot`。

### 6.3 Hook inbox

目录结构建议：

```text
<app-data>/status/
  inbox/
  quarantine/
```

每次 hook 调用写一个不可变事件文件：

```text
inbox/<generated-event-id>.json
```

写入流程：

1. 在 inbox 中写临时文件。
2. `flush`/`sync_all`。
3. 原子 rename 为 `.json`。
4. Rust monitor 消费后删除或归档。
5. JSON 损坏时移动到 `quarantine`，不能让 monitor 退出。

文件名不能直接使用未经校验的 session id。状态目录由 Rust/Tauri app-data 决定，不允许 renderer 传入任意路径。

## 7. Rust 后端实现

建议目录：

```text
src-tauri/src/status/
  mod.rs
  model.rs
  reducer.rs
  hook.rs
  source.rs       # 如需要拆分 provider/source
  monitor.rs      # 如需要从 mod.rs 拆出 monitor
```

关键对象：

- `StatusState`：Tauri managed state，保存 registry、状态目录、保留时长和停止句柄。
- `StatusRegistry`：按 `provider:session_id` 保存会话，维护 revision 和去重集合。
- `ClaudeAdapter`：Claude provider-specific payload 到 `AgentEvent` 的转换。
- `CodexAdapter`：Codex provider-specific payload 到 `AgentEvent` 的转换。
- `StatusReducer`：事件排序、去重、状态迁移、完成保留和清理。
- `StatusMonitor`：扫描 inbox、quarantine、10 秒终止检查和 Tauri emit。

在 `src-tauri/src/lib.rs` 的 `setup` 中：

1. 获取 app-data 目录。
2. 加载模块配置。
3. 使用配置中的 completed retention 初始化 `StatusState`。
4. `app.manage` 状态。
5. 启动唯一 monitor。
6. 注册快照和模块配置 command。

建议 commands：

```text
get_status_snapshot()
get_floating_module_config()
set_floating_module_config(config)
```

状态事件固定使用：

```text
agent-status
```

配置事件固定使用：

```text
floating-module-config
```

每 10 秒执行一次 liveness/termination check。若 session 没有可靠 PID，不得调用不受限制的进程扫描来猜测；应保留状态并标记 source health degraded，或等待明确 SessionEnd 事件。

不要为了监测功能给 renderer 开放任意 shell、filesystem 或网络权限。

## 8. 模块化悬浮窗

### 8.1 模块契约

建议在 `src/lib/floating-modules.ts` 定义：

```text
FloatingModuleId
FloatingModuleDefinition
FloatingModuleConfigEntry
FloatingModuleConfig
FloatingModuleContext
```

模块定义至少包含：

- 稳定 id，例如 `agent-status`。
- 显示名称。
- 版本号。
- 默认启用状态。
- 支持的四种 layout mode。
- 编译期绑定的 Vue component。
- 最小尺寸和推荐占用策略。

模块只能从编译期注册表加载。配置中的任意字符串不能动态变成可执行组件，避免远程/不可信模块注入。

### 8.2 模块宿主

新增：

```text
src/components/FloatingModuleHost.vue
```

`FloatingWindow.vue` 只负责：

- 拖拽。
- 点击打开主窗口。
- 调用贴边算法。
- 保存 `docked` 和 `layoutMode`。
- 把上下文交给 `FloatingModuleHost`。

`FloatingModuleHost` 负责：

- 根据 `square`、`corner`、`horizontal`、`vertical` 选择 grid/flex 布局。
- 为已启用模块分配稳定槽位。
- 让状态模块占一半空间。
- 为未来模块保留另一半空间。
- 不因为模块内容触发原生窗口 resize。
- 让模块内容不能截获外层拖拽/点击事件。

状态模块关闭时：

- 不渲染状态灯。
- 不伪造状态数据。
- 不停止后台监测，除非未来设置明确要求停止。
- 另一半内容槽继续保留给现有或未来模块。

### 8.3 AgentStatusModule

新增：

```text
src/components/AgentStatusModule.vue
```

该组件只负责渲染 Pinia 中的 `sessions[]`：

- 每个 session 一个 item 和一盏灯。
- 不负责 hook 接收。
- 不负责 Tauri invoke。
- 不负责聚合逻辑。
- 不负责决定悬浮窗尺寸。
- 通过 `layoutMode` 选择紧凑布局。
- 通过 `is_flashing` 开启完成动画。
- 通过 aria-label/title 提供 provider、短 session id 和 phase。

首版半区最多直接显示 8 个 session。8 个以内全部显示，不使用 `+N`、聚合灯或颜色占位。超过 8 个时保留完整数据，在主窗口提示超出上限。

## 9. 主窗口模块设置

新增或扩展 `src/stores/floatingModules.ts`，并由 Rust 持久化配置：

```text
schema_version
revision
modules[]
completed_retention_ms
max_cells_by_layout { square, corner, horizontal, vertical }
```

默认配置：

```text
agent-status: enabled
completed_retention_ms: 30000
max_cells_by_layout: { square: 8, corner: 8, horizontal: 8, vertical: 8 }
```

旧配置若仍含 `max_visible_sessions`，加载时迁移为各形态相同上限后废弃该字段。

主窗口至少提供：

- Agent 状态模块启用/停用开关。
- 完成灯保留/闪烁时长输入，默认 30 秒。
- 各贴边形态格子上限（4..=8）。
- 当前会话诊断列表。
- provider、短 session id、phase、颜色和 source health。
- 超过形态上限时由悬浮窗以 `+N` 提示。

配置保存要求：

- 使用 revision 防止旧窗口覆盖新窗口。
- 原子写入配置文件。
- Windows 上不能假设 rename 会覆盖已有文件；必要时先安全删除或使用平台支持的替换策略。
- 主窗口拥有写配置权限。
- 浮窗只拥有读取配置权限。
- 配置失败时回滚 UI 输入并显示降级状态。

## 10. Pinia 状态层

新增：

```text
src/lib/agent-status.ts
src/stores/agentStatus.ts
src/stores/floatingModules.ts
```

`agentStatus` store：

```text
snapshot
connection
lastRevision
started
```

actions：

```text
start()
stop()
applySnapshot(snapshot)
```

要求：

- `start()` 幂等。
- `stop()` 释放 listener。
- 只接受 revision 不小于当前值的快照。
- 对事件 payload 做 runtime guard。
- `sessions` 按固定规则排序，建议最近更新优先，再按 key 排序。
- `colorForSession` 只根据单个 session 派生颜色。
- completed phase 映射为绿色，且由 `is_flashing` 控制动画。

在 `src/App.vue` 中统一启动两个 store。不能只依赖 `MainWindow.vue`，因为 floating WebView 也必须接收同一状态和模块配置。

## 11. 权限与安全

新增：

```text
src-tauri/permissions/status.toml
src-tauri/capabilities/main-module-settings.json
```

建议权限：

- `allow-get-status-snapshot`。
- `allow-get-floating-module-config`。
- `allow-set-floating-module-config`。

`allow-set-floating-module-config` 只放在 `main-module-settings` capability，窗口范围只包含 `main`。

`default.json` 可给 `main` 与 `floating`：

- status snapshot read。
- module config read。
- 必要的 event listen/unlisten。
- 现有窗口拖拽、位置和尺寸权限。

不要给 floating renderer：

- 任意 shell execute/spawn/kill。
- 任意 filesystem read/write。
- 任意外部网络权限。
- 任意动态组件加载能力。

hook 输入按不可信本地输入处理：

- 限制 JSON 总大小。
- 限制 session id、event id、error code 长度。
- 只允许已知 provider。
- 只接受有限枚举或规范化事件名。
- 不把 session id 拼接为路径。
- 拒绝 `..`、绝对路径和异常 symlink。
- 不广播原始 stdout/stderr。
- 不记录 prompt、token、工具参数和环境变量。

## 12. 分阶段执行顺序

### 阶段一：协议和 CLI 能力探测

- 锁定 schema version、event name、phase、颜色和清理规则。
- 锁定默认 4 格、各形态上限 4..=8；窗口可由模块经壳层 `setWindowCells` 申请放大。
- 锁定状态模块整区单槽布局。
- 锁定默认 30 秒完成闪烁保留时长和可调范围。
- 锁定每 10 秒终止检查。
- 收集目标 Claude Code 版本的真实 hook fixture。
- 对 Codex 做 capability probe，不能确认的状态标为 unknown/degraded。

### 阶段二：模块基础设施

- 定义模块配置协议和编译期 registry。
- 新增 `FloatingModuleHost.vue`。
- 实现两个半区槽位。
- 状态模块关闭时保持空槽位，未来模块可直接注册。

### 阶段三：纯 Rust 状态核心

- 实现 `model.rs`。
- 实现 provider adapter。
- 实现 `StatusRegistry` 和 reducer。
- 实现事件去重、sequence 保护和终态保护。
- 实现 completed 绿灯保留/闪烁/到期移除。
- 实现红灯保持和 10 秒终止检查。
- 使用 fake clock/source 编写单测。

### 阶段四：Hook bridge

- 在 `src-tauri/src/main.rs` 识别 `--status-hook`。
- 读取 stdin 并写入 app-local inbox。
- 使用临时文件加 atomic rename。
- 损坏事件移入 quarantine。
- 提供 Claude/Codex 手动配置说明。
- 不自动覆盖用户现有 CLI 配置。

### 阶段五：Tauri 集成

- 在 `src-tauri/src/lib.rs` setup 创建 managed state。
- 启动唯一 monitor。
- 注册 snapshot/config commands。
- 广播 `agent-status` 和 `floating-module-config`。
- 添加最小 capability 和 permission。

### 阶段六：前端状态与主窗口

- 实现 event wrapper 和 runtime guard。
- 实现 Pinia stores。
- 在 `App.vue` 统一初始化。
- 主窗口加入模块开关、30 秒配置和多会话列表。

### 阶段七：浮窗 UI

- 将 `docked` 转成 `layoutMode`。
- 改造 `FloatingWindow.vue` 使用 `FloatingModuleHost`。
- 实现方形、角落、横条、竖条四种模块布局。
- 状态模块只占一半空间。
- 最多 8 个会话灯全部独立显示。
- 保证状态点不截获拖拽和点击。

### 阶段八：可选 fallback 和发布验证

- 只有 hook 实测漏报后才考虑进程 liveness fallback。
- fallback 必须 allowlist，低置信度，不能将进程存在直接变成绿色。
- 验证 Windows 打包后 hook helper 的路径和参数 quoting。
- 验证应用重启、状态目录权限和并发 hook。
- 文件轮询延迟不足时，再评估命名管道或 socket；保持同一事件协议。

## 13. 测试要求

### Rust 单元测试

- Claude/Codex 使用相同 session id 时互不覆盖。
- 同一 provider 多会话并发。
- 事件 id 按 session 作用域去重。
- sequence 乱序不会覆盖新状态。
- completed 期间普通事件不能恢复为 running。
- Stop/turn complete 映射为 waiting。
- 明确 session-end/success 映射为 completed。
- session-end/error 保持 error。
- completed 绿灯默认保留 30 秒。
- 自定义 retention 生效。
- 10 秒终止检查删除红灯。
- 10 秒检查不能提前删除 completed 绿灯。
- 无 PID/无法判断 liveness 时不误删。
- 坏 JSON、超大 JSON、未知 provider、字段过长和路径异常。
- 单个坏事件不会停止 monitor。
- 原子写入产生完整 JSON 文件。

### TypeScript/Vue 测试

- snapshot runtime guard。
- revision 去重。
- listener 初始化和卸载。
- Pinia `start/stop` 幂等。
- 每个 phase 的单灯颜色映射。
- completed 映射为绿色并开启闪烁。
- 8 个会话全部渲染。
- 不产生 aggregate 总灯或 `+N` 状态占位。
- 方形、角落、横条、竖条 layout helper。
- 状态模块只占半区。
- 模块关闭后不渲染状态灯。
- 主窗口配置开关和保留时长回滚。
- 状态点不阻断拖拽、点击和打开主体窗口。
- 保持现有 `src/lib/snap.spec.ts` 的负坐标、多显示器和尺寸测试。

## 14. 验证命令

在仓库根目录执行：

```text
pnpm test --run
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
pnpm check
```

Tauri 打包验证：

```text
pnpm tauri build --debug --no-bundle
```

如果 Windows 报错无法删除 `src-tauri/target/debug/tauri-app.exe`，先关闭正在运行的调试程序或占用该 exe 的进程，再重新执行；不要直接把它当成源代码编译失败。

## 15. 手工验收场景

- 两个 Claude 会话并发，显示两盏独立灯。
- 两个 Codex 会话并发，显示两盏独立灯。
- Claude 与 Codex 同时运行，provider 不互相覆盖。
- 同一 provider 的多个项目并发。
- 运行中、等待输入、等待权限、明确错误、明确成功完成。
- 成功完成后绿色闪烁完整 30 秒，时长可调。
- 红灯保持红色，10 秒检查确认终止后移除。
- 绿色完成灯在 10 秒终止检查后仍保留到 retention 到期。
- hook 缺失、payload 损坏、状态目录不可读。
- 应用关闭/重启后旧状态不直接恢复为绿色。
- 悬浮窗未贴边方形、角落方形、上下横条、左右竖条。
- 状态模块开启和关闭。
- 8 个会话同时显示，且另一半内容槽保持可用。
- 超过 8 个时主窗口完整列出并提示上限，不能静默丢弃。
- 多显示器、负坐标和不同 DPI。
- hook 配置与已有用户配置共存。
- 未授权 floating renderer 无法写模块配置、读任意文件或执行任意命令。

## 16. 类 claude-hud 的借鉴边界

可以借鉴 `@sooneocean/claude-hud` 的总体思路：使用 Claude Code 原生状态输入、按 session 维护状态、从 transcript/事件中解析工具和 agent 活动，并持续刷新 HUD。

不要直接复制其内部字段或假设其事件名。UFO 必须以目标 Claude Code 版本实际 hook fixture 为准，并使用自己的 `AgentEvent` 协议和 Rust registry。

UFO 与普通终端 HUD 的差异：

- 状态灯是一个可启停的悬浮窗模块。
- 每个 CLI session 一盏独立灯。
- 首版最多直接显示 8 盏灯。
- 状态模块只占悬浮窗一半。
- 成功完成显示绿色闪烁，默认保留 30 秒且可调。
- 红灯保持红色，10 秒终止检查确认后移除。
- 需要同时支持 Claude Code 和经过 capability probe 的 Codex。

参考包：

- [@sooneocean/claude-hud on npm](https://www.npmjs.com/package/@sooneocean/claude-hud)

## 17. 关键文件清单

后端：

- `src-tauri/src/main.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/status/model.rs`
- `src-tauri/src/status/reducer.rs`
- `src-tauri/src/status/hook.rs`
- `src-tauri/src/status/mod.rs`
- `src-tauri/src/module_config.rs`
- `src-tauri/permissions/status.toml`
- `src-tauri/capabilities/default.json`
- `src-tauri/capabilities/main-module-settings.json`
- `src-tauri/Cargo.toml`

前端：

- `src/App.vue`
- `src/components/FloatingWindow.vue`
- `src/components/FloatingModuleHost.vue`
- `src/components/AgentStatusModule.vue`
- `src/components/MainWindow.vue`
- `src/lib/agent-status.ts`
- `src/lib/agent-status-layout.ts`
- `src/lib/floating-modules.ts`
- `src/stores/agentStatus.ts`
- `src/stores/floatingModules.ts`
- `src/assets/main.css`

测试和文档：

- `src/lib/agent-status-layout.spec.ts`
- `src/lib/snap.spec.ts`
- `reference/doc/agent-status-hooks.md`
- `reference/doc/悬浮窗Agent状态监测与模块化实施方案.md`
