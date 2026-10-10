# UFO · Agent 指南

Tauri 2 + Vue 3 + Tailwind v4 的桌面 App。UI 设计走 `ui-ux-pro-max` skill；和用户用中文交流；具体色值与模糊度以 `reference/doc/rule/visual-style.md` 为准（那份已对照代码校准）。

## 项目概览

UFO 是一块**可拼装的悬浮窗**：显示 Claude / Codex / Qoder 等 agent 的运行状态灯，悬停展示 endpoint、token 用量与流程脚本拓展字段；主窗负责配置。

- 五个窗口（`src-tauri/tauri.conf.json`，一律「声明 + 隐藏 + 按需显示」，不在运行时建窗）：`floating` 状态灯（默认 45×45、透明、alwaysOnTop）、`main` 设置主窗（实色哑光）、`tooltip` 悬停信息窗（`focusable: false` + 点击穿透）、`script-editor` 整窗编辑脚本、`status-log` 转灯日志完整链路。
- 后两个是主窗派出去的**独立面板窗**：外壳 `components/ui/PanelWindow.vue`（圆角实色面板 + 与主窗同版式的标题栏：左端 UFO 图标 + 名字 + 本窗标题，右端 × 关闭），来回协议 `lib/panel-window.ts`，视图登记 `src/windows/index.ts`，label 与声明 / 权限的同源护栏 `src/windows/panel-windows.spec.ts`。它们不挂模块格子，因此**不参与 `activateModules`**（见悬浮窗 guide §9.1）。
- 悬浮窗是容器，组件（模块）同级共享同一块窗：注册表在 `src/modules/index.ts`，**外壳不认识任何模块**。
- 组件包（插件）是第三层：宿主侧基建在 `src/lib/plugin-*.ts`、`src/stores/plugins.ts`、`src/components/plugins/**` 与 Rust `src-tauri/src/plugin/`。这些文件里**同样不许出现某个包的 id 或专有字段**，一切从安装记录与清单来；作者侧模板与产物在 `reference/plugin-template/`，契约与能力边界见 `reference/doc/guide/plugin-guide.md`；宿主能力面变了（新命名空间 / 新方法 / 新权限名 / 入口多读一个字段）就跑 `/create-plugin-template` 刷新模板那份 `CAPABILITIES.md` 与清单版本号，`plugin-template-doc.spec.ts` 就是「该刷新了」的旗标。
- 悬浮窗三形态：方形（默认未贴边）/ 横条（上下贴边、宽度放大）/ 竖条（左右贴边、高度放大）。窗口尺寸随贴边状态变化，布局不得写死固定像素。
- Rust 与前端同构：`src-tauri/src/floating/` = 外壳，`src-tauri/src/modules/<id>/` = 组件，组合根 `lib.rs` 点名登记钩子。

### 文档地图（细则的唯一落点，动相应模块前先读）

| 主题 | 文档 |
| --- | --- |
| 悬浮窗外壳 / 模块契约 / 格子与缩放 / Rust 侧 / 新增组件步骤 | `reference/doc/guide/floating-module-guide.md` |
| 视觉 token、主窗导航与版面（tips 角标、两级 tab、`fill` 高度链） | `reference/doc/rule/visual-style.md` |
| 平台 hook 契约、灯态语义与子代理红线、Codex rollout | `reference/doc/rule/agent-status-hooks.md` |
| 会话数据链：token / base_url / prompt 采集、会话存档 | `reference/doc/rule/agent-status-session-data.md` |
| Endpoint 档案、代理解析器、QuickJS 流程脚本沙盒 | `reference/doc/rule/agent-status-endpoint-flow.md` |
| 接入一个新平台（改动清单与验收） | `reference/doc/guide/agent-status-platform-onboarding-guide.md` |
| 组件包（插件）：清单契约 / 宿主能力边界 / 作者侧构建与热重载 | `reference/doc/guide/plugin-guide.md` |
| 转灯日志 | `reference/doc/guide/agent-status-log-guide.md` |
| 消息推送 | `reference/doc/guide/agent-status-push-guide.md` |
| 状态机全景图（`/draw` 生成） | `reference/doc/agent-status-state-machine.html` |

分层规则：本文件只写**稳定规则与指针**，不复制细则；冲突时以**代码**为准（行号会漂，符号名不会）。`CLAUDE.md` 现在只有一行导入，不再有需要同步的第二份正文。

## 命令

- `pnpm tauri dev` 起真窗口开发（`pnpm dev` 只跑前端 + devtools）
- `npx vitest run` 前端单测；`pnpm build` = `vue-tsc --noEmit` + `vite build`
- `pnpm check`、`cargo test --manifest-path src-tauri/Cargo.toml` Rust 侧
- `pnpm tauri build` 发布构建（release 是 `panic = "abort"`）
- `pnpm bump` 版本号（替换键全按 `package.json` 的旧值取，见 `bump-version.cjs`）
- hook 入口是 `main.rs` 的 `--status-hook` 分支，快进快出

**提交**：只有用户明确说「提交 / commit」或调 `/commit` 时才提交；格式照 `.agents/skills/commit/SKILL.md`（中文一段话摘要、标题带 `[u]`、无署名）；只提交、不 push / 不 amend / 不 force；提交前先跑本次改动相关的 `npx vitest run` 与 `pnpm build`，红了不交；一次改动一个 commit，工作区没改动不空提交。

## 代码规范

- **注释**：每个方法写功能；接口 / type 描述字段；关键代码写明在干什么。
- **外壳与模块的边界**：`src/App.vue`、`src/components/floating/**`、`src/lib/floating-*`、`src/stores/floatingModules.ts`、`src-tauri/src/floating/**` 里**不得出现模块 id 或模块专有字段名**；模块的一切（视图 / lib / store / 样式 / 设置分区）收在 `src/modules/<id>/`，跨目录引用写 `@/modules/<id>/…`。验收 rg 见 guide §1。
- **模块 ↔ 外壳只走公开契约**：注入的 `floatingModuleKey`（`allocatedCells` / `reportHover`）、注册表定义字段（`demand` / `settings.sections` / `tooltip` / `activate`）、外壳公开的 store。接新组件**不改 `App.vue`**。
- **配置下放**：外壳配置只留 `modules[].enabled|order` + 窗口级 `cell_px`；其余字段一律进各模块自己的 `module_settings[<id>]`，字段名 / 默认值 / 取值范围只写在模块的 `settings.ts` 与 Rust `settings.rs`，逐字段宽松夹取（坏一个只影响它自己）。
- **样式**：颜色 / 圆角 / 字体一律复用 token，禁止新增硬编码灰色系；模块样式写在 `src/modules/<id>/<id>.css`（独立 CSS 里用 `@apply` 必须先 `@reference`）；写死的设计像素要乘 `--floating-cell-scale`，槽内尺寸用 cq 单位。
- **界面**：解释性 tips 走问号角标 `ui/SettingsHint.vue`；「当前是什么状态」的读数必须常驻，不许把读数藏进 hover；组件名已由两级 tab 表达，页面标题用 `sr-only`。
- **Tauri 接线**：新增命令三处注册（`#[tauri::command]` + `permissions/*.toml` + capability），读命令给所有窗口、写命令只给设置侧（`main` 与它派出的面板窗）；新增窗口要把 label 加进 `capabilities/default.json` 的 `windows`（要写命令的再加 `main-module-settings.json`），并在 `src/windows/index.ts` 登记视图。要读盘的命令一律 `async` + `spawn_blocking`。
- **护栏**：前端文案 / 常量与 Rust 同源的地方保住 `?raw` 双向断言；改硬约定（格子、单位、样式权重、行高）就补对应 spec。
- 悬浮窗 UI 改动必须在方形 / 四角 / 横条 / 竖条下都验证过才算完成。

## 禁区

- **线协议 `SCHEMA_VERSION` 永不递增**：加字段一律 `Option<T>` + `#[serde(default)]`，否则旧事件被静默丢弃。`PRAGMA user_version` 是库内布局，另一套，可以递增 —— 别混。
- **hook 进程必须快进快出**：不解析 transcript、不开 AppData 的 SQLite、不出网；重活留在 monitor 线程。
- **配置只按名字取需要的那个 key**：`settings_config` / `config.toml` 只读 endpoint 一个键，`auth.json` 与 `env` 里的 token / api key 一律不读取、不缓存、不上报；脚本密钥只在 `secrets.get` 闭包里按名取，值不落盘、不进档案、不进日志。
- **沙盒不得扩能力**：脚本没有 fs / net / child_process，也没有模块加载器；出网只走注入的 `http`，且必过 `validate_http_url`。「域名解析到内网 IP 挡不住」是已注明的固有代价，不要再开第二条出网路径。
- **`panic = "abort"` 下宿主闭包不得 panic**：不 unwrap / expect / 索引 / slice，失败一律变 `Err(String)` 或跳过。
- **路径与文件名不接受未经校验的输入**：贯穿键（`trace_id`）由 hook 生成，不许拿 payload 可控的值拼路径；导出必须过 `validate_export_target`；`read_light_sound` 只认白名单文件名，否则就是一条任意文件读取。
- **明文落盘是已知代价，必须写清**：`prompt` 会长期留在 inbox / `session-records.db` / endpoint 档案；推送的四项内容（endpoint / prompt / script / session_id）默认全不勾，且勾选处必须写明风险。
- **灯态判定只在内核**：不得据 `AgentEvent.event` / `notification_type` 转灯；子代理事件不得把主线程「等用户操作」的黄灯顶回绿；两条事件源不得同时驱动同一会话（Codex 只配 `PermissionRequest`）。
- **需求汇总不得成环**：`demand(layout)` 只能依赖模块自身数据与传入形态，不许反过来读 `allocatedCells`；想用 `demand` 返回 0 来「不占空间」是错的（0 至少按 1 格算）。
- **尺寸口径是两处同步的约定**：`FLOATING_BASE_CELL_PX` 与 `tauri.conf.json` 的初值必须同为 45；`MAX_FIELDS` 与提示窗高度绑定，加字段要重算一屏预算。
- **不阻塞 UI 线程**：非 `async` 命令跑在主线程；`window.alert` / `confirm` 会被 `tauri-plugin-prevent-default` 拦掉，反馈一律走行内 `role="status"` + 两步确认。
- **指示灯不加 `title`**：Windows 原生文字提示会与自绘 tooltip 重复，无障碍信息只走 `aria-label`。
- `src/assets/bgm/` 的三段 mp3 用 `include_bytes!` 编进二进制，删掉直接编译失败；`additionalBrowserArgs` 会覆盖 wry 的默认值，改它必须把原来的 `--disable-features=...` 一起写回。
- 不 push、不 amend、不 force、不 `reset --hard`；不动用户的本地数据文件（要清理就先移到 `%TEMP%` 备份）。
