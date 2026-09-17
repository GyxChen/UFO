UI设计请使用:ui-ux-pro-max-skill
窗口UI风格为极简,悬浮窗UI风格为毛玻璃,模糊度0.8,浅灰色背景

## agent语言
和用户通过中文交流

悬浮窗有三种形态:
1. 方形(默认 56×56,未贴边;贴到四角时变成更大的方形)
2. 横条(上下贴边时,宽度放大)
3. 竖条(左右贴边时,高度放大)

对悬浮窗UI做修改时,必须考虑适配这三种形态:窗口尺寸会随贴边状态变化,布局不能写死固定像素,要保证三种形态下内容都能正常显示。

## 代码注释
项目代码需要适当增加注释
1.每个方法都要描述功能
2.接口和type需要描述字段
3.关键代码写明在干什么

## 全局视觉风格（Matte Panel · 方案 A）
全 App 以深色哑光控制台为准；主窗口已按此落地，后续页面/组件必须复用同一套 token，禁止再散落硬编码灰色系。

### 背景与表面
- 悬浮窗背景：`rgba(61, 61, 61, 0.8)` → CSS `--ufo-bg`（透明毛玻璃）
- 主窗背景：`#3D3D3D` → CSS `--ufo-bg-main`（实色，无透明/blur）
- 卡片表面：`#2A2A2A` → `--ufo-surface`
- 次级表面（列表行等）：`#323232` → `--ufo-surface-2`
- 输入底：`#1F1F1F` → `--ufo-input`
- 边框：`rgba(255,255,255,0.08)` / 强边框 `0.14` → `--ufo-border` / `--ufo-border-strong`
- 圆角：`12px` → `--ufo-radius`

### 窗口透明策略
- 悬浮窗：`transparent: true`；`.floating-shell` 用半透明 `--ufo-bg` + blur（毛玻璃）
- 主窗：`transparent: true` 仅用于圆角裁切；`.main-shell` 用实色 `--ufo-bg-main`（`#3D3D3D`），**不加** backdrop-filter / 不加 alpha
- 主窗：外壳与 html/body/#app 不滚动；仅内部 `.main-window` 滚动，并使用 Matte 细滚动条
- 主窗去边框后需自绘标题栏并调用 `startDragging`；关闭按钮走隐藏到托盘

### 文本与强调
- 主文本：`#F3F4F6` → `--ufo-text`
- 次文本：`#A3A3A3` → `--ufo-muted`
- 强调色（开关开、主按钮、焦点环）：`#34D399` → `--ufo-accent`
- 强调浅底：`rgba(52,211,153,0.16)` → `--ufo-accent-soft`
- 强调上的文字：`#052E1D` → `--ufo-accent-on`
- 危险：`#F87171` → `--ufo-danger`
- 焦点环：`rgba(52,211,153,0.55)` → `--ufo-ring`

### 状态灯色（与指示灯语义一致）
- 绿 `#34D399` / 黄 `#FBBF24` / 红 `#F87171`
  → `--ufo-status-green|yellow|red`

### 交互控件约定
- 开关：灰轨 `#404040` → 开态翠绿填充；拇指白圆
- 输入：方角圆角 10px，focus 时翠绿边 + soft ring
- 主按钮：实心翠绿 + 深绿字；次按钮：描边幽灵
- 字体：Plus Jakarta Sans（`--font-sans`）
- Token 定义见 `src/assets/main.css`（`:root` + `@theme`）
- 预览参考：`reference/mock/main-window-a-matte.html`

## 悬浮窗开发
悬浮窗将来会引入更多的组件,因此悬浮窗的开发提示

### 窗口构成

应用有三个窗口，均在 `src-tauri/tauri.conf.json` 声明；`src/App.vue` 按 label 分派组件：

| label | 用途 | 关键配置 |
|---|---|---|
| `floating` | 悬浮窗（状态灯） | 56×56，transparent，alwaysOnTop |
| `main` | 设置主窗 | 800×600，`visible:false`，按需显示 |
| `tooltip` | 悬浮信息提示窗 | 300×260，`visible:false`，`focusable:false`，点击穿透 |

- `tooltip` 采用「声明 + 隐藏 + 按需显示」，不在运行时创建窗口，因此**不需要** `core:webview:allow-create-webview-window`。
- 点击穿透在 Rust 侧 `setup` 中设置（`set_ignore_cursor_events(true)`），Rust 调用不经过 capability 校验，无需额外权限。
- 新增窗口必须把 label 加入 `src-tauri/capabilities/default.json` 的 `windows` 数组，否则该窗口**完全没有 IPC 权限**（连事件监听都不行）。
- 悬浮窗跨窗口控制 tooltip 是允许的：ACL 校验的是**调用方**窗口的权限，目标 label 只是参数。

### 悬浮窗 hover 信息提示

- 指示灯 `pointerenter`/`pointerleave` → `floatingWindowKey` 上下文的 `onSessionHover(key)` → 悬浮窗壳延迟 250ms 后 `emitTo('tooltip', 'tooltip-hover', payload)`。
- tooltip 自行订阅 `agent-status` 取会话数据，payload 只传「哪个会话 + 悬浮窗外框 + 贴边状态」，避免数据陈旧。
- 定位是纯函数 `placeTooltip`（`src/lib/tooltip.ts`），优先放在悬浮窗**未贴边**的一侧，越界自动换侧。
- 拖拽 / 点击 / 贴边重算时必须 `dismissTooltip()`：`startDragging()` 后指针交给系统，`pointerleave` 可能不再触发。
- tooltip 是独立窗口，**不适用**方形/横条/竖条三形态适配；其尺寸由 `tauri.conf.json` 给出，内容用 flex + 省略号自适应。
- 展示字段（自上而下顺序固定）：provider/phase、Endpoint（见「代理解析器」）、token 用量、**流程脚本拓展字段 + 刷新标注**、`prompt`（用户提问）。拓展字段紧跟 token 行、提问永远在最后（不要插回提问下面，否则与「字段属用量类」的视觉分组错位）。**不展示 session id**。
- ⚠️ **指示灯不要加 `title` 属性**：`title` 会触发 Windows 原生文字提示，与悬浮窗自己的 tooltip 重复且更简陋。无障碍信息只走 `aria-label`（不产生可见提示）。
- ⚠️ tooltip 的 token 单位来自模块配置，因此 tooltip 窗口**必须**在 `App.vue` 中 `floatingModules.start()`；否则它永远停留在 `fallbackConfig` 的默认单位，主窗口切换后不生效。

### token 用量展示

- 数据链路：hook 只透传 `transcript_path` → 常驻 monitor 线程用 `TokenTracker` 增量解析 transcript → `StatusRegistry::set_tokens` → `SessionSnapshot.tokens`。**hook 进程不解析 transcript**（每次事件都要启动一次，必须快进快出）。
- 去重按 `message.id`：同一条 API 响应会写成多条 assistant 记录，每条都带同一份 usage，不去重会翻 2–3 倍。子代理 transcript（`<session-id>/subagents/*.jsonl`）并入同一会话。
- `SessionSnapshot.tokens` 为 `Option<TokenTotals>` + `#[serde(default)]`（**不得升 `SCHEMA_VERSION`**）；它不是事件派生字段，由 `snapshot()` 时合并。
- 单位可配（`module_config.rs` 的 `TokenUnit`）：`tokens` = 原始计数（`1,261,847`），`m_tokens` = 百万（`1.26M`，不足 0.01M 显示 `<0.01M`）。**只在主窗口设置**，悬浮窗只读展示。
- 格式化在 `src/lib/agent-status.ts`：`tokenTotal` / `formatTokenCount` / `formatTokenMillions`。tooltip 里的 token 明细（title）始终用原始计数，不受单位影响。

### 会话展示字段（base_url / prompt）

两个字段都从 hook 侧采集，经 `AgentEvent` → `SessionSnapshot` → 前端 `SessionSnapshot` 传递，均为 `Option<String>` + `#[serde(default)]`（**不得升 `SCHEMA_VERSION`**，否则旧 hook 事件被静默丢弃）。

| 字段 | 来源 | 处理 |
|---|---|---|
| `base_url` | **配置文件优先**：`~/.claude/settings.json` → `<cwd>/.claude/settings.json` → `<cwd>/.claude/settings.local.json` 的 `env.ANTHROPIC_BASE_URL`（后者覆盖前者，`cwd` 取自 hook payload）；**环境变量兜底**：`ANTHROPIC_BASE_URL`。codex 只走环境变量 `OPENAI_BASE_URL` / `OPENAI_API_BASE` | 只按名字取该 key（`env` 块里的 token 绝不读取）；不脱敏，仅 trim；上限 4096 |
| `prompt` | **仅** `UserPromptSubmit` payload 的 `prompt` 字段（`UserPromptExpansion` 的展开模板文本不认，否则会覆盖用户真正的提问） | trim + 截断；上限 4000 字符；同轮内后续事件继承，下次提交才覆盖 |

- 同轮内后续事件继承这两个值，下次提交/新事件才覆盖（`kernel.rs` 用 `.or(previous_*)`）。
- ⚠️ 二者**明文写入** `%APPDATA%/<identifier>/status/inbox/*.json`（约 500ms 内消费删除）。提问可能含密钥或私有代码，这是已知代价。

### 代理解析器（Endpoint 反查上游）

用 cc-switch 这类本地代理时，`base_url` 只是 `127.0.0.1:<port>`，真实上游写在代理自己的配置里。UFO 通过「代理解析器」把本地地址反查成上游 endpoint 显示在 tooltip 的 Endpoint 行。

- 下拉选的是**解析器工具**（`不解析(none)` / `cc-switch` / 将来更多），不是代理内部的 provider。存 `module_config.rs` 的 `proxy_resolver` 字段，旧配置缺字段回落 `none`。
- 命中解析时 `SessionSnapshot.upstream_url` 被填充；tooltip 用 `displayEndpoint`（`src/lib/agent-status.ts`）= `upstream_url ?? base_url`，**上游优先、回落本地**，两者都无才显示 `—`。
- 接口：`ProxyResolver` trait（`proxy/mod.rs`），`build_registry` 里注册一行即接入新工具，下拉与填充逻辑零改动。
- `CcSwitchResolver`：只读打开 `~/.cc-switch/cc-switch.db`，按 `proxy_config` + `providers` 反查。命中要求该 app 行**两列同时为 1**：`proxy_enabled`（全局代理服务器开关，三行镜像同值）且 `enabled`（该 app 是否被接管）——语义来自 cc-switch 源码，只看其一会在「服务器开着但该 app 未接管」时误命中。⚠️ `listen_port` 是 `INTEGER`（非 TEXT），必须按整数读，否则 rusqlite 读整行 `InvalidType` 失败、功能空转。上游取 `is_current=1` provider 的 `env.ANTHROPIC_BASE_URL`；带 5s TTL 缓存。
- ⚠️ `settings_config` 整块 JSON 会进内存解析，但**只按名字提取 `env.ANTHROPIC_BASE_URL` 一个 key**，同块里的 token / api key 一律不读取、不缓存、不上报。Codex 的 base_url 走内嵌 TOML（`config` 字段）而非 `env.ANTHROPIC_BASE_URL`，因此目前只反查 Claude，Codex 会话回落本地地址。
- ⚠️ `upstream_url` 是 `Option<String>` + `#[serde(default)]`，**不得升 `SCHEMA_VERSION`**。
- ⚠️ cc-switch.db 被应用持有：必须 `SQLITE_OPEN_READ_ONLY` + 短 `busy_timeout`，避免争锁。切换 provider 后最多 5s（TTL）反映到悬浮窗。

### 信息卡拓展（按 endpoint 的 tooltip 拓展字段）

tooltip 的拓展字段**跟着 endpoint 走**（同一上游共用一套配置），而会话快照里的 base_url/upstream_url 只在会话存活时存在。因此主窗口顶端加了 tabbar（`悬浮窗设置` / `信息卡拓展` / `会话记录`），第二页用左右主从布局配置各 endpoint。

- 数据源是 `endpoint_profiles.rs` 的持久化档案表（`<app-data>/endpoint-profiles.json`），**不靠会话存活**；monitor 每轮采集存活会话的 `upstream_url ?? base_url` 落盘，前端 `get_endpoint_profiles` + `endpoint-profiles` 事件读取。
- ⚠️ **loopback（127.0.0.1 / localhost / ::1）一律不登记** —— 本地代理地址不是可配置对象，端口还会随重启变。有效 endpoint 口径与前端 `displayEndpoint` 一致：命中解析时登记反查出的**上游**，不是代理监听地址。
- 去重键 `id` = `proxy::endpoint_identity` 归一的 `host:port`；只在出现未见过的 id 时才写盘（同一批 endpoint 每 500ms 都进来）。
- 档案结构含 `extensions`（JSON map），**加拓展字段不改协议**。信息卡拓展的 per-endpoint 配置（流程脚本 `FlowConfig`）就整体存在 `extensions["flow"]` 下，不给 `EndpointProfile` 加字段。
- ⚠️ `normalize_host_port` 早期把 `[::1]:8080` 切成 `[::1:8080`（漏 `]`），会让 loopback 判定失效、本地地址被登记；已由 `normalize_host_port_keeps_ipv6_brackets_and_port` 锁定。
- 主窗 tabbar 走 WAI-ARIA Tabs：roving tabindex + ←/→/Home/End；面板容器加 `.main-window--wide` 以突破 `.ufo-card` 的 `max-w-md` 限宽。
- ⚠️ 「信息卡拓展」/「会话记录」两面板的左右两列**高度各自独立**（grid `items-start`，不让列互相拉伸）；左侧列表卡固定高（`height: clamp(300px,46vh,520px)` + flex 列，列表 `flex-1` 内部滚动），宽度随网格列 flex。别改回 `max-h`：那会让左卡随内容长高、撑开右侧，并把底部「清除全部 / 手动添加」挤出可视区。
- 读写分离：读命令 `get_endpoint_profiles` 给三窗口；`add/delete_endpoint_profile` 只在 `main-module-settings.json` 授权（仅 main 窗口）。
- ⚠️ 拓展面板的反馈**不能用 `window.alert` / `confirm`**：装了 `tauri-plugin-prevent-default`，原生对话框会被拦；一律走 `role="status"` 行内提示，移除用两步确认。
- ⚠️ tick 读配置必须走 `profiles.with_profile(id, …)`（只 clone 该档案的 extensions），**不要**用 `snapshot()`——后者整表 clone 含脚本正文（单份上限 64KB × 64 条），每 500ms 一次是纯浪费。

### 流程脚本（QuickJS 沙盒，per-endpoint）

「信息卡拓展」右侧让用户为某 endpoint 写一段 JS 脚本，返回 `{label,value,color}` 数组，作为 tooltip 信息卡的拓展字段（如查上游余额）。

- **执行方式**：内嵌 QuickJS（`rquickjs`，features 只 `std/parallel/classes/properties`，**刻意不含 `loader`/`dyn-load`** ⇒ 脚本 `import`/`require` 不可用，没有读文件/隐式出网路径）。沙盒三件套：内存帽 2MB + 中断超时 + 无模块加载器。见 `flow/runner.rs`。
- **注入的宿主 API**（唯一能做的事）：`http.get/post/request`（Rust 侧 `ureq` 代发）、`secrets.get`、`cache.get/set`、`format.num/pct/bytes`、只读 `ctx.endpoint/session/lastResult`。`console` 只写内存缓冲、**不广播**（防脚本 `log(secrets.get('K'))` 泄漏）。
- ⚠️ 出网只走 `http`，且 `validate_http_url` 拦住 loopback / 私网 / 链路本地 / CGNAT / 云元数据 / 无点短主机名。固有代价：域名解析到内网 IP 挡不住（不发连接无法预解），文档已注明。
- **为什么把出网收成一个 `http`**（改沙盒能力前先读这段）：脚本是用户原文，安全性完全依赖「他能碰到什么」。QuickJS 本身没有任何 syscall —— 没有 `loader`/`dyn-load` feature 就连 `import`/`require` 都不存在，JS 层也没有 fs/net/child_process 这些宿主接口。因此脚本唯一能「影响外界」的路径，就是我们主动 `globals.set` 进去的那几个闭包。收成一条通道换来三件事：① **地址策略只有一处可管** —— `validate_http_url` 把 loopback/私网/云元数据挡在发起连接之前，若同时开放 socket 或模块加载，端口扫描与 SSRF 根本无从拦起。② **资源可控** —— 单次请求 500ms–15s 超时、响应体 256KB 封顶，配 worker 串行队列，不会出现一个脚本把带宽和内存吃光。③ **可审计、可静默** —— agent 全停 = 零出网（调度只看存活会话），用户能靠「网络有没有动静」判断脚本在干什么；多一条出网路径这条不变量就破了。要加新能力（如读本地文件、走 IPC）必须同样收敛成一个带策略校验的宿主闭包，绝不能给脚本原生网络。
- **调度**：status monitor 每轮（约 500ms）调 `flow.tick`，**只对「当前存活会话涉及的 endpoint」且到点者入队**；口径是「还在 `StatusSnapshot.sessions` 里」，不看 phase（黄/绿/闪烁灯都可能被悬停）。agent 全停 = 零出网。跑在专用 worker 线程、串行执行；结果按**会话 key** 索引广播 `flow-results`，tooltip 零归一逻辑直接查表。
- ⚠️ 广播值类型是 `FlowSessionResult { fields, updated_at_ms, interval_sec }`（不是裸 `FlowField[]`）：tooltip 要在字段下方标注「每 Ns 刷新 · 更新于 HH:MM:SS」，让用户能分辨屏上是刚跑的值还是旧值（脚本挂掉时字段整块不显，更需要时间锚点）。`interval_sec` 由 `FlowJob` 透传进 `EndpointState`，`apply_result` 每次回写都刷新（改间隔不改脚本时不触发 invalidate，标注也要跟上）。格式化在 `src/lib/flow.ts` 的 `formatFlowInterval` / `formatFlowUpdatedAt`；`isFlowSnapshot` 对**整份结果对象**严格校验（缺新鲜度字段的旧形态直接拒绝）。
- ⚠️ **同 endpoint 多会话只跑一次**（`running` 去抖），但结果回填到每个会话 key。连续失败按 `interval × 2^n` 指数退避、封顶 600s（否则脚本挂了还每秒打网）。
- **密钥**：`secrets.get(name)` 按名字从 OS 凭据管理器取（`keyring`，service = `<APP_ID>/flow/<endpoint_id>`）；值不落盘、不进档案、不进日志。⚠️ Windows 凭据管理器**不能按 service 列举**，因此档案 extensions 里存一份 `secret_names` 名字清单——它是删档案时唯一能级联清凭据的依据，清单丢了凭据就成了孤儿。
- ⚠️ `secret_names` 与 `script_hash` 由**服务端权威维护**：`save_endpoint_flow` 会保留旧 `secret_names`、按脚本内容重算 `script_hash`，**不接受**前端传入这两项（前端漏传会清空凭据记账）。
- `script_hash`（FNV-1a，非加密）只用于判断「改没改脚本」：脚本没改才把上次结果作为 `ctx.lastResult` 继承（增量语义），改了就不给（否则语义错位）。
- ⚠️ `parse_fields` 的 `MAX_FIELDS=4` 与 tooltip 高度（`tauri.conf.json` 的 260）绑定：加字段数要同步看窗口尺寸，超出会被 clip。一屏预算（含刷新标注）：head + Endpoint + token + 4 字段 + 标注 + 提问 1 行 ≈ 236px；提问靠 `--compact` 压到 1 行，**不要再给它加行数**，否则标注会被挤出可视区。
- ⚠️ rquickjs 闭包两个坑（已踩过）：① 参数生命周期必须写外层具名 `'js`，不能写 `'_`（否则 E0106 / lifetime may not live long enough）；② 可选参数用 `prelude::Opt` 而非 `Option`——`Option` 没实现 `FromParam`，会被当成**必填**参数。
- ⚠️ release 是 `panic = "abort"`：`flow` 全模块宿主闭包不得 panic（不 unwrap/expect/索引/slice），失败一律变 `Err(String)` 或跳过，否则一次 panic 带走整个 App。
- 前端：编辑器 `EndpointFlowEditor.vue` + `lib/flow.ts` + `stores/flowResults.ts`；`flowResults.start()` **必须在 `App.vue` 调用**（tooltip 要用它，和 tokenUnit 同坑，不启动永远收不到数据）。`run_flow_test` 不进调度队列，日志回显（仅主窗口）；⚠️ 它是 **`async` 命令 + `spawn_blocking`**——非 async 命令跑在主线程，一次测试最坏阻塞 15s 会冻结窗口事件循环；带引用入参的 async 命令按 tauri 宏要求必须返回 `Result`。
- 保存/删除 endpoint 后会**立刻**再广播一次 `flow-results`（字段已被 invalidate 清掉），否则 tooltip 手里仍是旧脚本的字段、要等下次轮询才刷新。
- 新命令注册三处（`#[tauri::command]` + `permissions/status.toml` + capability）：`get_flow_results`/`get_flow_snapshot` 给三窗口（`default.json` 的 `allow-get-flow-results`）；`save_endpoint_flow`/`run_flow_test`/`set_flow_secret`/`delete_flow_secret` 只在 `main-module-settings.json` 授权（仅 main）。

### 会话记录持久化（SQLite，跨重启续档）

会话状态原本完全活在内存（`StatusRegistry` 的 HashMap），重启即丢失，`Completed` 还会按 retention 清掉。后果是同一 `session_id`（如 `claude --resume`）被当成全新会话：token 归零、`started_at_ms` 变现在（灯跳位）、prompt/base_url 要等下次事件。`session_store.rs` 把「跨重启仍有意义」的字段落 `<app-data>/session-records.db`（`rusqlite` bundled，零新增依赖），事件命中存档时回填。主窗口加了第三个 tab「会话记录」管理这些行（列表 + 单条删除 + 清除全部）。

- **回填三字段**：`started_at_ms` / `prompt` / `base_url` 的继承阶梯变成「事件 > 内存前驱 > 存档」，走 `apply_event_with_seed`（seed 仅在内存里没有该会话时生效）。resume 的会话因此保住原灯位（`snapshot()` 按 started 升序）。
- **token 地板**：`set_tokens` 逐字段取 max（`merge_token_floor`）—— transcript 被截断/删除时重扫值小于历史值，直接覆盖会让用户已看到的数字凭空缩小。⚠️ 副作用是**数字只增不减**，与 `tokens.rs` 既有取舍一致。seed 非零时先灌地板，resume 瞬间不必等重扫就显示历史值。
- **只 GUI 侧持有**：hook 二进制必须继续快进快出，**绝不打开** AppData 的 SQLite。
- **两套版本别混**：`SessionRecordList.schema_version` 是线协议，按硬规则**永不递增**（加字段一律 `Option`+`#[serde(default)]`）；`PRAGMA user_version` 是库内布局，加列时递增它配 `ALTER TABLE`。
- **保留策略**（本轮为常量）：`MAX_RECORDS = 200`（超出按 `last_seen` 淘汰最旧，存活会话豁免）+ 启动删 `last_seen` 超 30 天的行。
- ⚠️ **prompt 现在长期明文驻留磁盘**：原先只在 inbox 里约 500ms 就被消费删除，落库后会长期留在 `session-records.db` 且在主窗口「会话记录」页可见。提问可能含密钥/私有代码，这是已知代价（见下）。
- ⚠️ **「清除全部」只删归档、不强清内存**：仍在运行的会话会在下一个 hook 事件后被重新记录（面板文案与详情都按此写明）。要连灯一起清是另一种语义（需 `registry.remove`），本轮按需求选择不这么做。
- **存活标记在前端 join**：列表行的「灯亮着」按 `agentStatus.sessions` 现算，不给存档加存活态字段。
- **Endpoint 展示 = `upstream_url ?? base_url`**（与 tooltip 的 `displayEndpoint` 同一口径）：`base_url` 是会话实际连的地址（用 cc-switch 时就是 `127.0.0.1:<port>`），`upstream_url` 是刷盘那一刻代理解析器反查出的上游。**两者都要存** —— 只存 base_url 会让面板一直显示 localhost；只存上游则丢失「经哪个本地代理转发」的副标注（`recordLocalNote` 用它）。前端读法是 `src/lib/session-records.ts` 的 `recordEndpoint` / `recordLocalNote`。
- ⚠️ `upstream_url` 随 `SessionSnapshot` 一起落库，**不是**回填字段（`StoredSeed` 不含它）：内核的继承阶梯只用 `base_url`，上游由快照生成时按当前解析器现算。库内对应列由 `PRAGMA user_version` 2 迁移补上（v1 用 `ALTER TABLE ADD COLUMN`，老行取 NULL）。
- **新命令注册三处**：`#[tauri::command]` + `permissions/status.toml` 的 `[[permission]]` + capability 文件。`get_session_records` 给三窗口（`default.json`），`delete_session_record`/`clear_session_records` 只在 `main-module-settings.json` 授权（仅 main）。
- ⚠️ 会话记录面板同样**不能用 `window.alert`/`confirm`**（`tauri-plugin-prevent-default`），删除/清除都走两步确认 + `role="status"`。

### 平台兼容层 / 内核层

平台（Claude / Codex / …）的 hook 数据先经「兼容层」归一化，再入「内核」维护灯态；内核不认识任何平台词汇：

```
平台 hook payload ─▸ compat（status/compat/，按 provider 选适配器）─▸ AgentEvent{ kind, … } ─▸ kernel（status/kernel.rs）
```

- **兼容层**：`status/compat/` 定义 `HookAdapter` trait + `adapter_for(provider)` 注册表；`claude.rs` / `codex.rs` 各独占自己平台的全部知识（字段别名、事件名表、base_url 来源、提问提取、子代理 `agent_id`）。跨平台共用的（字段多别名取值、事件名归一化、提问裁剪、`agent_id` 判定…）放 `compat/mod.rs`。
- **内核**：`status/kernel.rs` 只认 `AgentEventKind` 六种语义（Running / TurnFinished / WaitingForUser / SessionFinished / Failed / Unknown），`light_for_kind` 把 kind 映射成灯态 + 是否绿闪。`AgentEvent.event` / `notification_type` 只是诊断字段，**内核不得据此转灯**。
- **接入新平台** = 新增 `compat/<platform>.rs` 实现 `HookAdapter` + 在 `adapter_for()` 加一个 match 分支；内核与前端零改动。
- **事件源目前有两个**：hook（inbox 里的规范 JSON）与 Codex rollout 补漏（`codex_rollout.rs` 合成 `Failed`）。两者都产出 `AgentEvent`，在 monitor 里走**同一条入口** `apply_status_event`（广播调试事件 → 登记 transcript → 取存档回填 → 进内核）。加第三个源时照着接这个函数，别绕开它。
- `AgentEvent.kind` 是 `#[serde(default)]` 新增字段，缺省 `Unknown`（**不得升 `SCHEMA_VERSION`**）；旧事件反序列化后为 Unknown = 不驱灯色，与「无法识别」一致。
- **hook 输入上限 4 MiB**：Codex 的工具事件会把 `tool_input`（apply_patch 补丁等）原样带上，卡 64 KiB 会丢掉有效生命周期事件。落盘的 inbox 事件仍只带提取字段。

### Codex（已接入）

- 12 个事件：`PreToolUse` / `PermissionRequest` / `PostToolUse` / `PreCompact` / `PostCompact` / `SessionStart` / `SessionEnd` / `UserPromptSubmit` / `SubagentStart` / `SubagentStop` / `Stop` / `Interrupt`（实测自 codex.exe v0.155 的 `HOOK_EVENT_NAMES`）。
- 与 Claude 的差异：**没有 `Notification` 事件**（无 `notification_type` 分流）；`Interrupt`（用户 Esc / Ctrl-C 取消当前轮）是 Codex 独有，映射为 **WaitingForUser（黄灯不闪）**——红灯在本项目专指接口/不可抗力错误，用户主动取消不是。`Stop` 仍映射 TurnFinished（绿闪落黄）。
- **子代理同构**：Codex 子代理事件也带 `agent_id`（+`agent_type`）、主线程不带，共用 `session_id` → 内核那条「子代理不驱灯」的守卫对两个平台同样生效。
- **Endpoint**：读 `$CODEX_HOME/config.toml`（默认 `~/.codex/config.toml`）→ `<cwd>/.codex/config.toml` 覆盖，取顶层 `model_provider` 指向的 `[model_providers.<name>].base_url`；环境变量 `OPENAI_BASE_URL` / `OPENAI_API_BASE` 兜底。只读这一个 key，api key（`auth.json`）一律不碰。用 TOML 解析依赖 `basic-toml`（**只读**，刻意不用官方 `toml`，避免连带 toml_edit 序列化链）。
- **token**：rollout JSONL 里 `event_msg`/`token_count` 的 `info.total_token_usage` 是**会话级累计值**（不是增量），同一文件内取 max；同一会话的多个 rollout 各文件累计值相加。⚠️ **口径与 Anthropic 相反**：Codex 的 `input_tokens` 是 prompt 总量、**已含** `cached_input_tokens` / `cache_write_input_tokens`（实测 `total_tokens == input + output`），而 `TokenTotals` 沿用 Anthropic 口径（input 不含缓存）—— 所以映射时必须把缓存从 input 里减出去，否则前端 `tokenTotal()` 会把缓存算两遍（实测缓存占比可达 1/3）。`output_tokens` 已含 `reasoning_output_tokens`，不重复加。
- ⚠️ **红灯在 Codex 上靠 rollout 补漏，不是 hook**：Codex 的 12 个 hook 事件里**没有错误类事件**（Claude 有 `StopFailure`），实测「重试耗尽」（429）这类以 error 结束的轮次**连 `Stop` 都不触发** —— 只用 hook 的话红灯永远不可达，灯会停在绿灯直到超时被清。因此 `status/codex_rollout.rs` 增量巡检 rollout：读到 `event_msg`/`task_complete` 带非空 `error` 就合成一条 `Failed` 事件（`error_code` 带错误正文）走同一条内核入口。它**只巡检已登记会话**（hook 的 `transcript_path` 带过的），agent 全停即零 I/O；按 `turn_id` 去重（`event_id` 也由轮 id 派生，吃内核那层去重）。⚠️ 已知时序窗口：rollout 落盘比真实发生晚最多一个巡检周期（~500ms），若用户在这窗口内已发下一条 prompt，红灯可能短暂盖住新绿灯，下一条 hook 事件会纠正。
- ⚠️ **`tokens.rs` 的解析必须按 provider 分流**（`observe()` 要传 provider）：Claude 是「按请求累加 + 按 `message.id` 去重」，Codex 是「取累计值」，混用会让数字翻倍或归零。

### Agent 状态指示灯语义
- 🟢 常亮 = Running（正在执行）
- 🟡 = Waiting（等待用户操作：Stop / PermissionRequest / Elicitation / 等待类 Notification）
- 🟢 + 闪烁 = 两种完成态
  - `Stop`：一轮执行完成 → 先绿闪，retention 后落成黄常亮（对话未结束）
  - `SessionEnd`：对话结束 → 绿闪后移除
- 🔴 = Error（StopFailure 等；红灯不粘，任何能映射出别的阶段的事件都可转灯，`Unknown` 事件除外）
- ⚠️ **子代理（subagent）事件不得驱动灯色**：子代理事件与主会话**共用同一个 `session_id`**（实测确认），只有 payload 里的 `agent_id` 能区分（主线程事件不带该字段）。带 `agent_id` 的事件对已存活会话**只刷新 `last_seen_at_ms`**，阶段 / `is_flashing` / `remove_at_ms` 全原样保留。否则主 agent 弹出提问框（`PermissionRequest`→黄）时，后台仍在跑的子代理事件会立刻把它顶回 Running —— 表现为「Claude 已经在问我了，灯却一直是绿色」。**改内核（`kernel.rs`）的 `apply_event_with_seed` 时务必保住这条守卫。**
- ⚠️ **`AskUserQuestion`（选择框提问）不走 `PreToolUse`**：它走 `PermissionRequest` + `Notification(permission_prompt)`，作答后发 `PostToolUse`。不需要按工具名特判。
- ⚠️ `notification_type` 的「等人操作」清单必须与 Claude Code 枚举对齐（详见 `reference/doc/agent-status-hooks.md`），**漏列会退化成绿色**。
- 详情见 `reference/doc/agent-status-hooks.md`
