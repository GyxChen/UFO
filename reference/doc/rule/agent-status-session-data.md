# Agent 状态灯 · 会话数据的采集与落盘

> **管什么**：悬浮窗与提示窗上「除灯色以外」的那几行会话数据 —— token 用量、Endpoint（`base_url`）、用户提问（`prompt`），以及让它们**跨重启仍然存在**的那份会话存档 `session-records.db`。代码在 `src-tauri/src/modules/agent_status/status/tokens.rs`、`status/kernel.rs`、`session_store.rs` 与前端 `src/modules/agent-status/{lib,stores}/`。
> **不管什么**：灯色怎么转（兼容层 + 内核的裁决）、平台 payload 里字段叫什么名字、日志与推送这两个外发出口。
> **一句话版本**：hook 只给「路径 + 原文」，一切解析、统计、落盘都在常驻 monitor；凡「跨重启仍有意义」的字段进 SQLite，回填阶梯固定为**事件 > 内存前驱 > 存档**。
> **与其它文档的关系**：
> - `../rule/agent-status-hooks.md` —— `base_url` / `prompt` 在 **payload 与平台配置文件里怎么取**、标签引擎、事件名表、各平台差异，**权威写法在那边**，本篇只讲这些值进入 `AgentEvent` 之后的事（§3）。
> - `../guide/agent-status-platform-onboarding-guide.md`（下称**接入指南**）—— 协议演进红线（§11）、线协议与限度（§5）、路径 B 采集器纪律（§9）、常量速查（附录）。本篇**不重述**这些红线本身，只点名它们落在会话数据的哪一处。
> - `../guide/agent-status-log-guide.md` —— 转灯日志。它与本篇的边界是一条硬红线：**关掉日志不得影响本篇的任何字段**（那边 §10 有命名测试锁住）。
> - `../guide/agent-status-push-guide.md` —— 推送正文取 `prompt` / `endpoint` 的口径与勾选风险（那边 §3）。
> - `../guide/floating-module-guide.md` —— 组件/外壳边界、线程与 `async` 命令纪律（§12.3）、命令注册三处（§12.5）、`activate` 里等配置（§11）。
> - Endpoint 的反查与按 endpoint 的拓展字段（`upstream_url` 从哪来、`extensions`）见 `agent-status-endpoint-flow.md`。
> - 设置页版面（二级分区、高度链、两级 tabbar、`fill`）见 `visual-style.md`，本篇不涉及。

## 目录

1. [全景：一条会话的数据从哪来、到哪去](#1-全景一条会话的数据从哪来到哪去)
2. [token 用量：谁解析、怎么去重、怎么展示](#2-token-用量谁解析怎么去重怎么展示)
3. [展示字段 `base_url` / `prompt` 的采集口径](#3-展示字段-base_url--prompt-的采集口径)
4. [会话记录持久化（SQLite，跨重启续档）](#4-会话记录持久化sqlite跨重启续档)
5. [命令与授权](#5-命令与授权)
6. [敏感数据与已知代价](#6-敏感数据与已知代价)
7. [红线汇总](#7-红线汇总)
8. [验收清单](#8-验收清单)
- [附录：常量与落点速查](#附录常量与落点速查)

---

## 1. 全景：一条会话的数据从哪来、到哪去

```
平台 hook 进程（短命，快进快出）
   stdin JSON → compat/<平台>.rs 归一化 → 原子写 inbox/*.json
   带过来的只有：会话 id / 事件名 / cwd / transcript_path / prompt 原文
                                    │
monitor 每 500ms 一轮 ──────────────┤
   ① 消费 inbox → apply_status_event（`status/mod.rs`，唯一入口）
   ② TokenTracker 增量解析 transcript（§2）→ StatusRegistry::set_tokens
   ③ 代理解析器反查上游 → SessionSnapshot.upstream_url（见 endpoint-flow 篇）
   ④ 会话存档 SQLite 刷盘 + 命中存档时回填（§4）
   ⑤ 广播 agent-status 快照 → 三窗口
                                    │
前端 stores/agentStatus.ts → 悬浮窗灯 / 提示窗信息卡 / 主窗「会话记录」面板
```

| 字段 | 采集处 | 解析或加工处 | 落盘点 | 展示处 |
|---|---|---|---|---|
| `prompt` | hook payload（仅「用户提交提问」那一刻） | 兼容层标签引擎 + 截断（权威：hooks 篇「prompt 的来源」） | `AgentEvent` → `SessionSnapshot` → `session-records.db` | 信息卡末行、主窗详情、推送（需勾选） |
| `base_url` | 平台自己的配置文件 / 环境变量（**不来自 payload**） | 兼容层按平台读配置（权威：hooks 篇「base_url 的来源」） | 同上 | 信息卡 Endpoint 行（与 `upstream_url` 合流） |
| `upstream_url` | 代理解析器反查（endpoint-flow 篇 §2） | monitor 每轮现算 | 随快照落库，**但不是回填字段**（§4） | 同上 |
| `tokens` | 只透传 `transcript_path` | `TokenTracker` 增量扫（§2） | `SessionSnapshot.tokens`，**不是事件派生字段** | 信息卡 token 行、主窗详情 |
| `started_at_ms` | 首个事件的时间 | 内核按它排灯位 | 存档回填（§4） | 灯位顺序、详情「开始于」 |

**共同约束**：以上字段全是后加的，一律 `Option` + `#[serde(default)]`，**不得升 `SCHEMA_VERSION`**（升版本会让新旧二进制互相丢事件且**静默**，见接入指南 §11）。落点分家的原则也只有一条：灯态归内存与事件，「跨重启仍有意义」的才进存档。

## 2. token 用量：谁解析、怎么去重、怎么展示

### 2.1 链路

- hook **只透传 `transcript_path`**（≤ 4096 字符）→ 常驻 monitor 线程用 `TokenTracker`（`status/tokens.rs`）**增量**解析 transcript → `StatusRegistry::set_tokens`（`status/kernel.rs`）→ `SessionSnapshot.tokens`。
- ⚠️ **hook 进程绝不解析 transcript**：它每次事件都要起一份，必须快进快出。这条是接入指南 §1/§2 的既定红线，不在这里重复理由。
- `SessionSnapshot.tokens` 是 `Option<TokenTotals>` + `#[serde(default)]`；它**不是事件派生字段**，由 `snapshot()` 时合并进快照 —— 因此「有没有 token」不取决于事件带没带，而取决于 monitor 扫到没有。

### 2.2 去重与并入

| 规则 | 原因 |
|---|---|
| 按 `message.id` 去重（同一 id 只计一次 usage） | 同一条 API 响应会写成**多条** assistant 记录，每条都带同一份 usage。不去重会翻 2–3 倍。护栏：`tokens.rs` 的 `deduplicates_repeated_message_ids`（另配 `counts_a_single_assistant_record` / `sums_distinct_messages`） |
| 子代理 transcript（`<session-id>/subagents/*.jsonl`）**并入同一会话** | 子代理的用量是这个会话花掉的，不单独算一盏灯；Claude 与 Qoder 是同一套目录布局，Qoder 甚至不用扫 `SubagentStop.agent_transcript_path`（hooks 篇 Qoder 节） |
| 数值**只增不减**（逐字段取 max，见 §2.4 的地板） | transcript 被截断或删行时重扫值会变小，直接覆盖会让用户已看到的数字凭空缩小 |

### 2.3 例外：Codex 的用量不走这条链

Codex 的 token **不在 `tokens.rs`**（那个文件只管 Claude 系 transcript），而来自 rollout 里 `event_msg`/`token_count` 的 `info.total_token_usage`。两点必须记住：

- 它是**会话级累计值**（不是增量），同一文件内取 max、同会话多份 rollout 各自相加；
- ⚠️ **口径与 Anthropic 相反**：`input_tokens` 是 prompt 总量、**已含** `cached_input_tokens` / `cache_write_input_tokens`（实测 `total_tokens == input + output`），映射时必须**减出来**，否则前端 `tokenTotal()` 把缓存算两遍（实测缓存占比可达 1/3）；`output_tokens` 已含 `reasoning_output_tokens`，不重复加。

rollout 链路的其余部分（发现/播种/增量/去抖）见接入指南 §9 与 hooks 篇「Codex」节。

### 2.4 token 地板：`merge_token_floor`

`set_tokens` 写入时**逐字段取 max**（`kernel.rs::merge_token_floor`），而不是整份替换。副作用是「数字只增不减」，与 `tokens.rs` 既有的取舍一致 —— 这是刻意的：宁可留着历史值，也不要屏上的数字凭空变小。§4 的存档 seed 复用的就是这同一个地板（seed 非零时先灌地板，`claude --resume` 瞬间就显示历史值，不必等重扫）。

### 2.5 单位、格式化与读取纪律

单位是**本模块自己的配置**：`module_settings['agent-status'].token_unit`，字段解释见 `src/modules/agent-status/settings.ts`，Rust 侧解析与夹取见 `src-tauri/src/modules/agent_status/settings.rs` 的 `AgentStatusSettings::from_block`。

| 取值 | 展示 | 函数（`src/modules/agent-status/lib/agent-status.ts`） |
|---|---|---|
| `tokens`（默认） | 原始计数 `1,261,847` | `formatTokenCount` |
| `m_tokens` | 百万 `1.26M`；不足 0.01M 显示 `<0.01M` | `formatTokenMillions` |

- 合计口径统一走 `tokenTotal()`（= input + cache write + cache read + output），各家展示都用它，别在组件里自己加。
- ⚠️ **只在主窗口改**（Agent 状态灯设置页），悬浮窗与提示窗只读展示。
- ⚠️ tooltip 里 token 明细的 `title`（完整数字）**恒用原始计数**，不受单位影响 —— 单位是观感，title 是事实。
- 提示窗读这份配置要走窗口级 store，且 `activate` 里必须等配置到位再取值，否则会永远停在 `fallbackConfig` 的默认单位。这条纪律的权威写法在 `../guide/floating-module-guide.md` §11（「`activate` 可能在配置异步加载完成之前被调用」）。

## 3. 展示字段 `base_url` / `prompt` 的采集口径

两个字段都从采集侧经 `AgentEvent` → Rust `SessionSnapshot` → 前端同名类型传递。**「从哪里取、按什么顺序覆盖、走不走标签引擎」的权威细则在 `../rule/agent-status-hooks.md`**（「base_url 的来源」「prompt 的来源」两节）与接入指南的表 T1，本节只登记进会话之后的规则：

| 字段 | 来源（要点，详见 hooks 篇） | 进入会话后的处理 |
|---|---|---|
| `base_url` | **配置文件优先**（Claude 系：用户级 → 项目级 → 项目本地 settings 的 `env.ANTHROPIC_BASE_URL`，后者覆盖前者，`cwd` 取自 payload）；**环境变量兜底**。Codex 读 `$CODEX_HOME/config.toml` | 只按名字取那一个 key；⚠️ **`env` 块里的 token 绝不读取**；不脱敏、仅 trim；上限 4096 |
| `prompt` | **仅**「用户提交提问」事件的 payload `prompt`；`UserPromptExpansion` 的展开模板文本**不认**（否则会覆盖用户真正的提问）；Qoder `/goal` 轮靠标签引擎 `extract` 只留 `<untrusted_objective>` 正文 | 先过通用标签引擎，再 trim + 截断；上限 4000 **字符**（按字符切，绝不按字节 —— 按字节会把多字节字符截断） |

- **同轮继承**：同一轮里后续事件（工具调用等）不带这两个字段，由 `kernel.rs` 用 `.or(previous_*)` 沿用上一值；**下一次提交**才覆盖。表现就是提示窗永远显示最近一次用户手输的提问。
- ⚠️ 二者都会**明文**写进 `%APPDATA%/<identifier>/status/inbox/*.json`，约 500ms 内被消费删除。提问可能含密钥或私有代码 —— 代价与后续驻留见 §6。
- 展示口径：Endpoint 行不直接显示 `base_url`，而是 `displayEndpoint = upstream_url ?? base_url`（上游优先、回落本地，两者都无显示 `—`；实现在 `src/modules/agent-status/lib/agent-status.ts`）。`upstream_url` 怎么反查出来的见 `agent-status-endpoint-flow.md` §2。

## 4. 会话记录持久化（SQLite，跨重启续档）

### 4.1 为什么需要

会话状态原本**完全活在内存**（`StatusRegistry` 的 HashMap），重启即丢失，`Completed` 还会按保留期清掉。后果是同一个 `session_id`（典型场景 `claude --resume`）被当成全新会话：

| 症状 | 用户看到什么 |
|---|---|
| token 归零 | 已看到的用量凭空消失 |
| `started_at_ms` 变成现在 | **灯跳位**（灯位按开始时间升序排） |
| `prompt` / `base_url` 丢失 | 要等下一次事件才有内容，中间是空的 |

`session_store.rs` 把「跨重启仍有意义」的字段落到 `<app-data>/session-records.db`（`rusqlite` bundled，零新增依赖），事件命中存档时回填。主窗「Agent 状态灯」设置页的二级 tab「会话记录」是这些行的管理面（列表 + 单条删除 + 清除全部 + 详情，转灯日志也挂在这一页，见 log 篇 §7）。

### 4.2 回填阶梯

- **回填三字段**：`started_at_ms` / `prompt` / `base_url`，优先级固定为 **事件 > 内存前驱 > 存档**，走 `apply_event_with_seed` —— seed **仅在内存里没有该会话时**才生效，绝不覆盖更晚的事实。
- `upstream_url` **不是回填字段**（`StoredSeed` 不含它）：内核的继承阶梯只用 `base_url`，上游由快照生成时按**当前**解析器现算。⚠️ 把它加进 seed 会让「换掉代理配置后仍显示旧上游」。
- 存活标记**在前端 join**：列表行「灯亮着」按 `agentStatus.sessions` 现算，不给存档加存活态字段（加了就会与内核不一致）。
- Endpoint 展示仍取 `upstream_url ?? base_url`，与 tooltip 同口径；**两者都要存** —— 只存 `base_url` 会让面板一直显示 `127.0.0.1:<port>`，只存上游则丢掉「经哪个本地代理转发」的副标注。前端读法：`src/modules/agent-status/lib/session-records.ts` 的 `recordEndpoint` / `recordLocalNote`。

### 4.3 token 地板与 seed

seed 里的 token 非零时**先灌 §2.4 那个地板**（`merge_token_floor`），于是 `--resume` 的那一刻就显示历史值，不必等 transcript 重扫完。这条与「数字只增不减」是同一个机制的两个调用点，别改成整份覆盖。

### 4.4 两套版本号（⚠️ 绝不混用）

| 名字 | 含义 | 规则 | 当前值 |
|---|---|---|---|
| `SessionRecordList.schema_version` | **线协议**版本（Rust ↔ 前端） | 按接入指南 §11 的硬规则**永不递增**；加字段一律 `Option` + `#[serde(default)]` | `1`（`session_store.rs` 的 `SCHEMA_VERSION`） |
| `PRAGMA user_version` | **库内布局**版本 | 加列时递增它并补一次 `ALTER TABLE` | `2`（`SCHEMA_USER_VERSION`） |

v1 → v2 就是给 `upstream_url` 补列：`ALTER TABLE ADD COLUMN`，老行取 NULL。护栏：`v1_database_is_migrated_with_upstream_url_column`。

### 4.5 保留与淘汰

| 规则 | 值 | 说明 / 护栏 |
|---|---|---|
| 行数上限 | `MAX_RECORDS = 200` | 超出按 `last_seen` **升序**淘汰最旧；**存活会话豁免** —— `eviction_drops_oldest_and_spares_live_sessions` |
| 启动清理 | 删 `last_seen` 超 **30 天** 的行 | `MAX_RECORD_AGE_MS`；`startup_prune_removes_records_older_than_the_window` |
| 写盘节流 | 内容没变不写、不广播 | `revision_advances_only_on_content_changes` / `second_flush_of_identical_batch_writes_nothing`（monitor 每 500ms 刷一次同一批） |

保留策略目前是**常量**，不给 UI 入口 —— 与日志那边「只有一个回收旋钮」的取舍同源（log 篇 §6）。

### 4.6 持有者与线程

- ⚠️ **只 GUI 侧持有这份库**：hook 二进制必须继续快进快出，**绝不打开** AppData 的 SQLite。这条同时被接入指南 §9（路径 B 纪律）与 log 篇 §2（「日志因此不用 SQLite」）引用 —— 三处说的是同一件事，改任何一处都要一起看。
- 刷盘在 monitor 单线程里做，且**先取快照再 I/O**，避免把 `apply_event` 卡在库锁上（`status/mod.rs` 的轮次注释写明）。
- 库不可用（被别的进程占着、路径坏了）时**降级不 panic**：`disabled_store_degrades_without_panicking`、`junk_file_is_moved_aside_and_rebuilt`、`occupied_path_is_moved_aside_and_recovers`。

### 4.7 「清除全部」的语义（⚠️ 不是清灯）

**只删归档、不强清内存**：仍在运行的会话会在下一个 hook 事件后被重新记录。面板文案与详情都按此写明。要「连灯一起清」是另一种语义（需要 `registry.remove`），本轮按需求**刻意不做**。护栏：`clear_empties_store_and_seeds_but_live_rows_return`。

面板反馈同样**不能用 `window.alert` / `confirm`**（装了 `tauri-plugin-prevent-default`，原生对话框会被拦，红线见接入指南 §11）；删除与清除都走两步确认 + `role="status"` 行内提示。

## 5. 命令与授权

注册三处的通用口径（`#[tauri::command]` + `permissions/status.toml` + capability + `lib.rs` 的 `generate_handler!`）见 `../guide/floating-module-guide.md` §12.5，此处只列本篇这几个：

| 命令 | 授权窗口 | capability |
|---|---|---|
| `get_session_records` | 所有窗口 | `capabilities/default.json` |
| `delete_session_record` / `clear_session_records` | **只设置侧** | `capabilities/main-module-settings.json` |
| `get_endpoint_profiles`（对照项，见 endpoint-flow 篇） | 所有窗口 | `default.json` |

**拆分策略**：只读命令给所有窗口（悬浮窗、提示窗与主窗派出的独立面板窗都要显示），写命令只给设置侧 —— 即 `main` 与它派出去带着同一套设置动作的 `status-log`。**读侧一律是 `async` + `spawn_blocking`** —— 存档要读盘，非 async 命令跑在主线程会冻窗口（纪律见 floating-module-guide §12.3）。事件名：`session-records`（`SESSION_RECORDS_EVENT`）。

## 6. 敏感数据与已知代价

1. ⚠️ **`prompt` 现在长期明文驻留磁盘**。原先它只在 inbox 里约 500ms 就被消费删除；落库后会长期留在 `session-records.db`，**并且在主窗口「会话记录」页可见**。提问可能含密钥或私有代码，这是已知代价，不是漏洞。
2. ⚠️ **inbox 明文**（`%APPDATA%/<identifier>/status/inbox/*.json`，约 500ms 消费删除）承载 `prompt` 与 `base_url`；`base_url` **按需求不脱敏**，原样保留（含 userinfo / query），因此**不得**把它当安全字符串拼进路径或 URL（日志那边用 `trace_id` 而不是 payload 可控的 `event_id` 拼文件名，理由相同 —— log 篇 §3/§9）。
3. **凭据一律不碰**：读 endpoint 只按名字取那一个 key，`env` 块里的 token、`~/.codex/auth.json`、`settings_config` 的 `auth` 段都不读取、不缓存、不上报（通用红线见 floating-module-guide §12.4）。
4. **转灯日志不是本子系统的外发口**：`verbose` 档会写 `prompt` 的截断摘录（≤160 字符），风险句常驻在开关下方；任何档位都不写凭据。细则见 log 篇 §5。
5. **推送默认只发状态、不带内容**：`prompt` / `endpoint` / 脚本结果都要用户显式勾选，且勾选处必须写明「ntfy 主题公开可订阅」。见 push 篇 §3。
6. **关掉日志不得影响本节任何字段**：`session-records.db` 与「记录详情」里的提问、Endpoint、Token 是**加日志之前就有的独立功能**。红线由 `status/hook.rs` 的 `a_disabled_log_still_publishes_the_event_and_touches_nothing_else` 锁（off 时事件照常落 inbox、字段原样，日志目录连建都不建）。

## 7. 红线汇总

| 做法 | 后果 |
|---|---|
| 给 `SessionSnapshot` 的 tokens / prompt / base_url / upstream_url 加字段时升 `SCHEMA_VERSION` | ❌ 新旧二进制互相丢事件且静默。一律 `Option` + `#[serde(default)]`（接入指南 §11） |
| 在 hook 进程里解析 transcript 或打开存档 SQLite | ❌ 那是每次事件都起的短进程，成本直接压在 agent 的调用路径上；且 hook 快进快出是既定契约 |
| 把线协议版本号和 `PRAGMA user_version` 当一套 | ❌ 前者永不递增，后者加列才动（当前 2）。混用的那次已经靠 `ALTER TABLE` 迁移补救 |
| 用整份替换的方式写 token（不经 `merge_token_floor`） | ❌ transcript 截断/删行时数字凭空缩小 |
| 把 `upstream_url` 做成回填字段塞进 `StoredSeed` | ❌ 换掉代理配置后仍显示旧上游；它由快照现算 |
| 让「清除全部」顺手清内存灯态 | ❌ 语义不同（需 `registry.remove`）；本轮刻意只删归档，文案必须写明 |
| 给存档加「存活态」列 | ❌ 存活标记在前端按 `agentStatus.sessions` join，加了就会和内核不一致 |
| 面板里用 `window.alert` / `confirm` | ❌ 被 `tauri-plugin-prevent-default` 拦；一律 `role="status"` + 两步确认 |
| 在 prompt / base_url 上「顺手」做脱敏改写 | ❌ 需求是不脱敏（仅 trim）；改了会与 hooks 篇的口径分家 |
| 把 token 单位改成在悬浮窗/提示窗可改 | ❌ 只在主窗改，其余窗口只读 |
| 按字节截断 `prompt` | ❌ 多字节字符会被切坏；按 `chars()` 计 |
| 在 release 路径上 `unwrap` / `expect` / 裸索引 / `slice` | ❌ release 是 `panic = "abort"`，一次 panic 带走整个 App（floating-module-guide §12.4） |

## 8. 验收清单

**自动化**

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`，其中本篇的护栏命名测试至少覆盖：
  - token：`status/tokens.rs` 的 `deduplicates_repeated_message_ids` / `counts_a_single_assistant_record` / `sums_distinct_messages`；
  - 存档：`session_store.rs` 的 `v1_database_is_migrated_with_upstream_url_column`、`eviction_drops_oldest_and_spares_live_sessions`、`startup_prune_removes_records_older_than_the_window`、`second_flush_of_identical_batch_writes_nothing`、`seed_lookup_exposes_only_backfill_fields`、`started_at_never_moves_later_across_flushes`、`clear_empties_store_and_seeds_but_live_rows_return`、`disabled_store_degrades_without_panicking`；
  - 跨子系统那条硬红线：`status/hook.rs` 的 `a_disabled_log_still_publishes_the_event_and_touches_nothing_else`。
- [ ] `npx vitest run`，其中：`lib/agent-status.spec.ts`（`tokenTotal` = input + cache write + cache read + output、两种单位格式化与 `<0.01M` 边界、`displayEndpoint` 的上游优先回落）、`lib/session-records.spec.ts`（`recordEndpoint` / `recordLocalNote` 的两段口径）、`settings.spec.ts`（`token_unit` 缺字段回落默认 + 越界夹取 + 与 Rust 同源的跨语言断言）。⚠️ `stores/sessionRecords.ts` 目前没有独立 spec，动它的合并或降级逻辑时要补。
- [ ] `npm run build`（`vue-tsc --noEmit` + vite）过。

**人工（真机）**

- [ ] 关掉转灯日志开关 → 会话记录列表与详情的 状态 / Endpoint / Token / 最近提问**逐字与关之前一致**（log 篇 §10 同条）。
- [ ] `claude --resume` 一个已存档会话：灯位不跳（`started_at_ms` 沿用）、token 立刻是历史值、Endpoint 与提问不用等下一个事件。
- [ ] 切单位（`tokens` ↔ `m_tokens`）：悬浮窗/提示窗跟着变，信息卡 title 仍是原始计数。
- [ ] 主窗「会话记录」→ 清除全部 → 仍在跑的会话在下一个事件后回档，其余消失；面板文案写明这一点。
- [ ] 悬浮窗与提示窗**没有**删除 / 清除能力（写命令只授权主窗）。

## 附录：常量与落点速查

| 项 | 值 | 定义处 |
|---|---|---|
| prompt 上限 | 4000 字符（trim + 按字符截断） | `status/compat/mod.rs` `MAX_PROMPT_CHARS` |
| base_url / transcript_path 上限 | 4096 字符 | `status/compat/mod.rs` `MAX_BASE_URL_LEN` 等 |
| inbox 消费周期 | 500 ms | `status/mod.rs` monitor 循环 |
| token 单位取值 | `tokens`（默认）/ `m_tokens` | `src/modules/agent-status/settings.ts` ↔ `settings.rs` |
| 存档行数上限 | 200（超出按 `last_seen` 淘汰，存活豁免） | `session_store.rs` `MAX_RECORDS` |
| 存档保留窗 | 启动删 `last_seen` 超 30 天 | `session_store.rs` `MAX_RECORD_AGE_MS` |
| 线协议版本 / 库内布局版本 | `1` / `2`（后者加列才动） | `session_store.rs` `SCHEMA_VERSION` / `SCHEMA_USER_VERSION` |
| 存档路径 | `<app-data>/session-records.db`（rusqlite bundled） | `session_store.rs` |
| 事件名 | `agent-status`（快照）、`session-records`（存档） | `status/mod.rs`、`session_store.rs` `SESSION_RECORDS_EVENT` |
| 落点：token 采集 | `status/tokens.rs` `TokenTracker` → `kernel.rs` `set_tokens` / `merge_token_floor` | — |
| 落点：存档读写 | `session_store.rs`（`apply_event_with_seed` 回填、`record_flush` 刷盘） | — |
| 落点：前端展示 | `src/modules/agent-status/lib/agent-status.ts`、`lib/session-records.ts`、`stores/sessionRecords.ts` | — |

**相关文档**：`../rule/agent-status-hooks.md` · `../guide/agent-status-platform-onboarding-guide.md` · `../guide/agent-status-log-guide.md` · `../guide/agent-status-push-guide.md` · `../guide/floating-module-guide.md` · `agent-status-endpoint-flow.md` · `visual-style.md`
