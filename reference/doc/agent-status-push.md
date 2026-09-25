# Agent 实时推送（ntfy.sh / Bark）方案

## Context

现在「Agent 状态灯」只能在本机发声（`sound_enabled` → `sound-player.ts`）。人离开电脑时，agent 跑完、出错、或在等授权都收不到通知 —— 而这三个时刻恰恰是最需要通知的。

目标是复用已有的「跳色检测 + Rust 出站 HTTP」两条现成链路，把同样的三个时刻**推送到手机**。手机端不写任何代码：装现成的 ntfy / Bark APP 并订阅一个 topic 即可，因此不需要服务器、SDK、账号，也不需要过华为/小米/OPPO/vivo 的厂商通道。

**已确认的两个决策**：
- **事件范围**：三类都推，与音效**同一套口径** —— 直接复用已测的 `transitionsBetween`，不新写一份跳变逻辑。
- **推送正文**：精简（provider + 状态）为默认，是否附带 endpoint / 提问摘要做成一个开关（默认关）。ntfy.sh 是第三方公共服务器，Bark 走苹果 APNs，提问原文默认不出本机。

## 架构（为什么这样切）

```
Claude/Codex 事件 ─▸ UFO 内核(灯态) ─▸ 前端 watch 比出跳变（已有 transitionsBetween）
                                            │ 红 / 黄 / 绿闪
                                            ▼
                              invoke('send_push', {service,target,title,body})
                                            ▼
                                Rust push.rs（ureq，固定 host）
                                            ▼
                                ntfy.sh  /  api.day.app  ─▸ 手机 APP
```

- **检测留在前端**：跳变逻辑 `transitionsBetween`（[agent-sound.ts:98](src/modules/agent-status/lib/agent-sound.ts#L98)）已经写好且在 spec 里锁着。放 Rust 重写一份是纯粹的重复与风险。前端 watch 里那几行就是现成的挂点。
- **发送放 Rust**：渲染进程受 CSP `connect-src` 限制不能直连外网（信息卡脚本的 HTTP 也是因此走 Rust）。Rust 侧 `ureq = "3"` 已经在用（[weather/client.rs:16](src-tauri/src/modules/weather/client.rs#L16)），零新增依赖。
- **策略全在前端，Rust 只做「按服务发一条」**：`send_push` 不读配置、不认识 agent 语义 —— 它收 `{service, target, title, body}`，按 `service` 从**固定 host 表**拼 URL。这样前端拿不到一个「POST 任意地址」的任意 HTTP 原语，出网范围被钉死在 ntfy.sh / api.day.app 两个域名。
- **Rust 侧 `settings.rs` 不需要改**（字段全由前端解释，与 `sound_enabled` / `token_unit` 同一处理方式）。

## 改动清单

### 1. 新增 `src-tauri/src/modules/agent_status/push.rs`

- `enum PushService { Ntfy, Bark }`（`serde(rename_all = "lowercase")`），字段逐个注释；`fn host(&self)` 返回固定 host。
- `#[tauri::command] pub async fn send_push(service, target, title, body) -> Result<(), String>`
  - ⚠️ **必须 `async` + `tauri::async_runtime::spawn_blocking`**：ureq 是阻塞的，Tauri 的非 async 命令跑在**主线程**，网络卡住会把悬浮窗冻住整段超时（同 [flow/mod.rs:629](src-tauri/src/modules/agent_status/flow/mod.rs#L629) `run_flow_test` 那条踩过的坑）。带引用入参的 async 命令按 tauri 宏要求返回 `Result`。
  - ⚠️ **不得 panic**：release 是 `panic = "abort"`，一次 panic 带走整个 App。全程 `Err(String)`，不 unwrap / expect / 索引。
  - **Rust 侧独立校验**（前端不是信任边界）：`target` 非空、长度上限、字符白名单 —— ntfy topic 允许 `[A-Za-z0-9_-]`，Bark key 允许 `[A-Za-z0-9_-]`。白名单校验同时挡住路径穿越 / 查询串注入（target 会进 URL 路径）。
  - 超时 5s（`timeout_global`）、`http_status_as_error(false)`、响应体截断 256KB —— 照 [weather/client.rs:15-42](src-tauri/src/modules/weather/client.rs#L15-L42) 的写法。
- ⚠️ **两个服务的正文编码都不一样，按服务分流**：
  - **ntfy**：用 **JSON publish** `POST https://ntfy.sh/`，body `{"topic":…, "title":…, "message":…}`。**不要**用 `Title:` 头 —— ntfy 的 HTTP 头必须是 ASCII，中文标题放进头会失败。
  - **Bark**：`POST https://api.day.app/<key>`，JSON body `{"title":…, "body":…}`。
  - 落地前对着官方文档核一遍这两个端点的当前写法。
- 不加单独的 `test_push` 命令：设置页的「发送测试」直接调 `send_push` 发一条固定文案，返回值就是结果。

### 2. `src/modules/agent-status/settings.ts`

新增 4 个字段（写进 `AgentStatusSettings` + `DEFAULT_AGENT_STATUS_SETTINGS` + `agentStatusSettings()` 的宽松取值，风格照抄现有 `sound_enabled` / `proxy_resolver`）：

| 字段 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `push_enabled` | boolean | `false` | 主开关。**默认关**（与 `sound_enabled` 默认开相反）：推送要先在手机装 APP、订阅 topic，没配好之前开着只会白白发失败请求 |
| `push_service` | `'ntfy' \| 'bark'` | `'ntfy'` | 只认这两个字面量，其它值回落默认 |
| `push_target` | string | `'2xzzikf3zg'` | ntfy = topic；Bark = 设备 Key。trim + 字符白名单过滤 + 长度上限。默认值就是本次生成的随机 topic（见下） |
| `push_include_prompt` | boolean | `false` | 正文是否附带 endpoint + 提问前 80 字 |

**生成的随机 topic：`2xzzikf3zg`**（完整订阅地址 `https://ntfy.sh/2xzzikf3zg`）。
⚠️ 把它做成默认值是**为这次自用/自编译**图方便。如果这个 build 将来发给别人，必须改掉：所有人共用同一个 topic = 你的手机收到别人的 agent 状态、且彼此都能看到。打算分发就把它改成空串默认 + 由设置页的「随机生成」按钮（见下）逐人生成。

不加节流/冷却字段：跳变是**边沿触发**的（`transitionsBetween` 只在状态**变化**那一帧返回），本来就不会重复推；加冷却反而可能吃掉一次真实的「等你操作」。

### 3. 新增 `src/modules/agent-status/lib/push.ts`（纯逻辑 + 薄封装）

照 `lib/agent-sound.ts` 的分工（逻辑与副作用分开，逻辑可单测）：

- `type PushService = 'ntfy' | 'bark'`
- `PUSH_SERVICE_LABELS` / `PUSH_SERVICE_HINTS`：下拉与提示文案（含「去哪拿 target」：ntfy = APP 里订阅的 topic；Bark = APP 首页那串 Key）。
- `isValidPushTarget(service, target): boolean` —— 与 Rust 侧同一套字符白名单，让设置页能即时提示、不必等一次失败请求。
- `pushTargetPreview(service, target): string` —— 展示用完整地址（`https://ntfy.sh/<topic>` / `https://api.day.app/<key>`），空 target 返回带占位符的样例。
- `randomNtfyTopic(): string` —— 10 位 `[a-z0-9]`，用 `crypto.getRandomValues` 取随机字节。
- `buildPushMessage(kind, sessions, includePrompt): { title, body }` —— 把 `LightSound`（`'error' \| 'warn' \| 'success'`）映射成标题与正文；`includePrompt` 为真时逐行附 endpoint（复用 `lib/agent-status.ts` 的 `displayEndpoint`）与提问前 80 字。多会话时正文列出各会话。
- `sendPush(...)` —— `invoke('send_push', …)` 的薄封装。

配套 `lib/push.spec.ts`：锁字符白名单、`randomNtfyTopic` 的形状、`buildPushMessage` 在「单会话 / 多会话 / 带不带 prompt」下的输出。

### 4. `src/modules/agent-status/AgentStatusModule.vue` —— 复用现有 watch

改 [第 96-107 行](src/modules/agent-status/AgentStatusModule.vue#L96-L107) 那个 watch，**不新增 watch、不新增基线**：

```ts
const fired = transitionsBetween(lightClasses, next)
if (soundEnabled.value) for (const sound of fired) playLightSound(sound)
if (pushEnabled.value) for (const kind of fired) void sendPushFor(kind, snapshot?.sessions ?? [])
lightClasses = next   // 基线照旧推进（两个开关都关也要推进，理由同现有注释）
```

- 与音效共用同一个 `lightClasses` 基线 → 「挂载那一刻的已有灯不算跳色」「关着开关期间的变化不事后补推」两条语义自动一致。
- `sendPushFor` 是模块内的小函数：`buildPushMessage` → `sendPush`，`.catch` 只 `console.warn`（推送失败绝不能影响灯与音效）。
- 同样受「模块视图只在悬浮窗挂载」约束 —— 组件被关掉就没有推送，与音效同理。

### 5. `src/modules/agent-status/sections/GeneralSection.vue` —— 新增「手机推送」卡

在 `.general-cards` **之后**、`.module-settings__actions` **之前**插一张整宽卡（复用 `.ufo-card module-settings`，模块自己的类加到 `agent-status.css`）。卡内一行一项：

1. `BaseSwitch` → `push_enabled`。**只有开关本身绑 `@change` 落盘**（CLAUDE.md「精准命中目标」：点文字/点空白不得误触发）。
2. `BaseSegmentedControl`（ntfy / Bark）→ `push_service`；切换后 target 的标签与提示跟着换。
3. `BaseInput` → `push_target`，标签随服务变（「ntfy topic」/「Bark 设备 Key」）；旁边一个 `BaseButton`「随机生成」（仅 ntfy 显示，调 `randomNtfyTopic`）；下方用 `pushTargetPreview` 显示完整地址，Bark 模式另附「iPhone 装 Bark，Key 在 APP 首页」的提示。
4. `BaseSwitch` → `push_include_prompt`，提示写明「打开后提问前 80 字会经 ntfy.sh / 苹果服务器」。
5. `BaseButton`「发送测试」→ 调一次 `send_push` 发固定文案，结果走 `role="status"` 行内提示。
   ⚠️ **不能用 `window.alert` / `confirm`** —— 装了 `tauri-plugin-prevent-default`，原生对话框会被拦。照 `EndpointFlowEditor` / 会话记录面板的行内提示做法。
6. 更新 `watch(settings, …)` 与 `resetInputs()`，把 4 个新 `ref` 一起同步。

### 6. 命令注册（三处）

- `src-tauri/src/lib.rs` 的 `invoke_handler` 加 `send_push`（[第 221-260 行](src-tauri/src/lib.rs#L221-L260)）。
- `src-tauri/permissions/status.toml` 加一条 `[[permission]]`：`identifier = "allow-send-push"`，`commands.allow = ["send_push"]`。
- `src-tauri/capabilities/default.json` 的 `permissions` 加 `"allow-send-push"`。
  ⚠️ **放 `default.json` 而不是 `main-module-settings.json`**：`send_push` 是**悬浮窗**里的 watch 调用的（`default.json` 已覆盖 `floating` + `main` + `tooltip`），只授权 main 会让真推送静默失败；设置页的「发送测试」在 main，同一个 capability 一并覆盖。

### 7. 测试

- `src/modules/agent-status/settings.spec.ts`：新增 4 字段的默认值、类型写错回落、target trim/过滤/截断。
- `src/modules/agent-status/lib/push.spec.ts`：见第 3 点。
- `src/modules/agent-status/sections/GeneralSection.spec.ts`：跟着新控件更新。
- Rust `push.rs` 内联 `#[cfg(test)]`：URL 拼接 + target 白名单校验的纯函数部分（不发真请求）。

### 不动的东西

- `src-tauri/src/modules/agent_status/settings.rs`（字段全由前端解释）
- 外壳（`src/lib/floating-*` / `src/components/floating/**` / `stores/floatingModules.ts`）—— 零改动，仍是「外壳不认识任何模块语义」
- `SCHEMA_VERSION` —— 新增字段全在模块配置块内，**不得升版本**

## 验收

1. `npx vitest run` 全绿；`npm run build` 通过。
2. `cd src-tauri && cargo test`（push.rs 的纯函数）通过；`cargo build` 无 panic 风险点（人工核一遍 push.rs 无 unwrap/expect/索引）。
3. `rg -n "ntfy|bark|push_" src/App.vue src/components/floating src/lib/floating-modules.ts src/lib/floating-module-context.ts` **应当无命中**（模块词汇不许漏进外壳）。
4. `rg -n "ntfy|bark" src-tauri/src/floating src-tauri/src/lib.rs` 只应命中 `lib.rs` 里 `send_push` 这一行注册。
5. **端到端**：手机装 ntfy → 订阅 `2xzzikf3zg` → 主窗「Agent 状态灯 → 常规 → 手机推送」打开开关 → 点「发送测试」→ 手机收到。然后开一个 Claude 会话跑到「等你授权」，确认黄灯推送到达且正文不含提问（`push_include_prompt` 关着）；打开该开关再跑一次，确认正文带上提问摘要。
6. 反例：`push_enabled` 关着时，跑完整一轮状态变化，**手机一条都不该收到**（同时确认不影响音效）。

## 已知边界（本轮不做）

- **UFO 必须开着**：PC 关机 / 休眠 / UFO 退出 = 收不到推送。agent 也在这台机器上跑，所以这个场景下影响有限；但它是「自建触发端」的固有代价。
- **host 写死**：自建 ntfy / 自建 Bark 服务器不支持。要支持得加 `push_server_url` 字段 —— 但自建 ntfy 常在本机 loopback，会与出网校验规则冲突，需要单独设计，故不混进这一轮。
- **单 target**：多设备要各自配。多填一个就需要 target 数组 + 逐个发，本轮不做。
- 无重试、无冷却（边沿触发已足够）。
