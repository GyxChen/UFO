# 转灯日志（agent-status log）

> **适用范围**：`src-tauri/src/modules/agent_status/status/log/` 与 `src/modules/agent-status/{lib,stores,panels}/` 里的日志查看面。
> **一句话版本**：把「一条信号从平台进来，到最终改变灯色（或者没改变）」的每一步裁决落成一行 JSONL，让「灯为什么不动 / 为什么乱跳」从**只能复现**变成**可以回查**。
> **与其它文档的关系**：灯色语义见 `../rule/agent-status-hooks.md`；平台接入与「两条事件源不得互顶」见 `agent-status-platform-onboarding-guide.md`；模块与外壳的边界见 `floating-module-guide.md`；本篇那条「关日志不得影响会话存档」的红线，被影响的存档链路见 `../rule/agent-status-session-data.md`。本篇只管**观测**，不管转灯判定本身。
> **与 AGENTS.md 的关系**：`AGENTS.md` 已收敛成不超过 100 行的稳定规则 + 指向本目录的链接，日志这一套细则**只在本文**。文档树是 `reference/doc/guide/`（指南）与 `reference/doc/rule/`（规范）；本文与代码冲突时**以代码为准**（数字与行号会漂，符号名不会）。
> **版面**：查看面并入既有「会话记录」二级分区，不新开 tab —— 顶部一张满宽的「转灯日志」设置卡（只有一个 switch），日志本身是**「记录详情」末尾的一个可展开条目**（折叠态一条结论 + 两行摘要），点开展开成**一扇独立窗口（`status-log`）里的两栏正文**（左流水 / 右选中记录的完整字段）。细节与图见 §7。

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

**读端只有一条路 = 盘上那份**。GUI 自己写的行也落同一个文件；tailer 每轮 `seek(offset)` 续读（半行留给下一轮），并且**给每条记住它的盘上坐标** `(UTC日, 当日序号, 起始字节)`。

面板的分页、计数与导出**全部按盘读**（`log/scan.rs`），内存里不再驻留任何"可看范围"—— 早先那份环形缓冲既封顶了能看多少、也封顶了能导多少，已经整块删掉（连 `ring_capacity` 这个配置字段一起：没有消费者的旋钮就是骗人的设置）。`next_seq` 只保留一个用途：告诉前端「这一轮有没有新行进来过」。

## 3. `trace_id`：贯穿一条信号的 id

一条信号要能从中断处往回查，必须有一个跨进程、跨环节的 id。

⚠️ **它不是 `event_id`**。`event_id` 是 payload 可控串（`hook.rs` 只校验非空与长度），拿它拼文件名等于把 `..`、`\`、`:` 送进文件系统。所以：

- `AgentEvent.trace_id: Option<String>` + `#[serde(default)]`（**不得升 `SCHEMA_VERSION`**，否则旧 hook 写的事件整批被拒）。
- 值由 hook 自己生成（`<pid>-<纳秒>`），**同时**用作：inbox 事件文件名、事件里的 `trace_id`、该信号每一条日志行的关联键。改之前文件名和 `event_id` 是两次独立随机数，对不上。
- rollout 侧没有文件可当 id，复用它自己生成的事件 id（`rollout-<事件名>-<turn id>`，同样不来自外部输入）。
- 回归防线：`hook.rs` 的 `a_payload_id_cannot_escape_the_inbox_directory`。

UI 上「只看这一条信号」就是按 `trace_id` 过滤，摊开它从 hook 到内核的完整链路（按时间正序）。

## 4. 词汇表：stage 与 reason

唯一来源是 Rust 的两个枚举（`log/record.rs`）：`LogStage`（走到哪一段）与 `LogReason`（为什么是这个结果）。当前是 22 个 stage + 68 个 reason —— **数字只是量级，清单以枚举为准**（§8 那张接线账才是验收依据）。

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

`verbose` 是这里唯一会把用户输入写进磁盘的档位。所以「明细级会落提问与等待原因的截断摘录、凭据任何档位都不写」+「关掉不影响下面的会话存档」这两句必须**贴着那颗 switch 写**（现落在 `StatusLogSettingsCard.vue` 里 switch 那一条 `SettingsField` 的 `hint` 上，由问号角标 `ui/SettingsHint.vue` 展开）—— 曾经常驻在 switch 正下方的 `.lg-settings__risk` 已随「解释性 tips 统一收进角标」的项目约定删掉，别再拆回常驻文本行；但**不许把风险做成默认值、也不许把它从开关处挪走**这一条不变。导出同理：文件一出这个窗口就不受控，所以导出这一刻要提示「含提问摘录」（`StatusLogPanel.vue` 的注释写明了这个意图，落点目前是设置卡的 hint + 原生另存为对话框的标题）—— 加改动时别让它只剩注释。

## 6. 配置块 `module_settings['agent-status'].log`

照 `push/config.rs` 的**嵌套子块**范式：外壳与 `settings.rs` 都不解释这些字段，`LogConfig::from_block` 自己从整块里取 `log` 再逐字段宽松解析 + 夹取（坏一个字段只回落那一个）。转发点在 `settings.rs` 的 `config_hook`，热生效走 `Arc<Mutex<LogConfig>>`。

| 字段 | 默认 | 范围 |
|---|---|---|
| `level` | `"off"` | off / minimal / normal / verbose |
| `hook_enabled` | `false` | bool |
| `retention_days` | 7 | 1–14（界面上只给 7 / 14 两档） |
| `max_file_bytes` | 8 MiB | 64 KiB–64 MiB |
| `max_total_bytes` | 96 MiB | 1 MiB–512 MiB |
| `live_tail_bytes` | 256 KiB | 64 KiB–4 MiB |

⚠️ **关日志只有 `level = off` 一处，界面上也只有那一个 switch。** 曾经并存过 `enabled` ∧ `level != off` 两个入口（效果完全一致、界面互相遮蔽），也曾经把四档做成一个分段控件 —— 现在都不在了：一个行为只留一个入口。switch 的两个取值就是 §5 表里那一对：开 = `{level: "verbose", hook_enabled: true}`，关 = `{level: "off", hook_enabled: false}`。

⚠️ **两个字段必须成对写。** 只写 `level` 会留下 `hook_enabled: false`，于是 hook 那一侧（含「适配器静默忽略」那些行）永远是瞎的 —— 而那正是三家平台最容易掉灯的地方。前端映射集中在 `settings.ts` 的 `LOG_SWITCH_ON` / `LOG_SWITCH_OFF`，别在组件里各拼一份。

⚠️ **落盘必须整块提交。** 外壳的 `updateModuleSettings` 是**模块层浅合并**：`{ log: { level } }` 会把 `retention_days` 等未暴露字段直接抹掉。前端用 `logBlockWithSwitch(current, on)` 返回完整一块。

⚠️ 前端有一份同口径的默认值与范围（`settings.ts` 的 `DEFAULT_LOG_SETTINGS` 与 `MIN/MAX_LOG_RETENTION_DAYS`），两边必须一致，否则「界面说 14 天、后端按别的天数删」。跨语言那条断言在 `settings.spec.ts`（`?raw` 读本目录的 `log/config.rs` 抠常量）。

⚠️ **`retention_days` 是界面上唯一的回收旋钮**，而且**收成了一只下拉、只有 7 / 14 两档**（`LOG_RETENTION_DAYS_CHOICES`）：默认 7 天，Rust 侧夹取区间 1–14，两档之外的天数只在配置里存在、界面上选不出来。盘上若还留着更早那版**数字输入框**能写下的 1–90 里任意一个数，界面会**就近取档并写回一次**（`nearestRetentionChoice` + 同一个目标值只试一次，不反复自旋）—— 理由是「显示与事实不许分离」：只把读数改成 14 而后端仍按 3 天删，是这个旋钮最不该出现的样子。体积上限（`max_file_bytes` / `max_total_bytes`）继续存在但**不给 UI 入口**：两个旋钮会让人分不清「留多久」与「占多少」谁说了算，而容量那条线只在量特别大时才先撞。它既然仍会在背后削掉最旧的整天，界面就必须把**目录实际占用**常驻写出来（`LogDiskUsage`，随 `StatusLogPage.usage` 到三处）、并在本次运行内真削过时追加一句 —— 否则"保留 N 天"是个会被静默打破的承诺。

**两个「回落」是分开定义的，别混成一个常量**（`log/config.rs` 的 `DEFAULT_LEVEL` 与 `FALLBACK_LEVEL`）：

- **没有 `log` 块** = 没人打开过开关 → `off`。默认若是 `normal`，设置页那个 switch 的读数就与真实档位对不上，且「没人碰过开关」的机器上会出现提问摘录。
- **有块但 `level` 认不出** → `normal`。能写出非法 `level` 的只有手改配置的人，他的意图明显是「要记」；静默变成「什么都不记」比静默少记一档更难查。

代价要写明：默认关意味着**不先打开就没有现场** —— 这套日志是主动排障工具，不是常开的黑匣子。

**`hook_enabled` 必须真的不碰盘**：关掉日志图的就是「别在 agent 的调用路径上写文件」，只丢内容而留着 open/write 等于没关（实现见 `Sink::disabled()`，由 `hook.rs` 按 `LogConfig::hook_writes()` 选它）。⚠️ 原先那条**无条件写的 `hook-debug.log` 也归这个开关管**：关了就什么都不写，所以「hook 到底有没有被调用过」这个证据在关闭时是不存在的 —— 要看它就得先打开开关。

## 7. 读端与查看面

命令五个（注册三处：`#[tauri::command]` + `lib.rs` 的 `generate_handler!` + `permissions/status.toml`，权限拆两份 capability）：

| 命令 | 授权范围 |
|---|---|
| `get_status_log(query?)` | 五个窗口（`default.json`） |
| `clear_status_log` / `export_status_log(path?, query?)` / `count_status_log(query?)` / `set_status_log_view(attached)` | 设置侧：主窗 **与它派出的 `status-log` 窗**（`main-module-settings.json`；导出用的 `dialog:allow-save` 同处给出） |

⚠️ `get_status_log` / `export_status_log` / `count_status_log` 三条都是 **`async` + `spawn_blocking`**：它们现在要读盘（分页要倒着翻文件，计数与导出要扫完整目录），而非 async 命令跑在**主线程** —— 几百 MiB 的目录能把窗口事件循环冻住几秒到几十秒。带引用入参的 async 命令必须返回 `Result`（tauri 宏要求）。`count_status_log` 归 manage 那一侧是因为它**必定**全目录扫描，是"贵"的读，而它的调用方只在设置侧（主窗那一页 + 日志窗）。

- **实时推送按批**：monitor 每轮 `drain()` 一次，只有还有窗口 attach 着（`set_status_log_view(true)`）且非空才 `emit(STATUS_LOG_EVENT, page)`。逐条 emit 在一次几百条时会把 IPC 打成噪声。
- 广播载荷与查询结果是**同一个形状**（`StatusLogPage`），前端只需一套 `applyPage`。
- 前端 store 合并按 **`id`（盘上坐标）**去重后前插，不是整份替换 —— 日志是只增的流水，覆盖会把用户正在翻的那一段抽掉。线协议里已经没有 `seq`：那是进程内计数器，重启归零，拿它去重或翻页都会串到别的行上。
- **只有列表钉在顶部时才前插**（`store.pinned` 由组件的 scroll 回调写）。人在看中间那一段时前插会把那一段挤走；等高行的锚定补偿要读旧首行位置再回滚，是这类列表最容易错的一块 —— 用「钉顶才跟」换掉它，攒下的条数由那颗「攒了 N 条，跟回最新」的 chip 兜住。
- 翻页锚点是上一页的 `next_before`（语义「严格比它更旧」）；`exhausted` 为真时收起「看更早的」。增量广播包不带这两个字段，前端合并时也不读它们。
- 页面卸载必须上报 `set_status_log_view(false)`，否则后端一直给一个不存在的面板发增量。⚠️ **attach 是按调用方窗口 label 记账的**（`LogState::set_viewer(label, attached)` → `Runtime.viewers: Vec<String>`，清单非空才广播；`clear()` 保留这份清单 —— 谁在看是运行时事实，与盘上有没有内容无关）：主窗那一页（`SessionRecordsPanel.vue` 的 `onMounted/onBeforeUnmount`）与独立日志窗（`StatusLogWindow.vue` 的 open / 收起）**各是一条 viewer**，共用一个 bool 的话一侧收起就会把另一侧的增量悄悄关掉。

**查看面挂在「会话记录」这一页，一共三层（第三层由这一页点开，但自己是一扇窗），不做成第五个二级分区**（排障动线本来就是「这条会话的灯不对 → 它的存档 → 它的信号链路」）：

1. **顶部满宽设置卡**（`StatusLogSettingsCard.vue`）：**只有两条标准设置行** —— ① 「转灯日志」switch（风险句与「关掉不影响会话存档」在那一条的 `hint` 里；**不给 `state` 读数**：在不在记，开关自己说得出，「正在记 / 已关闭 · 零写盘」这行字归下面第二层的条目头）；② 「保留天数」**下拉**（`BaseSelect`，只有 7 / 14 两档）：标题与问号角标在左、下拉钉右，**目录占用**走 `SettingsField` 的 `state`、常驻在标题**下面那一行**（`logUsageLabel`：`占用 24.7 MiB · 9 个文件 · 最早 09-22`，被容量线削过时追加「更早的已按容量上限回收」；`log.start()` 首拉之前说「占用读取中…」而不是「还没写过日志」）。选中即落盘（不像早先的数字输入框要等失焦），越界值先就近取档再写回；`log` 子块整块提交。
2. **「记录详情」末尾的可展开条目**（`StatusLogFeed.vue`）：一条结论 + 最近两行摘要 + 行尾两个动作（「导出本会话」/ 展开图标钮，点它开独立日志窗并只递一个 `sessionKey`，契约见 `lib/status-log-window.ts`）。数据走独立查询 `get_status_log({ key, limit: 50 })`，**不是**从日志窗那份 `entries` 里筛 —— 那边的过滤条件变了不该把详情条目带空。条数另走 `count_status_log({ key })`，文案是「本会话 N 条 · 截至 HH:MM:SS」：**精确值 + 它是截至哪一刻的数**，所以任何一档都不需要「N+」或「约」这种含糊（加载中说「计数中…」，失败说「计数暂时读不到」，都不许留着旧数当事实）。
3. **独立日志窗**（外壳 `StatusLogWindow.vue` 走 `ufo-panel-window-open` 协议拿 `sessionKey`，正文 `StatusLogPanel.vue`）：左 = 本会话流水（新→旧，环节下拉 / 只看被拒 / 暂停跟随），左右比例 **1.8 : 1**（行是长文本单行截断，宽度给流水；右栏窄一点靠竖排读，⚠️ 因此右栏所有长标量必须 `overflow-wrap: anywhere`，护栏在 `agent-status-css.spec.ts`），**虚拟列表** = `@tanstack/vue-virtual`（本模块唯一的第三方前端依赖：headless，只给「该渲染哪几行」，样式全在我们这边）：行等高（CSS `--lg-row-h` 与 `lib/status-log.ts` 的 `LOG_ROW_HEIGHT_PX` 由护栏对撞），只渲染视口 ±overscan 行，总高由 `.lg-rows__sizer` 这一个占位元素撑、条目绝对定位 `translateY`，容器 `overflow-anchor: none`；所以「能看多少」不再由前端驻留上限封顶。⚠️ **不许改回「padding 垫高 + 自己算除法」**：Tailwind 全局 `box-sizing: border-box`，垫高一旦超过可视高，滚动容器会被自己的 padding 撑到整表高度（无头 Chrome 实测 `clientHeight` = 5910px），内部滚动当场失效、「看更早的」与正文底栏被整页滚走；库另外顺手解决了「只量一次可视高度」—— 它用 ResizeObserver 观察滚动元素，容器一变高就重算；右 = 选中那条的完整信息（全字段 + `detail` + 同 `trace_id` 的其它行 + 重建的线协议一行，末尾标出它在盘上的字节坐标）。外壳不再是 `.jse-modal` 那一套，而是与脚本窗、插件日志窗共用的 `PanelWindow.vue`（`.panel-shell` 实色圆角 + `.panel-bar` 40px 标题栏与主窗同版式（左端 UFO 图标 + 名字，右端 × 关闭钮））。⚠️ 这扇窗**不在 `tauri.conf.json` 里声明**：由宿主的 `open_panel_window` 按需建、关掉即销毁（声明即建窗 = 一扇隐藏窗白占一个 WebView2 renderer，约 85~105 MB），所以载荷走两段 —— 新建那一次窗口挂载后自取暂存，之后每次 open 收 `ufo-panel-window-open`，两条都由 `useStatusLogOpen()` 接上。detach 的口径：× 与 Esc 由前端在收起前先 `log.stop()`，Alt+F4 没有出口，由宿主在 `WindowEvent::Destroyed` 里按 label 补一刀，两条都不会留下「人看不见了还在每轮广播」（见悬浮窗 guide §13.1）。—— 从前那两处各写一份标题栏，是因为脚本窗那时还没有 slot，两个浮层没法共用。

**没有「按这一条信号过滤」这个控件**：一条信号的完整链路就在右栏里跟着选中行走（曾经是一颗可摘的 chip，和「就地展开」混在一起，界面上自相矛盾）。右栏那条链路走**独立的 `get_status_log({ trace_id })` 查询**，不从左栏已载入的行里筛 —— 一则分页/虚拟列表下左栏只有一小段，二则勾了「只看被拒」时链路里被过滤掉的兄弟行照样该给（这一栏的意义就是看完整链路）。

**关闭态的画法（已定）**：条目**留在原处**、内容换成一行「已关闭，这个会话没有留下任何裁决痕迹」。整块不显示会让人分不清「没发生」与「没记录」，而这正是排障时最需要答案的地方。

**导出**（`export_status_log`）：**按当前条件把盘上命中的行全部写出去**（`scan::scan_forward` 正序流式 + `BufWriter` 逐行），条数没有上限、也不再被内存视图封顶；扫描期间界面挂加载遮罩并 disable 按钮（重复点就是多扫一遍全目录），被扫描预算截断时反馈必须说「不完整」。命中 0 条仍产出一份带表头的文件 —— 那是"我按这个条件翻过整个目录，确实没有"的证据。表头只写开打前就知道的事（机器、配置、条件、覆盖范围）；条数要扫完才知道，所以在返回值里，不留一个骗人的占位。路径来自原生「另存为」对话框（`tauri-plugin-dialog`，`dialog:allow-save` **只授权设置侧：主窗与它派出的日志窗**），取消即不发命令；`path` 省略时落到日志目录内的 `export/diagnose-*.jsonl` 作退路。⚠️ 既然路径来自参数，`log/runtime.rs::validate_export_target` 就是**唯一防线**：必须绝对路径、后缀限 `jsonl/log/txt`、拒 UNC（`\\host\share` 一次写出可能变成凭据外发）、**拒落在日志目录内**（那里正被 tailer 增量读，也容不下第三个文件名）。导出内容含提问摘录 —— 这一刻才真正出这个窗口，所以按钮旁要写明。

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
| **关掉日志时顺带动了事件的发布与字段采集**（不发 inbox、不填 `prompt` / `base_url`、或让日志目录的建立牵连别的落盘） | ❌ 硬红线：「会话记录 / 记录详情」里的提问、Endpoint、Token 是**加日志之前就有的独立功能**（`session_store.rs`），关日志不得影响它们半分。由 `hook.rs` 的 `a_disabled_log_still_publishes_the_event_and_touches_nothing_else` 锁：off 时事件照常落 inbox、字段原样，而日志目录**连建都不建**（`hook-debug.log` 也归这个开关管）。这条一红就是功能回归，不是日志少记了几行 |
| 把「清除日志」做成「保证一条不漏」 | ❌ 抬序号后仍在按旧序号写的 hook 会把在途几条写进即将被删的文件。这是手动清空的既定代价，文案要写明 |

## 10. 验收清单

**自动化**

- [ ] `cd src-tauri && cargo test` —— 全绿；其中 `log::{record,paths,sink,config,runtime,scan}` 覆盖：多进程行不交错、活动文件序号规则、轮转只删昨日文件、off 档真的不写、半行留给下一轮、清空后仍能续读、导出表头、**导出路径校验**（相对 / UNC / 错后缀 / 落在日志目录内都要被拒，被拒后不留残留文件）。
- [ ] **倒序分页的三条边界**（`scan.rs`）：块头那一刀切在行中间时既不重复也不漏（`page_boundary_neither_duplicates_nor_skips_the_anchor_row` 逐页翻到底，条数与唯一性都断）、尾部半行永不进倒序结果、锚点所在文件被回收后**退化**成翻更旧的文件而不是顶成空列表。块长必须是**入参**（测试传 64 字节逼它跨块），写成常量就测不到这三条。
- [ ] **读侧不再被内存封顶**：`ring_capacity=500` 写 600 行 ⇒ 导出与计数都得是 600（`export_writes_every_matching_line_on_disk_beyond_one_page` / `count_matches_is_exact_and_independent_of_ring_capacity`）；实时那一路与按盘那一路必须给出**同一个** `id`（`ring_entries_carry_the_same_position_the_scanner_computes`），否则前端去重是空谈。
- [ ] 计数缓存：同查询在节流窗口内回同一份（`scanned_bytes` 不变 ⇒ 没重扫），有新行落盘后作废并给出新的 `as_of_ms`。
- [ ] ⚠️ **本轮硬验收**（`hook.rs` 的 `a_disabled_log_still_publishes_the_event_and_touches_nothing_else`）：日志开关关掉时，hook 仍照常把规范事件落 inbox，`prompt` / `base_url` 原样进事件，而日志目录**连建都不建**。提问采集与 `session-records.db` 是加日志之前就有的独立功能 —— 这条一旦红，就是功能回归。
- [ ] 测试用的「正在记」配置走 `runtime::tests::recording()` 这类显式构造，不许借 `LogConfig::default()`：默认已是 `off`，借它当「开着」会让测试变成空断言（这次 7 个运行态测试就是这么炸出来的）。
- [ ] `npx vitest run` —— 全绿；其中：
  - `lib/status-log.spec.ts`：色调三档、`isRejected` 按理由而非按环节判、占用与计数文案（`logCountLabel` 四态里不许出现「+」或「约」）、重建的线协议行不含内存游标那种假身份。
  - ~~`lib/log-window.spec.ts`~~ **已删**（2026-09-29 换成 `@tanstack/vue-virtual`）：窗口算术不再归我们写，而 happy-dom 没有排版引擎，`clientHeight` 恒为 0 —— 留那份恒等式测试等于测一个不再存在的模型。几何改由**无头 Chrome 实测**：把真组件挂在只喂 4000 条假行的临时入口里（用完即删），量四件事 —— 容器 `clientHeight` 由版面钉住（552px，不是内容的 120000px）、`scrollHeight` = 条数 × 行高、翻到底 `scrollTop` 恰等于 `scrollHeight − clientHeight` 且最后一行贴容器底、把容器改矮后渲染条数自动从 31 掉到 20（ResizeObserver 在管，不需要手工 `measure()`）。
  - `lib/status-log-reasons.spec.ts`：`?raw` 读 `record.rs` 断言 stage / reason 清单**双向**同源、短标签互不重复（下拉里两个同名就没法选）、事件名与 `STATUS_LOG_EVENT` 一致。
  - `stores/statusLog.spec.ts`：attach/detach 成对且重复 stop 不再发命令、增量按 `id` 去重前插、**滚离顶部时只涨 `pending` 不动画面**、翻页发出去的 `before` 必须等于上一页的 `next_before`、`exhausted` 后不再发翻页请求、条数只来自 `count_status_log`（含迟到回包要按 identity 丢弃）、通道坏时落 `degraded` 而不是永远「正在读取」。
  - `panels/StatusLogSettingsCard.spec.ts`：switch 成对写两个字段、**整块**提交保住未暴露字段、落盘失败回退本地值；盘上那个**选不出来的天数**（旧数字输入框写的 1–90）要**就近取档并写回一次**，同一个目标值不反复自旋，写回失败要还能再试；占用读数在 store 首拉回答前说「占用读取中…」而不是「还没写过日志」。
  - `settings.spec.ts` 的跨语言断言：`?raw` 读 `log/config.rs` 抠 `DEFAULT/MIN/MAX_RETENTION_DAYS`，与 `DEFAULT_LOG_SETTINGS` 同值。
  - `agent-status-css.spec.ts`：列表类 grid 钉 `align-content: start`（不钉会把行拉开；⚠️ `.lg-rows` 换成虚拟列表后已不是 grid，这条只查 `.sr-list`）、会话卡定高 + 提问单行、满宽卡两条类权重、日志窗正文 `.lg-window__body` 的高度链（`min-h-0` + `flex-1`，`relative` 是给导出遮罩当定位祖先的；⚠️ 同一份护栏还断言 `.lg-modal` **永不回来** —— 那一层整窗浮层已经删干净）；虚拟列表那几条 —— `--lg-row-h` 与 `LOG_ROW_HEIGHT_PX` 同值、`.lg-rows` 必须 `relative` + `overflow-y-auto` + `overflow-anchor: none` 且**规则体里不许有 padding**、`.lg-rows__sizer` relative / `.lg-rows__cell` absolute、组件原文里不许再出现 `paddingTop` / `paddingBottom`（回到老的 padding 垫高就是当场复发）、`.lg-pane` 带 `min-h-0` + `minmax(0, 1fr)`。
- [ ] `cargo check --lib` 的 dead-code 警告是「还有没有黑洞」的探测器：某个 `LogReason` 变体 never constructed ⇒ 那处仍未接线（或该删）。
- [ ] `npm run build`（`vue-tsc --noEmit`）通过。

**人工（真机）**

- [ ] 打开 switch → 装好 Claude hook 跑一轮对话 → 详情条目出现「一条结论 + 两行摘要」；点开展开 → 独立日志窗左列能看到 `hook → 落盘 → 内核裁决 → 灯变`，点其中一行，右列出现它的全部字段与同 `trace_id` 的其它行。
- [ ] Codex 只发非 `PermissionRequest` 事件 → 出现一行 `兼容层 · adapter_ignored_event`，带 provider 与原始事件名。
- [ ] 同一 `event_id` 连发两次 → 第二条是 `内核 · duplicate_in_seen_window`，而不是「什么都没发生」。
- [ ] Codex 会话弹审批 → 批准后**立刻**回绿，日志里能看到 `baseline_reflow` 顶住了 `same_as_last_round_baseline`。
- [ ] 开两个会话，A 完成落黄、B 开始新一轮 → A 出现一行 `清空闲灯 · cleared_by_new_turn_of_same_provider`。
- [ ] 把 `max_file_bytes` 调到下限跑五分钟 → 出现新序号文件与 `log_rotated`；跨日旧文件被删。
- [ ] **关掉 switch** → 新行不再产生、`hook-debug.log` 也不再写；但「会话记录」列表与「记录详情」的状态 / Endpoint / Token / 最近提问**逐字与关之前一致**；已有记录仍可查询与导出。
- [ ] 「导出本会话」与独立日志窗的「导出当前条件」→ 都弹**原生另存为**（这是 `tauri-plugin-prevent-default` 之下必须实测的一条：插件对话框走 IPC→RFD，与被拦的 webview `alert/confirm` 不是同一通道 ——  theory 不等于结论）。选一个路径 → 文件首行是表头（App 版本、时区、当刻配置、过滤条件），正文旧→新。
- [ ] 在另存为里点取消 → 不发命令、不产生文件、界面不报错。
- [ ] 悬浮窗与提示窗**没有**清空 / 导出能力（权限只给了设置侧：主窗与它派出的独立日志窗）。

## 11. 已知代价

1. **日志目录跟着 `status_dir` 走**：用户用 `--status-dir` / `UFO_STATUS_DIR` 覆盖时，那些 hook 行落在被覆盖的目录，而配置永远在 AppData 根下（全局唯一）—— 于是主窗面板读不到它们。独立日志窗常驻显示当前日志目录路径就是为了这一条。
2. **默认关 = 出问题时没有现场**：用户得先记得打开。这是明码标价的选择（另一个方向意味着「没人碰过开关却在落提问摘录」），不是遗漏。
3. **常开后的磁盘上限 = `max_total_bytes`**：默认 96 MiB，但**可以被配到 512 MiB**，且回收 ≥5s 一次、活动文件永不删 ⇒ 瞬时可达 `max_total_bytes + max_file_bytes`。所以导出与计数都必须流式/带预算，不许先把结果读进内存。`verbose`（= switch 打开）档体积与敏感度同时上升。
3b. **计数与「按盘分页」是有成本的读**：一次计数 = 把该条件涉及的目录整个翻完（先子串预筛再解析）。靠「3 秒节流 + 4 条 LRU 缓存 + 只在选中会话变化 / head 前进时调」三条兜住，而不是靠把数说成约数。
4. **手动清空会丢在途的少数几条**（见 §9 最后一条）。
5. **导出接受路径**：命令只授权设置侧（主窗与它派出的独立日志窗），且有 `validate_export_target` 那道校验，但它确实是本项目里唯一「按调用方给的地点写文件」的口子。
4. **不做 `sync_all`**：hook 侧为了不把退出拖慢，系统崩溃时可能丢最后几 KB。
5. **不修既有语义**：`order_inbox_batch` 的跨批乱序仍无法识破（没有可靠源序号），日志只是把它**显示**成「后到的低 `received_at` 事件被采纳」—— 是观测，不是修复。
6. **时间口径分家**：文件名用 UTC、UI 显示本地。跨零点的批次整批归入新文件，于是「昨天产生的几条」和「今天的」同处一个文件；读端按文件全量读，不丢。
7. **旧 `hook-debug.log` 不再写**，但磁盘上遗留的旧文件不主动删（避免误删用户手放的东西）—— 想清掉请手工删除 `<status_dir>/hook-debug.log`。
