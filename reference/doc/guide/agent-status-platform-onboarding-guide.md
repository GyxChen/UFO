# Agent 平台接入 UFO 状态灯

> **适用范围**：让一个 AI CLI（Claude Code、Codex、或任何第三方 agent）的运行状态出现在 UFO 的悬浮窗指示灯上。
> **参考实现**：`compat/claude.rs` = hook 路径的范本；`status/codex_rollout.rs` + `compat/codex.rs` = 日志采集路径的范本。
> **与 AGENTS.md 的关系**：`AGENTS.md` 已收敛成不超过 100 行的**稳定规则 + 指向本目录的链接**，不再收各子系统细则；「接一个新平台」的完整契约**只在本文**。文档树是 `reference/doc/guide/`（指南，含本篇）与 `reference/doc/rule/`（规范，如 `../rule/agent-status-hooks.md`）。**冲突时一律以代码为准** —— 包括与 `AGENTS.md` 的冲突（行号与数字会漂，符号名不会）。
> **一句话版本**：平台侧配一行 hook 命令把 stdin 的 JSON 交给 `ufo --status-hook --provider <name>`，payload 里只要有「会话 id + 事件名」，灯就会亮；UFO 侧登记 `Provider` 与适配器即可，内核不用动。

## 目录

1. [hook 命令怎么配](#1-hook-命令怎么配)
2. [payload 最少要给什么](#2-payload-最少要给什么)
3. [事件 → kind → 灯色](#3-事件--kind--灯色)
4. [UFO 取不到的状态](#4-ufo-取不到的状态)
5. [线协议与限度](#5-线协议与限度)
6. [自测接线](#6-自测接线)
7. [全景：两条事件源路径](#7-全景两条事件源路径)
8. [路径 A：有 hook 能力的平台](#8-路径-a有-hook-能力的平台)
9. [路径 B：只有日志文件的平台](#9-路径-b只有日志文件的平台)
10. [前端 provider 注册表](#10-前端-provider-注册表)
11. [协议演进红线](#11-协议演进红线)
12. [验收清单](#12-验收清单)
- [附录：常量与不变量速查](#附录常量与不变量速查)

**§1–§6 面向平台侧**（不需要读懂 UFO 的代码，照抄即可）；**§7–§12 面向 UFO 侧**（真实改动清单）。

---

# Part 1 · 平台侧：把状态推给 UFO

## 1. hook 命令怎么配

UFO 的可执行文件在安装语境里叫 `ufo`：`pnpm tauri build` 按 `tauri.conf.json` 根级 `mainBinaryName` 把 cargo 产物改名成 `UFO.exe`，安装器再把安装目录追加进**当前用户**的 PATH（MSI 与 NSIS 两条通道各有一份实现，见 `../rule/agent-status-hooks.md` 的「安装前提」）。⚠️ `pnpm tauri dev` / `cargo build` **不改名**，产物仍是 `src-tauri/target/debug|release/tauri-app.exe`，所以本地自测只能写绝对路径（见下文自测脚本）；而装过安装包之后，PATH 里那一条要等 agent 宿主进程重启才被读到。任何支持「事件触发时跑一条命令、把 JSON 喂进 stdin」的 CLI 都能接：

```json
{
  "hooks": {
    "Stop": [
      { "hooks": [{ "type": "command", "command": "ufo --status-hook --provider <name>", "async": true, "timeout": 3 }] }
    ]
  }
}
```

| 项 | 约定 |
|---|---|
| 参数 | `--status-hook` 必需；`--provider <name>` 或 `--provider=<name>`（也接受 `--status-hook <name>` 的位置写法）。值大小写不敏感，认哪些写法见 §8 第 1 步 |
| 输入 | 一次性 `read_to_end` 读满 stdin，**上限 4 MiB**（`MAX_HOOK_INPUT`）。超限或非法 JSON → 失败。⚠️ **别把它调回 64 KiB**：Codex 的工具事件会把 `tool_input`（`apply_patch` 的补丁、长命令、MCP 调用的参数）原样带上，轻松超过 64 KiB，卡在那里等于把**有效生命周期事件整个丢掉**（表现为灯卡住不动）。读的是管道、只解析一次，4 MiB 对短命进程没有压力；真正落盘的 inbox 事件仍**只带提取后的字段**，还是 KB 级 |
| 退出码 | 成功 `0` / 失败 `1`，并向 stderr 打一行 `UFO status hook failed: <原因>`。**没有重试与兜底队列**，失败就是丢了这一个事件 |
| 状态目录 | `--status-dir <dir>` > 环境变量 `UFO_STATUS_DIR` > 默认 `%APPDATA%\<identifier>\status`（macOS/Linux 走各自 XDG/AppSupport 口径） |
| 建议 | 事件配置带 `async: true` 与短 `timeout`（如 3 秒）。UFO 侧不自己设超时，超时由平台侧管 |

⚠️ **这个进程必须快进快出**：每个事件都要起一次，它只做「读 stdin → 归一化 → 写一个小文件」。因此 UFO 绝不在 hook 里解析 transcript（token 由常驻 monitor 增量统计，见 `model.rs` 里 `transcript_path` 的字段文档）—— 平台侧也不必为它准备额外环境。

✅ **合并而不是覆盖**：往平台的 hook 配置里加条目时不要整份覆写用户已有配置。

📌 hook 那一侧的排查记录就是**转灯日志**：写在与 inbox 同级的 `log/` 目录里，**跟着 `--status-dir` 一起走**。旧版那条硬写默认目录的 `hook-debug.log` 已经废掉 —— 正是因为它与 inbox 分家，才出现过「日志说有事件、目录里却没有」这种排查黑洞。⚠️ 两个开关默认都是关的：设置里的记录级别（`log.level`，`off` = 整块不记）与 hook 侧那一行（`log.hook_enabled`）。都开着才有「hook 到底有没有被调用过」这个证据；关着就什么都不落盘，唯一剩下的证据是 stderr 那行 `UFO status hook failed: …` 与退出码 1。另：开关本身从默认 AppData 位置的设置文件读，**不**跟 `--status-dir`。

## 2. payload 最少要给什么

**最小契约 = 一个会话 id + 一个事件名。** 其余全可缺省，UFO 按「取不到就不显示」退化。

```json
{ "session_id": "5f0d…", "hook_event_name": "Stop" }
```

### 表 T1 · payload 字段

别名按**查找顺序**排列：第一个命中的非空字符串生效。

| 字段 | 必填 | 别名 | 约束与坑 |
|---|---|---|---|
| 会话标识 | ✅ | claude 侧 `session_id` / `sessionId` / `conversation_id`；codex 侧 `session_id` / `sessionId` / `thread_id` | **非空且 ≤ 256 字符**，否则整个事件被拒。缺失 = 直接报错退出 1。⚠️ 别名表由各适配器自己写，**不是全局的** —— 新平台用自己习惯的字段名要在适配器里登记 |
| 事件名 | ✅ | `hook_event_name` / `event` / `event_name` / `event_type` / `type` | 取不到时按 `"unknown"` 处理 → 多半是 `Unknown`（不转灯）。所以**宁可不发这条事件，也别发一条没有名字的事件** |
| `notification_type` | 可选 | `notification_type` / `notificationType` | 只在「一个事件名承载多种语义」时用到（Notification）。**仅供诊断**，灯色在兼容层就已定完 |
| `prompt` | 可选 | `prompt` | **只在「用户提交提问」那一刻采**；trim + 截断 4000 字符。⚠️ 明文落盘（见 §5） |
| `cwd` | 可选 | `cwd` | 项目级配置（endpoint 来源）以它为基准；不带则只读用户级配置 |
| `transcript_path` | 可选 | `transcript_path` | 只透传路径，≤ 4096；token 由常驻侧解析。**hook 自己绝不解析它** |
| `agent_id` | 子代理必需 | `agent_id` / `agentId` | ≤ 256。主线程事件**不要带**。⚠️ 漏带会让子代理的活动把主线程的审批黄灯顶回绿（§3 表末行） |
| `event_id` | 可选 | `event_id` / `eventId` / `id` | ≤ 256，缺省由 UFO 生成。⚠️ 若平台自己传，**必须每事件唯一** —— 去重键是 `provider:session_id:event_id`，固定 id 会让灯只亮一次 |
| `pid` | 可选 | `pid` / `process_id`（或环境变量 `UFO_AGENT_PID`） | 读不到整数就当没有。给了就能在进程死亡时立刻收灯，否则等「无响应超时」（旧名「执行中超时」） |
| `sequence` | 可选 | `sequence` / `occurred_at_ms`（`timestamp_ms`） | 给了可做乱序过滤；不给只按到达顺序 |
| `error_code` | 可选 | `error_code` / `error` / `failure_reason` | **有值即判失败**（优先于事件名），转红灯 |
| `outcome` | 可选 | `outcome` / `reason` / `status` / `stop_reason` | 值为 `error` / `failed` / `failure` / `cancelled` / `denied` 之一时**判失败**；其余值只作诊断（如 SessionEnd 的 reason） |

`base_url`（endpoint）**不从 payload 取** —— 见 §4。

## 3. 事件 → kind → 灯色

UFO 内核只认 8 种语义（`AgentEventKind`，`status/model.rs`）。平台侧要做的全部事情，就是把自己的事件名映射到这 8 种上。

灯态与「是否绿闪」**不是查表查出来的**：`status/kernel.rs` 的 `reduce_state(previous, event, subagents)` 推出内部态 `SessionState`（七个），再经 `SessionState::to_phase` 投影成对外的五个阶段、`resolve_timing` 决定绿闪时长与移除时刻。**没有 `light_for_kind` 这个函数**，也不要造一个 —— 一旦存在「kind → 灯」的直查表，前态与子代理集合就没地方参与了（见 §11 那条「原始事件名仅供诊断」）。

事件名先归一化再过表：大小写、`-`、`.`、`:` 的差异统一成 snake_case（`normalize_event_name`），所以 `PreToolUse` / `pre-tool-use` / `pre.tool.use` 是同一个。表没覆盖的名字走保守兜底（`fallback_kind`）：含 `error`/`fail` → Failed，含 `start`/`tool` → Running，含 `stop`/`wait` → WaitingForUser，否则 Unknown。

### 表 T2 · 语义 → 灯（示例名取 Claude Code 的实际事件名）

| 你的事件语义 | 归一化 kind | 灯的表现 | 必需性 |
|---|---|---|---|
| 会话启动 / 提交提问 / 工具调用前后 / 任何「在干活」的信号 | `Running` | 🟢 常亮 | ✅ **必须**，至少有一个 |
| 一轮回复跑完、等你输入 | `TurnFinished` | 🟢 先闪烁，保留期（默认 30s）后落 🟡 常亮 | 推荐（不给就永远没有「该你了」） |
| 弹权限框 / 追问 / 任何等人操作 | `WaitingForUser` | 🟡 常亮、**不闪** | 推荐（见下「缺授权类怎么退化」） |
| 对话彻底结束 | `SessionFinished` | 🟢 闪烁后**移除这盏灯** | 推荐 |
| 本轮 API/模型出错 | `Failed` | 🔴（红灯不粘：任何能映射出别的语义的事件都可转回来） | 可选（可用 `error_code` 代替，见 T1） |
| 子代理开始 / 结束 | `SubagentStarted` / `SubagentStopped` | 见下 | 可选（无子代理概念就别发） |
| 认不出来 | `Unknown` | **保持原灯色**并继承计时 | —— |

**「等你授权」期间子代理还在后台跑**是灯最容易搞错的一种局面：

- ⚠️ 子代理事件与主会话**共用同一个 `session_id`**，只有 `agent_id` 能区分。内核靠 `agent_id` 维护「活跃子代理集合」，规则是：**主线程正在等你操作、或会话已结束 / 已出错时，子代理事件只记账、绝不转灯**；只有主线程已停、处于「等 prompt」的空闲态时，子代理活动才把灯拉回「后台运行」（对外仍是绿常亮），最后一个子代理收尾后落回黄。
- 带 `agent_id` 的 `Failed` 只代表那个子代理失败，**不把父会话染红**。
- 没实现子代理的平台不用管这些 —— 不声明 `agent_id` 就等于活跃集合恒空，主线程语义完全不受影响。

**缺错误类怎么退化**：不必专门有「错误事件」。任何事件带 `error_code`，或 `outcome` 落在失败类取值上，都会被短路判成 Failed。所以「没有错误事件名」就把错误放进 `error_code` 字段。

**缺授权类怎么退化**：黄灯永远不出现，卡在绿直到「无响应超时」（旧名「执行中超时」，默认 60s）被收掉 —— 这正是 Codex 今天的退化档位（它的 rollout 里根本没有「等你授权」这个状态），属合法降级，不是 bug。

📌 灯色的完整内部状态机（`SessionState` 七个内部态：Running / WaitingForPrompt / WaitingForUser / BackgroundRunning / Completed / Failed / Unknown）见 `../agent-status-state-machine.html`；已接入平台的实测事件清单见 `../rule/agent-status-hooks.md`。

## 4. UFO 取不到的状态

诚实清单。**没有的字段宁可不给，也别乱映射**（乱给会污染状态判定）：

| 状态 | hook 路径 | 日志采集路径 | UFO 的退化行为 |
|---|---|---|---|
| **Endpoint（`base_url`）** | ❌ 不从 payload 取 | ❌ 同左 | 从**平台自己的配置文件**读（Claude 读 `settings.json` 的 `env.ANTHROPIC_BASE_URL`，Codex 读 `config.toml` 的 `model_provider → base_url`），环境变量兜底。取不到就不显示 Endpoint 行 |
| 上游真实地址 | 可选 | 可选 | 经「代理解析器」反查（cc-switch 等），见 §8 第 7* 步 |
| 模型 / 用量明细 | ❌ | 部分 | token 由 UFO 侧解析 transcript / 日志得到，平台不用报 |
| `waiting_reason`（等人操作的原文理由） | 可选 | ❌ | 只有 payload 里带（如 Codex 审批理由）才显示；⚠️ **不要塞进 `prompt`**，那会覆盖用户真正的提问 |

⚠️ 只读 endpoint 类的那一个 key。**token / api key 一律不读取、不上报、不缓存** —— 这是 UFO 侧的硬约束，平台侧配置里带了密钥也不会被碰。

## 5. 线协议与限度

想绕过 hook 直接投递可以，但要明白代价：inbox 里的文件就是一个 `AgentEvent` 序列化结果（snake_case，共 18 个键，缺省字段序列化成 `null`），字段清单见 `status/model.rs` 的结构体文档。

- `schema_version` 必须为 `1`，内核**硬校验**（`kernel.rs`）。不匹配的事件被直接丢弃。
- ⚠️ 手写 JSON 要精确匹配全部键；任何解析失败会被移进 `quarantine/` 并把事件源**永久置为 degraded**（没有自动恢复路径，只在主窗对应面板可见）。
- ✅ 因此：**要么走 `--status-hook`，要么在 UFO 里新增一个采集源**，不要手写 inbox。

演进规则（对 UFO 侧开发者，见 §11）：新字段一律 `Option` + `#[serde(default)]`，**永不递增 `SCHEMA_VERSION`** —— 升版本号会让新旧二进制互相丢事件，且失败是静默的。

✅ 现成的正面例子：`AgentEvent.kind` 就是按这条加进来的（`#[serde(default)]`，缺省 `Unknown`）。旧 hook 写的事件反序列化后拿到 `Unknown` = **不驱灯色**，语义与「这条我认不出来」完全一致，因此新旧互相兼容；反过来，**不许**为了「让 kind 必填」而去动版本号。

## 6. 自测接线

平台侧只需要一条命令：手动喂 payload，看灯。GUI 必须在运行（消费事件的是常驻 monitor）。

下面这条路径是 **debug 产物**，仍叫 `tauri-app.exe`（`tauri build` 才会按 `mainBinaryName` 改名成 `UFO.exe`，`tauri dev` 不改名），所以自测一律写绝对路径。

```powershell
$exe = "E:\workspace\AIProjects\ufo\src-tauri\target\debug\tauri-app.exe"

'{"session_id":"e2e-1","hook_event_name":"SessionStart"}' | & $exe --status-hook --provider claude
# → 悬浮窗约 500ms 内出绿灯（不出灯就按 §1 末尾那条 📌 取证：转灯日志里有没有 hook 那一行、inbox 有没有过文件）
'{"session_id":"e2e-1","hook_event_name":"Stop"}'         | & $exe --status-hook --provider claude
# → 先绿闪，30 秒后落黄常亮
'{"session_id":"e2e-1","hook_event_name":"SessionEnd"}'   | & $exe --status-hook --provider claude
# → 绿闪后这盏灯消失
```

装过安装包（MSI 或 setup.exe）之后，同一条命令可以直接用裸名字，这也是文档里 hook 配置的正式写法：

```powershell
'{"session_id":"e2e-1","hook_event_name":"SessionStart"}' | ufo --status-hook --provider claude
```

⚠️ 裸 `ufo` 只在**新开的**宿主进程里存在 —— PATH 是安装器写进 `HKCU\Environment` 的，已经在跑的 agent 进程读不到（见 `../rule/agent-status-hooks.md`「安装前提」）。

想不污染真实状态目录，加 `--status-dir <临时目录>`；inbox 与转灯日志都跟着走（只有「开关本身」仍从默认 AppData 的设置文件读，见 §1 末尾的 📌）。

反例自查：`--provider <还没登记进 UFO 的名字>` 期望 **exit 1**，并把传入值原样回显成 `unknown --provider "qcoder"`（报错不列举平台清单 —— 那份清单每接一个平台就会失真一次）。这说明 `Provider` 是闭包枚举 —— 新平台必须先按 §8 登记，光配 hook 是接不进来的。

---

# Part 2 · UFO 侧：接入一个新平台的真实改动清单

## 7. 全景：两条事件源路径

```
平台 A（有 hook）      ufo --status-hook --provider x   ← 短命进程，快进快出
                            │  stdin JSON → compat/<x>.rs 归一化 → 原子写 inbox/*.json
                            ▼
                    monitor 每 500ms 消费 inbox ─┐
                                                ├─▸ apply_status_event ─▸ kernel（reduce_state）
平台 B（只有日志文件） 常驻巡检器（第二事件源）  ─┘        │
   增量读 <平台>/sessions/**，自己合成 AgentEvent          ├─▸ 会话存档 SQLite（跨重启续档）
                                                         └─▸ 广播 agent-status 快照 → 三窗口
```

两条路产出的都是同一个 `AgentEvent`，**必须走同一个入口** `apply_status_event`（`status/mod.rs`）：它负责广播调试事件、登记 transcript、取存档回填、进内核。绕开它 = 少一条链路。

选路判据：**平台的事件能覆盖生命周期吗？** Codex 的教训（实测）是三条 —— 没有错误类事件（429/502 不触发任何 hook）、中断路径不触发、且 hook 会**静默失效**（一段时间一次都不调用、四处日志无报错）。任一条命中就走路径 B，hook 只留极窄的补漏口子。

## 8. 路径 A：有 hook 能力的平台

表 T3 按**编译错误暴露的顺序**排列 —— 前一步的报错会正好点名下一步，照着走不会漏。

### 表 T3 · 改动清单

| # | 位置 | 改什么 | 不改的后果 |
|---|---|---|---|
| 1 | `status/model.rs` 的 `enum Provider` + `parse` + `as_str` | 加一个 variant、别名写法、稳定字符串 | hook 入口就拒绝：exit 1 并提示 `missing --provider …`，事件根本进不来 |
| 2 | 新建 `status/compat/<platform>.rs` | 实现 `HookAdapter::adapt`（把 payload 归一化成 `AdaptedEvent`），含事件名表、session_id 别名、提问提取、`agent_id` 判定 | 没有适配器 |
| 3 | `status/compat/mod.rs` 的 `adapter_for` | 加一个 match 分支 | **编译失败**（穷尽 match） |
| 4 | `status/compat/mod.rs` 的 `base_url_env_names` | 该平台的环境变量名 | 环境变量兜底失效，endpoint 恒空 |
| 5 | `status/compat/claude.rs` 的 `provider_base_url` | 该平台的配置文件读取（哪个文件、哪个 key） | **编译失败** |
| 6 | `proxy/mod.rs` 的 `AgentApp::from_provider` | 加一路映射 | **编译失败** |
| 7\* | `proxy/ccswitch.rs` 的 `app_key` 与 `upstream_from_settings` | **仅当**该平台也被本地代理接管：两处各加一路，且必须复用同一份解析规则（`base_url_from_config_text`） | tooltip 停在 `127.0.0.1:<port>`，看不到真实上游 |
| 8 | `src/modules/agent-status/lib/provider-registry.ts` | 前端展示名 + 深 / 浅两档图标，加一行 | 图标缺失、退化成首字符徽标 —— **灯仍然正常亮**（§10） |

内核（`kernel.rs`）**零改动**：它只认 kind，不认平台词汇。

`HookAdapter` 的契约只有一个方法：

```rust
fn adapt(&self, payload: &Value, ctx: &AdaptContext<'_>) -> Result<Option<AdaptedEvent>, String>;
```

- `Ok(Some(..))` = 归一化成功；`Ok(None)` = **静默忽略**（不落盘、不算失败、退出码仍 0）。只关心少数事件的平台（如 Codex 只要 `PermissionRequest`）就靠这个把其余事件吃掉，避免多起进程空转。
- `Err(..)` = 真失败：exit 1 + stderr 一行 `UFO status hook failed: …`；开着日志时（见 §1 末尾的 📌）还会在转灯日志里留下一行判定（如 `PayloadNotJson`）。
- 信封字段（`event_id` / `pid` / `received_at_ms` / `schema_version`）由 hook 外壳 `into_event` 统一补，适配器**不要**自己拼。

⚠️ **release 构建是 `panic = "abort"`**：适配器里不得 panic（不 `unwrap` / `expect` / 裸索引 / 字节切片）。按长度截字符串要用 `chars()` 而不是 `&s[..n]`（后者遇多字节字符直接 panic，会带走整个 App）。失败一律返回 `Err(String)`。

⚠️ **不要把两条路径同时接到同一会话上**：hook 与日志采集器同时驱动同一盏灯会让灯态互顶。若两条都有，必须像 Codex 那样把 hook 收窄成一个补漏口子，并把「哪个源负责什么」写成测试锁住。

### 兼容层的文件边界：三个适配器**互不引用**

`status/compat/` 对外只有两样东西：`HookAdapter` trait 与 `adapter_for(provider)` 注册表（表 T3 的第 3 步）。除此之外，目录内的分工是硬约定：

- `claude.rs` / `codex.rs` / `qoder.rs` **互不引用**，各自独占自己平台的全部知识：字段别名表、事件名表、`base_url` 来源、提问的提取与裁剪、子代理 `agent_id` 的判定。⚠️ **共用一张表 = 改一个平台会静默带偏另一个平台的灯** —— 三家的事件名与注入模板恰好长得都不一样，这种回归一旦种下极难复现。
- `compat/mod.rs` 只放**与平台无关**的通用工具：多别名取值、事件名归一化（`normalize_event_name`）、未知事件兜底（`fallback_kind`）、错误短路判定（`error_code` / `outcome`）、裁剪额度、`agent_id` / `transcript_path` 提取，以及下面那个提问标签引擎（`apply_tag_rules` / `build_tag_rules`）。
- 一段代码该写在哪儿，只问一句：**它知不知道自己是哪个平台？** 知道就留在 `<platform>.rs`，不知道才配进 `mod.rs`。

### 提问标签引擎：**机制共用、策略各留各的**

平台会往 `prompt` 里塞自己的模板，而两家的**修法是相反的**，所以规则表必须一平台一份：

| 平台 | 动作 | 为什么 |
|---|---|---|
| Claude | `drop` —— 删标签连同内文（`<system-reminder>` / `<ide_opened_file>` 等） | 那些是平台注入的上下文，用户一个字都没说过 |
| Qoder | `extract` —— **只留**标签内的正文（`<untrusted_objective>`） | `/goal` 那一轮整段被换成 3282 字的 harness 模板，用户真正的目标只在标签里；按 `drop` 处理会连着正文一起删掉 |

⚠️ **方向拿错等于把用户的正文一起丢**（该 `drop` 的配成 `extract`，或反过来），症状是「提问从此不再更新」这种最难查的静默失效。接新平台时先抓几条真实 `UserPromptSubmit` payload 再决定方向，别照抄别家。

- **扫描与边界只有一份**（`compat/mod.rs` 的 `apply_tag_rules`）：成对标签、自闭合、未闭合、多字节安全、同名前缀不误命中 —— 这些不认识平台，也**不该**在各适配器里重复实现。
- **规则表与配置一平台一份**：`claude.rs::tag_rules()` 读 `UFO_TAG_RULES_CLAUDE`（默认清一色 `drop`），`qoder.rs::tag_rules()` 读 `UFO_TAG_RULES_QODER`（默认 `untrusted_objective:extract`）。语法是逗号分隔的 `name` / `name:drop` / `name:extract`（`name` 不带动作默认 `drop`，会剥掉误写的尖括号；非法项**整项丢弃**、不影响其它项）；值为空或整表都非法时回落到**该文件自己的** `DEFAULT_TAG_RULES`。改一家动不到另一家的行为。
- ⚠️ 两个键都由 `src-tauri/build.rs` **构建期**注入（`option_env!` → `cargo:rustc-env`）：hook 进程的 cwd 是被监测 agent 的项目目录，运行时读不到仓库的 `.env`，所以**改完 `.env` 必须重新构建**才生效。
- ⚠️ Qoder 那条 `extract` 规则额外带一道**命中门槛**（`TagRule.when_prefix`）：「文本以 `A goal has been set.` 开头才生效」。门槛写在 `compat/qoder.rs` 里而**写不进环境变量** —— 环境变量只表达得出 `name:action`，表达不出条件。没有门槛，普通提问会被判成「无正文」，提问从此不再更新。

payload 实录、门槛落空后的两层回落与「哪一轮被裁剪了多少字」见 `../rule/agent-status-hooks.md` 的 Claude「prompt 的来源」与 Qoder 两节。

## 9. 路径 B：只有日志文件的平台

范本是 `status/codex_rollout.rs`。照它的五段来，不要自创：

| 环节 | 做法 | 为什么 |
|---|---|---|
| **发现** | 扫平台会话目录（含**回溯 N 天**），登记没见过的新文件；会话 id 从文件名取 | 跨零点仍在跑的会话落在前一天目录 |
| **播种** | 注册时读一遍**已有内容**，只为拿 `cwd`（→ endpoint）、累计用量、当前灯态基线；**绝不为历史产出事件** | 否则 UFO 一启动就把当天所有历史会话复读成一串灯色动画 |
| **增量** | 只读新增字节（`status/lines.rs` 的 `LineCursor`）；先做**子串预筛**再解析 JSON | 日志里 90%+ 的字节是命令输出/正文，逐行 JSON 解析会把常驻线程吃满 |
| **去抖** | 连续同灯态压成一条；但**一批里灯态真的变了要各产出一条**（按文件里的先后顺序）。「绿」每 30s 补一条保活 | 只留最后一条会吞掉「恢复执行 → 一轮结束」的完成信号；不续命会被「无响应超时」清掉 |
| **结束** | 归档 = 文件被**移走**；连续两轮读不到即判 `SessionFinished` | —— |

接线三条纪律：

1. ⚠️ **每轮处理顺序是「采集源先、inbox 后」**，别调回去。hook 事件代表更晚发生的事实（弹审批框之前日志已经写过工具调用记录），放后手它才能压过由「文件在长」推出的绿；反过来会让刚抬起的黄灯被同一批旧字节顶回绿。
2. ⚠️ 若该平台**同时**有 hook 口子，hook 事件过完后要回灌去抖基线（巡检器的 `note_light`）。否则 hook 把灯抬成黄的这一刻巡检器并不知道，批准后恢复写入的那条「绿」会被当成重复事件整个吃掉，灯卡在黄直到 30 秒保活。这条回灌写在 `apply_status_event` 里、由它的 `source: EventSource`（`Hook` / `Rollout`）参数判定 —— **只有来源是 hook 的事件**才回灌，采集源自己产出的事件已经带着基线走。Codex 的实测账（含 `baseline_reflow` 那行日志）见 `../rule/agent-status-hooks.md`。
3. 每轮结束要 `retain` 清账（会话消失就丢游标），否则注册表只增不减。

新平台的巡检器同样只在 GUI 侧跑 —— **hook 二进制绝不打开**存档 SQLite 或日志（必须快进快出）。

## 10. 前端 provider 注册表

`src/modules/agent-status/lib/provider-registry.ts` 是前端唯一的 provider 知识来源：

```ts
const PROVIDER_REGISTRY: Record<string, ProviderMeta> = {
  claude: { label: 'Claude', icon: claudeIcon, iconLight: claudeIcon },
  codex:  { label: 'Codex',  icon: codexIcon,  iconLight: codexIconLight },
}
```

| 字段 | 必填 | 说明 | 约束与坑 |
|---|---|---|---|
| `label` | ✅ | 可见名称与读屏文案 | 未登记时回落成**原始 provider 串** —— 刻意不猜，冒充 Claude 比不显示更糟 |
| `icon` | 可 `null` | 深色档 svg | `null` 时渲染 `providerFallbackMark()` 的首字符徽标（按码点取，代理对不会被切坏） |
| `iconLight` | 可 `null` | 浅色档 svg（深色墨版本） | ⚠️ **不再用 `invert(1)` 反相**：反相只对纯白单色成立，Qoder 的白 + 品牌绿会被翻成品红。彩色 mark 两档通用时直接复用同一份资源即可 |

渲染侧一律走 `providerIcon(meta, theme.isDark)` 取资源（灯位、提示窗、会话记录三处同源），不要按 provider 名现查、也不要再给 `<img>` 加 `.ufo-mono-icon`。

🧪 护栏：`provider-registry.spec.ts` 会**读 Rust 源码**解析出 `enum Provider` 的变体清单，断言注册表与之逐个对应，并断言每个已登记平台**两档图标都不为空**。Rust 加了 variant 而忘了前端登记 → CI 红。

⚠️ **快照校验器不按 provider 收窄**（这是本轮整改的本体）：`isStatusSnapshot` 只验「非空、≤ 64 字符」，未知平台**照样进快照**，展示交给注册表降级。早先这里是 `claude | codex` 白名单 —— 它会让新平台的**每一帧**被整帧丢弃，而首帧走 `invoke`（不经此校验），于是症状变成「灯冻结在首帧、永不更新」，比全盲难查得多。同一取舍也用在 `isSessionRecordList`（否则一条陌生存档会让整张记录面板空掉）。

✅ `schema_version === 1` 这道校验**保留**：它是刻意的前后端同步 tripwire，与 provider 白名单性质不同。

## 11. 协议演进红线

| 做法 | 后果 |
|---|---|
| 给 `AgentEvent` / `SessionSnapshot` 加字段时升 `SCHEMA_VERSION` | ❌ 新旧二进制互相丢事件，且**静默**。✅ 一律 `Option` + `#[serde(default)]`，版本号恒为 1 |
| 给存档表加列时动线协议版本号 | ❌ 两套版本别混：线协议 = `schema_version`（永不递增）；库内布局 = `PRAGMA user_version`（当前 2，加列时递增它配 `ALTER TABLE`） |
| 新增 `SessionPhase`（投影给前端的阶段） | 要同步三处：`lib/agent-status.ts` 的 `SessionPhase` 联合类型 + `colorForSession` / `phaseLabel` / `colorLabel` + 内核快照 `counts` 分支。**新增 kind 不需要** —— 只进适配器的表 |
| 让 `AgentEvent.event`（平台原始事件名）或 `notification_type` 参与转灯 | ❌ 平台词汇会漏回内核层。二者**仅供诊断** |
| 用 `window.alert` / `confirm` 做面板反馈 | ❌ 装了 `tauri-plugin-prevent-default` 会被拦；一律走 `role="status"` 行内提示 |

✅ 只有引入**新的 IPC 命令**时才需要动 capability（新命令注册三处：`#[tauri::command]` + `permissions/status.toml` + `capabilities/*.json`）。单纯加一个平台的事件源**不需要**。灯位排序（按 `started_at_ms` 升序）、音效触发（只看 phase / is_flashing）、流程脚本与端点档案（按 endpoint 存）都与 provider 无关，同样零改动。

## 12. 验收清单

**自动化**

- [ ] `npx vitest run` 全绿（新增平台的注册表条目应被 `provider-registry.spec.ts` 的同源断言覆盖）
- [ ] `npm run build`（`vue-tsc --noEmit` + vite）过 —— 类型放宽后不应有回归
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` 过；新平台的适配器要带 `#[cfg(test)]` 事件名表测试（每个事件名 → 期望 kind），以及一条「认不出的 payload 返回 `Ok(None)` 而不是 panic」
- [ ] 若该平台同时有 hook 与日志采集：一条测试锁住「哪个源负责哪些事件」，防止两条路互顶

**人工（真机）**

- [ ] 按 §6 三条命令走一遍：绿 → 绿闪落黄 → 灯消失
- [ ] 悬停信息卡：provider 名与图标正确、Endpoint 行按 §4 口径显示、`--provider <未登记>` 确实 exit 1
- [ ] 主窗「会话记录」面板能看到该平台的会话（存档主键是 `provider:session_id`，自动进）
- [ ] 拖一格大小（32–96）看新平台徽标不溢出：写死的像素必须乘 `--floating-cell-scale`
- [ ] 若走路径 B：agent 全停时**零解析零事件**（日志不再增长）
- [ ] 主线程弹审批框（黄）时，后台子代理仍在写入 → 灯**必须停在黄**，不被顶回绿

**落地检查**（应当无命中）

```
rg -n "provider === 'claude'|provider === 'codex'" src/modules/agent-status
rg -n "light_for_kind" src-tauri/src
```

---

## 附录：常量与不变量速查

| 项 | 值 | 定义处 |
|---|---|---|
| hook stdin 上限 | 4 MiB | `status/hook.rs` `MAX_HOOK_INPUT` |
| 事件线协议版本 | 恒为 `1`，永不递增 | `status/model.rs` `SCHEMA_VERSION` |
| 存档库内布局版本 | `2`（加列才动） | `session_store.rs` `PRAGMA user_version` |
| session id / agent_id / event_id 上限 | 256 字符 | `compat/*` |
| prompt 上限 | 4000 字符（trim + 按字符截断） | `MAX_PROMPT_CHARS` |
| base_url / transcript_path 上限 | 4096 字符 | `MAX_BASE_URL_LEN` 等 |
| inbox 消费周期 | 500 ms | `status/mod.rs` monitor 循环 |
| 一轮完成保留期（绿闪→落黄 / 绿闪→移除） | 默认 30 s（可配 1s–24h） | `settings.rs` `DEFAULT_RETENTION_MS` |
| 无响应超时（无事件则收灯；旧名「执行中超时」，配置字段仍是 `running_timeout_ms`） | 默认 60 s（可配） | `settings.rs` `DEFAULT_RUNNING_TIMEOUT_MS` |
| 日志采集「绿」的保活间隔 | 30 s | `codex_rollout.rs` `RUNNING_KEEPALIVE` |
| 判会话结束的连续读不到轮数 | 2 | `codex_rollout.rs` `MISSING_POLLS_BEFORE_END` |
| 会话存档上限 | 200 条 + 启动删 `last_seen` 超 30 天 | `session_store.rs` |
| 事件去重键 | `provider:session_id:event_id` | `kernel.rs` |
| 灯位排序 | `started_at_ms` 升序，同刻按 key 兜底 | `kernel.rs` |

**相关文档**（路径相对本文件所在的 `reference/doc/guide/`）：

- `../rule/agent-status-hooks.md` —— 已接入平台（Claude / Codex / Qoder）的 payload 实录、Codex 的 rollout 读法、指示灯语义与子代理红线
- `../rule/agent-status-session-data.md` —— token / `base_url` / `prompt` 的采集链路、代理解析器、会话存档
- `../rule/agent-status-endpoint-flow.md` —— 信息卡拓展与 QuickJS 流程脚本沙盒
- `../rule/visual-style.md` —— 视觉 token 与主窗/悬浮窗版面约定
- `agent-status-log-guide.md` —— 转灯日志（每一次裁决为什么落这一行）
- `agent-status-push-guide.md` —— 灯态变化的消息推送
- `floating-module-guide.md` —— 悬浮窗组件（模块）开发规范
- `../agent-status-state-machine.html` —— 状态机全景（由 `/draw` 命令生成）
- `AGENTS.md`（仓库根）—— 只剩稳定规则与指向以上文档的链接，**不承载本篇的细则**
