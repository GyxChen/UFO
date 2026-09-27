# 转灯日志（agent-status log）

> **适用范围**：`src-tauri/src/modules/agent_status/status/log/` 与 `src/modules/agent-status/{lib,stores,panels}/` 里的日志查看面。
> **一句话版本**：把「一条信号从平台进来，到最终改变灯色（或者没改变）」的每一步裁决落成一行 JSONL，让「灯为什么不动 / 为什么乱跳」从**只能复现**变成**可以回查**。
> **与其它文档的关系**：灯色语义见 `reference/doc/agent-status-hooks.md`；平台接入与「两条事件源不得互顶」见 `reference/doc/agent-status-platform-onboarding-guide.md`；模块与外壳的边界见 `reference/doc/floating-module-guide.md`。本篇只管**观测**，不管转灯判定本身。
> **版面**：查看面并入既有「会话记录」二级分区，不新开 tab —— 顶部一张满宽的「转灯日志」设置卡（只有一个 switch），日志本身是**「记录详情」末尾的一个可展开条目**（折叠态一条结论 + 两行摘要），点开展开成整窗两栏浮层（左流水 / 右选中记录的完整字段）。细节与图见 §7，mock 见 `reference/mock/status-log/`。

## 目录

1. [为什么需要它：链路上的黑洞清单](#1-为什么需要它链路上的黑洞清单)
2. [存储：为什么是 JSONL 而不是 SQLite](#2-存储为什么是-jsonl-而不是-sqlite)
3. [trace_id：贯穿一条信号的 id](#3-trace_id贯穿一条信号的id)
4. [词汇表：stage 与 reason](#4-词汇表stage-与-reason)
5. [档位与敏感数据](#5-档位与敏感数据)
6. [配置块 `module_settings['agent-status'].log`](#6-配置块-module_settingsagent-statuslog)
7. [读端与查看面](#7-读端与查看面)
8. [接线状态表（哪些理由已有产生点）](#8-接线状态表)
9. [红线](#9-红线)
10. [验收清单](#10-验收清单)
11. [已知代价](#11-已知代价)

---

## 1. 为什么需要它：链路上的黑洞清单

改之前，这条链路**一切拒绝都不留痕迹**：

| 环节 | 位置 | 原症状 |
|---|---|---|
| 兼容层静默忽略 | `status/hook.rs` `Ok(None)`；`compat/codex.rs` 只认 `PermissionRequest` | 事件到了、被丢掉，外部与「事件根本没来」完全同形 |
| hook 早退 | provider 不认 / 超 4 MiB / JSON 非法 / 落盘三步失败 | 只有一行 `hook FAILED`，不带 session / provider 归一结果 |
| 收件箱 | 非 `.json` 跳过、解析失败移隔离、隔离 rename 失败**直接删文件** | 文件凭空消失，内容永久查不到 |
| 内核四条 `return false` | `kernel.rs`：schema 不符 / 空 id / 去重命中 / 序号过旧 / 完成保留窗内 | 「灯不动」的头号原因，且四种情况彼此不可区分 |
| 子代理守卫 | 带 `agent_id` 且前态为 Completed/Failed → 只记账、`return true` | 返回 true 但灯没动，与「转灯成功」同形 |
| 别人的黄灯被摘 | 同 provider 新一轮清掉 `WaitingForPrompt` | 「我另一盏灯凭空没了」，而因在**另一个会话**的事件上 |
| 清理 | 五种删除理由（保留期 / Running / Waiting / Error 超时 / 进程已死） | 灯自己消失，零解释 |
| Codex rollout | `cares()` 预筛、分类不出、批内合并、**与上轮基线相同被丢**、已登记、播种 | 「批准后卡黄」这类历史 bug 完全没有现场 |
| 数据源健康 | `source_health` 只有 setter，**只降不升** | 隔离过一次坏文件 → 永久显示「数据源异常」 |

现在每个点都有具名 reason 与产生位置（清单见 §8）。

## 2. 存储：为什么是 JSONL 而不是 SQLite

会话存档用的是 SQLite，日志**刻意不用**：

- hook 侧必须记（它是最大的黑洞），而 hook 有一条既定红线「绝不打开 AppData 的 SQLite」—— 那是 agent 每次事件都要起的短进程，开库成本直接压在调用路径上。
- 于是两侧共用一个行格式：**一次 `open(append)` + 一次 `write_all(整批)`**。Windows 上 Rust 的 `append(true)` 映射到 `FILE_APPEND_DATA`，单次小写入在 EOF 处原子，因此多进程不会互插半行（这正是原 `hook-debug.log` 已在用、跑了很久没出问题的机制）。

**文件**：`<status_dir>/log/transitions-<UTC 日期>.<序号>.jsonl`

- **活动文件名是纯函数**（[`paths::pick_active_seq`]）：取「今天已存在且没超单文件上限」的最大序号，都超了就 +1。写者因此**自己就会换文件**，GUI 不参与切文件、只管删。
- **只有 GUI 回收**（`LogState::sweep`，与 monitor 的 10 秒清理同频）：先删**严格早于今天 UTC** 的整天文件，再按总量上限从最旧整天开始删。当天的文件永不 unlink —— 它可能正被某个 hook 持有追加句柄。
- 分文件用 **UTC**，UI 显示本地时间。原 `hook-debug.log` 那个「函数名叫 `chrono_local_now`、算的其实是 UTC 秒」的坑在此显式化了。

**读端只有一条路**：GUI 自己写的行也落同一个文件，内存环形缓冲**只由 tailer 喂**（每轮 `seek(offset) + read_to_end`，半行留给下一轮）。系统里因此只有一份真相，不需要「内存一份 + 文件一份」再对账 —— 与 `codex_rollout.rs` 增量读字节是同一套路。

## 3. `trace_id`：贯穿一条信号的 id

一条信号要能从中断处往回查，必须有一个跨进程、跨环节的 id。

⚠️ **它不是 `event_id`**。`event_id` 是 payload 可控串（`hook.rs` 只校验非空与长度），拿它拼文件名等于把 `..`、`\`、`:` 送进文件系统。所以：

- `AgentEvent.trace_id: Option<String>` + `#[serde(default)]`（**不得升 `SCHEMA_VERSION`**，否则旧 hook 写的事件整批被拒）。
- 值由 hook 自己生成（`<pid>-<纳秒>`），**同时**用作：inbox 事件文件名、事件里的 `trace_id`、该信号每一条日志行的关联键。改之前文件名和 `event_id` 是两次独立随机数，对不上。
- rollout 侧没有文件可当 id，复用它自己生成的事件 id（`rollout-<事件名>-<turn id>`，同样不来自外部输入）。
- 回归防线：`hook.rs` 的 `a_payload_id_cannot_escape_the_inbox_directory`。

UI 上「只看这一条信号」就是按 `trace_id` 过滤，摊开它从 hook 到内核的完整链路（按时间正序）。

## 4. 词汇表：stage 与 reason

唯一来源是 Rust 的两个枚举（`log/record.rs`）：`LogStage`（走到哪一段）与 `LogReason`（为什么是这个结果）。

**理由必须是具名枚举，不许自由文本** —— 否则 UI 没法聚合、没法解释，也就没法变成验收清单。

前端有一份同口径的中文解释表（`lib/status-log.ts` 的 `STAGE_LABELS` / `REASON_EXPLAIN`），由 `lib/status-log-reasons.spec.ts` 用 `?raw` 读 Rust 源比对两边清单：

- Rust 加了理由、前端没写解释 → 测试红（面板上不会出现没人看得懂的暗号）；
- 前端写了解释、Rust 没有 → 测试红（过滤下拉里不会多出永远选不中的选项，那会掩盖「那处仍是黑洞」）。

## 5. 档位与敏感数据

档位是**后端的数据模型**，四档的包含关系不变（`verbose ⊇ normal ⊇ minimal`）。⚠️ 界面上**没有档位选择器**：设置页只有一个 switch，映射见下表最后一列（§6）。

| 档位 | 记什么 | 用户文本 | 界面怎么到达 |
|---|---|---|---|
| `off`（默认） | 什么都不写 | — | switch 关闭 |
| `minimal` | 灯变化、绿闪落定、谁被摘灯 | 不记 | 界面上到不了（只可能来自手改配置） |
| `normal` | 外加每一次「为什么没转灯」的拒绝理由、巡检聚合账、配置生效 | 只记**长度** | 界面上到不了；它是「`level` 写坏了」的回落档 |
| `verbose` | 外加 rollout 逐行细节 | 记 `prompt` / `waiting_reason` 的**截断摘录**（`record::excerpt`，按字符切，≤160） | switch 打开（连同 `hook_enabled: true`） |

三条硬纪律：

1. `detail` 是唯一的自由字段，只允许白名单标量（事件名、文件 basename、计数、时长、阶段名、长度）。
2. **任何档位都不写**：`secrets.get` 的值、`auth.json` / `settings_config` 内容、endpoint 的 query 段。
3. 文件名只带 basename 不带完整路径（父目录含用户名，而日志是要给人整屏翻、甚至导出发出去的）。

`verbose` 是这里唯一会把用户输入写进磁盘的档位。所以风险那句必须**常驻在 switch 正下方**（`.lg-settings__risk`），不藏进问号角标 —— 按项目约定，勾选项自身的风险要落在勾选处。导出同理：文件一出这个窗口就不受控，所以两个导出口子都在按钮旁写明「含提问原文」。

## 6. 配置块 `module_settings['agent-status'].log`

照 `push/config.rs` 的**嵌套子块**范式：外壳与 `settings.rs` 都不解释这些字段，`LogConfig::from_block` 自己从整块里取 `log` 再逐字段宽松解析 + 夹取（坏一个字段只回落那一个）。转发点在 `settings.rs` 的 `config_hook`，热生效走 `Arc<Mutex<LogConfig>>`。

| 字段 | 默认 | 范围 |
|---|---|---|
| `level` | `"off"` | off / minimal / normal / verbose |
| `hook_enabled` | `false` | bool |
| `retention_days` | 14 | 1–90 |
| `max_file_bytes` | 8 MiB | 64 KiB–64 MiB |
| `max_total_bytes` | 96 MiB | 1 MiB–512 MiB |
| `live_tail_bytes` | 256 KiB | 64 KiB–4 MiB |
| `ring_capacity` | 5000 | 500–50000 |

⚠️ **关日志只有 `level = off` 一处，界面上也只有那一个 switch。** 曾经并存过 `enabled` ∧ `level != off` 两个入口（效果完全一致、界面互相遮蔽），也曾经把四档做成一个分段控件 —— 现在都不在了：一个行为只留一个入口。switch 的两个取值就是 §5 表里那一对：开 = `{level: "verbose", hook_enabled: true}`，关 = `{level: "off", hook_enabled: false}`。

⚠️ **两个字段必须成对写。** 只写 `level` 会留下 `hook_enabled: false`，于是 hook 那一侧（含「适配器静默忽略」那些行）永远是瞎的 —— 而那正是三家平台最容易掉灯的地方。前端映射集中在 `settings.ts` 的 `LOG_SWITCH_ON` / `LOG_SWITCH_OFF`，别在组件里各拼一份。

⚠️ **落盘必须整块提交。** 外壳的 `updateModuleSettings` 是**模块层浅合并**：`{ log: { level } }` 会把 `retention_days` 等未暴露字段直接抹掉。前端用 `logBlockWithSwitch(current, on)` 返回完整一块。

⚠️ 前端有一份同口径的默认值与范围（`settings.ts` 的 `DEFAULT_LOG_SETTINGS`），两边必须一致，否则「面板显示 14 天、实际留 30 天」。

**两个「回落」是分开定义的，别混成一个常量**（`log/config.rs` 的 `DEFAULT_LEVEL` 与 `FALLBACK_LEVEL`）：

- **没有 `log` 块** = 没人打开过开关 → `off`。默认若是 `normal`，设置页那个 switch 的读数就与真实档位对不上，且「没人碰过开关」的机器上会出现提问摘录。
- **有块但 `level` 认不出** → `normal`。能写出非法 `level` 的只有手改配置的人，他的意图明显是「要记」；静默变成「什么都不记」比静默少记一档更难查。

代价要写明：默认关意味着**不先打开就没有现场** —— 这套日志是主动排障工具，不是常开的黑匣子。

**`hook_enabled` 必须真的不碰盘**：关掉日志图的就是「别在 agent 的调用路径上写文件」，只丢内容而留着 open/write 等于没关（实现见 `Sink::disabled()`，由 `hook.rs` 按 `LogConfig::hook_writes()` 选它）。⚠️ 原先那条**无条件写的 `hook-debug.log` 也归这个开关管**：关了就什么都不写，所以「hook 到底有没有被调用过」这个证据在关闭时是不存在的 —— 要看它就得先打开开关。

## 7. 读端与查看面

命令四个（注册三处：`#[tauri::command]` + `lib.rs` 的 `generate_handler!` + `permissions/status.toml`，权限拆两份 capability）：

| 命令 | 授权范围 |
|---|---|
| `get_status_log(query?)` | 三个窗口（`default.json`） |
| `clear_status_log` / `export_status_log(path?, query?)` / `set_status_log_view(attached)` | 只主窗（`main-module-settings.json`） |

- **实时推送按批**：monitor 每轮 `drain()` 一次，只有面板开着（`set_status_log_view(true)`）且非空才 `emit(STATUS_LOG_EVENT, page)`。逐条 emit 在一次几百条时会把 IPC 打成噪声。
- 广播载荷与查询结果是**同一个形状**（`StatusLogPage`），前端只需一套 `applyPage`。
- 前端 store 合并按 `seq` 去重后**前插**，不是整份替换 —— 日志是只增的流水，覆盖会把用户正在翻的那一段抽掉。
- 页面卸载必须上报 `set_status_log_view(false)`，否则后端一直给一个不存在的面板发增量（`SessionRecordsPanel.vue` 的 `onMounted/onBeforeUnmount` 是唯一挂点）。

**查看面在「会话记录」这一页，一共三层，不做成第五个二级分区**（排障动线本来就是「这条会话的灯不对 → 它的存档 → 它的信号链路」）：

1. **顶部满宽设置卡**（`StatusLogSettingsCard.vue`）：那一个 switch + 常驻读数（「正在记 · 明细级 + hook」/「已关闭 · 零写盘」）+ switch 下方的风险句。
2. **「记录详情」末尾的可展开条目**（`StatusLogFeed.vue`）：一条结论 + 最近两行摘要 + 行尾两个动作（「导出本会话」/ 展开图标钮）。数据走独立查询 `get_status_log({ key, limit: 50 })`，**不是**从浮层那份 `entries` 里筛 —— 浮层的过滤条件变了不该把详情条目带空。计数说「本会话 N 条」按实际捞到的条数，捞满一窗说「N+」；⚠️ 绝不能拿 `page.total` 冒充，那是整个环形缓冲的条数。
3. **整窗浮层**（`StatusLogOverlay.vue`）：左 = 本会话流水（新→旧，环节下拉 / 只看被拒 / 暂停跟随），右 = 选中那条的完整信息（全字段 + `detail` + 同 `trace_id` 的其它行 + 重建的线协议一行）。外壳照 `.jse-modal` 那套类名（实色铺满、44px 标题栏、「完成」+ Esc）；⚠️ **没有复用 `ScriptEditorModal.vue`** —— 它没有 slot，模板里写死了 `ScriptEditorFrame` 与 `aria-label`。

**没有「按这一条信号过滤」这个控件**：一条信号的完整链路就在右栏里跟着选中行走（曾经是一颗可摘的 chip，和「就地展开」混在一起，界面上自相矛盾）。

**关闭态的画法（已定）**：条目**留在原处**、内容换成一行「已关闭，这个会话没有留下任何裁决痕迹」。整块不显示会让人分不清「没发生」与「没记录」，而这正是排障时最需要答案的地方。

**导出**（`export_status_log`）：路径来自原生「另存为」对话框（`tauri-plugin-dialog`，只授权主窗），取消即不发命令；`path` 省略时落到日志目录内的 `export/diagnose-*.jsonl` 作退路。⚠️ 既然路径来自参数，命令里那道校验就是唯一防线：必须绝对路径、后缀限 `jsonl/log/txt`、拒 UNC（`\\host\share` 一次写出可能变成凭据外发）、**拒落在日志目录内**（那里正被 tailer 增量读，也容不下第三个文件名）。导出内容含提问摘录 —— 这一刻才真正出这个窗口，所以按钮旁要写明。

## 8. 接线状态表

「枚举里有、但没有任何产生点」的理由等于骗人，所以这张表是验收依据。已接线：

- hook 全部分支：`unknown_provider` `stdin_unreadable` `input_too_large` `payload_not_json` `no_hook_adapter` `adapter_rejected` **`adapter_ignored_event`** `inbox_dir_unwritable` `temp_write_failed` `publish_rename_failed` `hook_completed`
- 收件箱：`not_json_extension` `stale_temp_swept` `parse_failed` `quarantine_rename_failed_file_lost` `inbox_unreadable`
- 内核：`schema_mismatch` `empty_event_id` `empty_session_id` `duplicate_in_seen_window` `stale_sequence` `completed_within_retention` `subagent_on_finished_session` `unknown_kind_inherits_timing` `phase_reduced_same` `phase_changed` `seed_applied` `refresh_only`
- 清理：`retention_elapsed` `running_timeout` `waiting_timeout` `error_timeout` `process_dead` `manually_dismissed` `flash_settled`
- 别人的黄灯：`cleared_by_new_turn_of_same_provider`
- rollout：`same_as_last_round_baseline` `unclassified_line` `activity_implies_running` `keepalive_running`；`cares_prefilter_skip` 以**聚合计数**形式落在 `rollout_scan` 行里（不逐行成行）
- 旁路：`resolver_hit` `resolver_miss`（按结论变化去重）
- 健康度：`source_degraded` **`source_recovered`**（补了只降不升那个缺口）
- 自身与配置：`config_reloaded` `log_opened` `log_rotated` `log_rotation_skipped_file_busy` `sink_disabled`

⚠️ **尚未接线**（枚举已留位，产生点还没埋；不要在下拉里当成"已支持"宣传）：
`temp_create_failed`（当前与 `temp_write_failed` 合并报告）、`already_registered`、`session_id_unparsable`、`seeded_without_event`、`sessions_dir_missing`、`rollout_file_vanished`、`batch_counters_only`、`resolver_error`、`source_*` 之外的 flow / push 全部六条（`flow_ok` `flow_error` `flow_backoff` `push_disabled` `push_no_topics` `push_queued` `push_delivered` `push_failed`）、`log_write_failed`。
接线方式已想好：flow / push 的投递跑在自己的 worker 线程，用 `app.try_state::<StatusState>()` 取 `log` 即可，不需要给 `FlowState` / `PushState` 加构造参数。

## 9. 红线

| 做法 | 后果 |
|---|---|
| 让日志写路径能改变灯态（`?` 冒泡、锁内埋点、失败早退） | ❌ 观测面变成第二套裁决。埋点一律吞结果；裁决化只加返回值、不改任何 `return` 分支的判定；埋点全在 registry 锁**外**；monitor 单线程是唯一 GUI writer，因此不会与 `LogState` 形成嵌套锁序 |
| 用 `event_id` 当文件名或贯穿 id | ❌ 那是 payload 可控串，等于把路径注入交给平台。只能用 `trace_id` |
| 默认档写 `prompt` / `waiting_reason` / `tool_input.command` | ❌ 日志是给人整屏翻、并会随导出包离开本机的。默认只记长度 |
| 把 `hook-debug.log` 那套留着不管 | ❌ 它的目录不跟 `--status-dir`，会出现「日志说有事件、目录里没有」。已由本系统取代 |
| 逐行记 rollout 的预筛跳过 | ❌ `cares()` 一轮就能刷几千条，把有用的行淹掉。聚合计数进 `rollout_scan` |
| 在日志的 `release` 路径上 `unwrap` / `expect` / 索引 / `slice` | ❌ release 是 `panic = "abort"`，一次 panic 带走整个 App |
| 为日志升 `SCHEMA_VERSION` 或给 `AgentEvent` 加必填字段 | ❌ 旧 hook 写的事件会整批落 quarantine |
| 把「清除日志」做成「保证一条不漏」 | ❌ 抬序号后仍在按旧序号写的 hook 会把在途几条写进即将被删的文件。这是手动清空的既定代价，文案要写明 |

## 10. 验收清单

**自动化**

- [ ] `cd src-tauri && cargo test` —— 全绿；其中 `log::{record,paths,sink,config,runtime}` 覆盖：多进程行不交错、活动文件序号规则、轮转只删昨日文件、off 档真的不写、半行留给下一轮、清空后仍能续读、导出表头、**导出路径校验**（相对 / UNC / 错后缀 / 落在日志目录内都要被拒，被拒后不留残留文件）。
- [ ] ⚠️ **本轮硬验收**（`hook.rs` 的 `a_disabled_log_still_publishes_the_event_and_touches_nothing_else`）：日志开关关掉时，hook 仍照常把规范事件落 inbox，`prompt` / `base_url` 原样进事件，而日志目录**连建都不建**。提问采集与 `session-records.db` 是加日志之前就有的独立功能 —— 这条一旦红，就是功能回归。
- [ ] 测试用的「正在记」配置走 `runtime::tests::recording()` 这类显式构造，不许借 `LogConfig::default()`：默认已是 `off`，借它当「开着」会让测试变成空断言（这次 7 个运行态测试就是这么炸出来的）。
- [ ] `npx vitest run` —— 全绿；其中：
  - `lib/status-log.spec.ts`：色调三档、`isRejected` 按理由而非按环节判、「本会话 N 条」不许拿 `page.total` 冒充、重建的线协议行不含内存 `seq`。
  - `lib/status-log-reasons.spec.ts`：`?raw` 读 `record.rs` 断言 stage / reason 清单**双向**同源、短标签互不重复（下拉里两个同名就没法选）、事件名与 `STATUS_LOG_EVENT` 一致。
  - `stores/statusLog.spec.ts`：attach/detach 成对且重复 stop 不再发命令、增量按 `seq` 去重前插、暂停跟随时不动画面只涨计数、翻页用最后一条游标、通道坏时落 `degraded` 而不是永远「正在读取」。
  - `panels/StatusLogSettingsCard.spec.ts`：switch 成对写两个字段、**整块**提交保住未暴露字段、落盘失败回退本地值。
  - `agent-status-css.spec.ts`：列表类 grid 钉 `align-content: start`（不钉会把行拉开）、会话卡定高 + 提问单行、满宽卡两条类权重、浮层高度链 `min-h-0`。
- [ ] `cargo check --lib` 的 dead-code 警告是「还有没有黑洞」的探测器：某个 `LogReason` 变体 never constructed ⇒ 那处仍未接线（或该删）。
- [ ] `npm run build`（`vue-tsc --noEmit`）通过。

**人工（真机）**

- [ ] 打开 switch → 装好 Claude hook 跑一轮对话 → 详情条目出现「一条结论 + 两行摘要」；点开展开 → 浮层左列能看到 `hook → 落盘 → 内核裁决 → 灯变`，点其中一行，右列出现它的全部字段与同 `trace_id` 的其它行。
- [ ] Codex 只发非 `PermissionRequest` 事件 → 出现一行 `兼容层 · adapter_ignored_event`，带 provider 与原始事件名。
- [ ] 同一 `event_id` 连发两次 → 第二条是 `内核 · duplicate_in_seen_window`，而不是「什么都没发生」。
- [ ] Codex 会话弹审批 → 批准后**立刻**回绿，日志里能看到 `baseline_reflow` 顶住了 `same_as_last_round_baseline`。
- [ ] 开两个会话，A 完成落黄、B 开始新一轮 → A 出现一行 `清空闲灯 · cleared_by_new_turn_of_same_provider`。
- [ ] 把 `max_file_bytes` 调到下限跑五分钟 → 出现新序号文件与 `log_rotated`；跨日旧文件被删。
- [ ] **关掉 switch** → 新行不再产生、`hook-debug.log` 也不再写；但「会话记录」列表与「记录详情」的状态 / Endpoint / Token / 最近提问**逐字与关之前一致**；已有记录仍可查询与导出。
- [ ] 「导出本会话」与浮层的「导出当前条件」→ 都弹**原生另存为**（这是 `tauri-plugin-prevent-default` 之下必须实测的一条：插件对话框走 IPC→RFD，与被拦的 webview `alert/confirm` 不是同一通道 ——  theory 不等于结论）。选一个路径 → 文件首行是表头（App 版本、时区、当刻配置、过滤条件），正文旧→新。
- [ ] 在另存为里点取消 → 不发命令、不产生文件、界面不报错。
- [ ] 悬浮窗与提示窗**没有**清空 / 导出能力（权限只给了主窗）。

## 11. 已知代价

1. **日志目录跟着 `status_dir` 走**：用户用 `--status-dir` / `UFO_STATUS_DIR` 覆盖时，那些 hook 行落在被覆盖的目录，而配置永远在 AppData 根下（全局唯一）—— 于是主窗面板读不到它们。浮层常驻显示当前日志目录路径就是为了这一条。
2. **默认关 = 出问题时没有现场**：用户得先记得打开。这是明码标价的选择（另一个方向意味着「没人碰过开关却在落提问摘录」），不是遗漏。
3. **常开后的磁盘上限 = `max_total_bytes`（默认 96 MiB）**；`verbose`（= switch 打开）档体积与敏感度同时上升。
4. **手动清空会丢在途的少数几条**（见 §9 最后一条）。
5. **导出接受路径**：命令只授权主窗，且有 `validate_export_target` 那道校验，但它确实是本项目里唯一「按调用方给的地点写文件」的口子。
4. **不做 `sync_all`**：hook 侧为了不把退出拖慢，系统崩溃时可能丢最后几 KB。
5. **不修既有语义**：`order_inbox_batch` 的跨批乱序仍无法识破（没有可靠源序号），日志只是把它**显示**成「后到的低 `received_at` 事件被采纳」—— 是观测，不是修复。
6. **时间口径分家**：文件名用 UTC、UI 显示本地。跨零点的批次整批归入新文件，于是「昨天产生的几条」和「今天的」同处一个文件；读端按文件全量读，不丢。
7. **旧 `hook-debug.log` 不再写**，但磁盘上遗留的旧文件不主动删（避免误删用户手放的东西）—— 想清掉请手工删除 `<status_dir>/hook-debug.log`。
