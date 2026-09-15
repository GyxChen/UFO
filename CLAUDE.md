UI设计请使用:ui-ux-pro-max-skill
窗口UI风格为极简,悬浮窗UI风格为毛玻璃,模糊度0.8,浅灰色背景

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
悬浮窗将来会引入更多的组件,因此悬浮窗的开发提示,需要在我更改悬浮窗的时候需要写明到claude.md中.

### 窗口构成

应用有三个窗口，均在 `src-tauri/tauri.conf.json` 声明；`src/App.vue` 按 label 分派组件：

| label | 用途 | 关键配置 |
|---|---|---|
| `floating` | 悬浮窗（状态灯） | 56×56，transparent，alwaysOnTop |
| `main` | 设置主窗 | 800×600，`visible:false`，按需显示 |
| `tooltip` | 悬浮信息提示窗 | 300×190，`visible:false`，`focusable:false`，点击穿透 |

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
- 展示字段：provider、phase、Endpoint（见「代理解析器」）、`prompt`（用户提问）、token 用量。**不展示 session id**。
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

- 同轮内后续事件继承这两个值，下次提交/新事件才覆盖（`reducer.rs` 用 `.or(previous_*)`）。
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
- 档案结构含 `extensions`（JSON map，本轮恒空），**加拓展字段不改协议**。目前只交付登记/列举/删除 + 右侧占位，tooltip 尚未消费 extensions。
- ⚠️ `normalize_host_port` 早期把 `[::1]:8080` 切成 `[::1:8080`（漏 `]`），会让 loopback 判定失效、本地地址被登记；已由 `normalize_host_port_keeps_ipv6_brackets_and_port` 锁定。
- 主窗 tabbar 走 WAI-ARIA Tabs：roving tabindex + ←/→/Home/End；面板容器加 `.main-window--wide` 以突破 `.ufo-card` 的 `max-w-md` 限宽。
- 读写分离：读命令 `get_endpoint_profiles` 给三窗口；`add/delete_endpoint_profile` 只在 `main-module-settings.json` 授权（仅 main 窗口）。
- ⚠️ 拓展面板的反馈**不能用 `window.alert` / `confirm`**：装了 `tauri-plugin-prevent-default`，原生对话框会被拦；一律走 `role="status"` 行内提示，移除用两步确认。

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
- **新命令注册三处**：`#[tauri::command]` + `permissions/status.toml` 的 `[[permission]]` + capability 文件。`get_session_records` 给三窗口（`default.json`），`delete_session_record`/`clear_session_records` 只在 `main-module-settings.json` 授权（仅 main）。
- ⚠️ 会话记录面板同样**不能用 `window.alert`/`confirm`**（`tauri-plugin-prevent-default`），删除/清除都走两步确认 + `role="status"`。

### Agent 状态指示灯语义
- 🟢 常亮 = Running（正在执行）
- 🟡 = Waiting（等待用户操作：Stop / PermissionRequest / Elicitation / 等待类 Notification）
- 🟢 + 闪烁 = 两种完成态
  - `Stop`：一轮执行完成 → 先绿闪，retention 后落成黄常亮（对话未结束）
  - `SessionEnd`：对话结束 → 绿闪后移除
- 🔴 = Error（StopFailure 等；红灯不粘，任何能映射出别的阶段的事件都可转灯，`Unknown` 事件除外）
- 详情见 `reference/doc/agent-status-hooks.md`
