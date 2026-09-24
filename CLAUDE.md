UI设计请使用:ui-ux-pro-max-skill
窗口UI风格为极简,悬浮窗UI风格为毛玻璃,模糊度0.8,浅灰色背景

## agent语言
和用户通过中文交流

悬浮窗有三种形态:
1. 方形(默认 45×45,未贴边;贴到四角时各模块竖向堆叠成更大的多行区块)
2. 横条(上下贴边时,宽度放大)
3. 竖条(左右贴边时,高度放大)

对悬浮窗UI做修改时,必须考虑适配这三种形态:窗口尺寸会随贴边状态变化,布局不能写死固定像素,要保证三种形态下内容都能正常显示。

## 代码注释
项目代码需要适当增加注释
1.每个方法都要描述功能
2.接口和type需要描述字段
3.关键代码写明在干什么

## 提交约定（仅用户明确要求时）
- **只有用户明确说「提交」「commit」「整理成一次提交」「写提交信息」，或显式调用 `/commit` 时才执行提交**；完成改动、通过测试或看到工作区有变化时，不得自动提交。
- 用户明确要求提交后，格式与摘要写法照 `.agents/skills/commit/SKILL.md`（一段话中文摘要、标题带 `[u]` 前缀、末尾不加任何署名）。
- 只提交：不 push、不 amend、不 force、不 `reset --hard`；只有用户明确要求时才动远端。
- 提交前先跑本次改动相关的验证（`npx vitest run`、`npm run build`）；红了就先修，不要把坏状态提交进去。
- 一次改动一个 commit，不把不相关的改动混在一起；工作区没有改动时不要空提交。

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
- 滚动条槽位：所有可滚动容器（列表 / 详情面板 / 日志 / JS 编辑框）都必须带 `scrollbar-gutter: stable`，滚动条出现时不得挤占内容宽度；JS 编辑框另把槽宽写进 `--jse-gutter`，让高亮层与输入层的内容盒重合
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
- 预览参考：`reference/mock/main-window/main-window-a-matte.html`

## 悬浮窗开发
悬浮窗将来会引入更多的组件,因此悬浮窗的开发提示
新增组件前先读 `reference/doc/floating-module-guide.md`（组件开发规范：外壳/组件契约、目录归属、配置迁移、验收清单）。

### 窗口构成

应用有三个窗口，均在 `src-tauri/tauri.conf.json` 声明；`src/App.vue` 按 label 分派组件：

| label | 用途 | 关键配置 |
|---|---|---|
| `floating` | 悬浮窗（状态灯） | 初始 45×45（= 一格，见「格子大小」），transparent，alwaysOnTop |
| `main` | 设置主窗 | 1000×750，`visible:false`，按需显示 |
| `tooltip` | 悬浮信息提示窗 | 默认 300×260（组件可用 `tooltip.size` 覆盖），`visible:false`，`focusable:false`，点击穿透 |

- `tooltip` 采用「声明 + 隐藏 + 按需显示」，不在运行时创建窗口，因此**不需要** `core:webview:allow-create-webview-window`。
- 点击穿透在 Rust 侧 `setup` 中设置（`set_ignore_cursor_events(true)`），Rust 调用不经过 capability 校验，无需额外权限。
- 新增窗口必须把 label 加入 `src-tauri/capabilities/default.json` 的 `windows` 数组，否则该窗口**完全没有 IPC 权限**（连事件监听都不行）。
- 悬浮窗跨窗口控制 tooltip 是允许的：ACL 校验的是**调用方**窗口的权限，目标 label 只是参数。

### 悬浮窗模块化（外壳 / 宿主 / 槽位）

悬浮窗不是「Agent 状态灯专用窗」，而是一块可拼装的容器：组件（模块）同级共享同一块窗口，彼此不知道对方存在。

```
src/
├── App.vue                            # 应用外壳：按 label 分派三窗 + 调 activateModules（不认识任何模块）
├── components/floating/               # 悬浮窗外壳（容器侧，零模块语义）
│   ├── FloatingWindow.vue             # 外壳：拖拽 / 贴边 / 形态 / 窗口尺寸 / 提示窗路由 / 点组件开设置页
│   ├── FloatingModuleHost.vue         # 宿主：挂载启用模块，汇总格子需求 → 窗口尺寸，按需求分槽
│   ├── FloatingModuleSlot.vue         # 槽位：注入 floatingModuleKey，并在根节点标 data-module-id
│   ├── FloatingModulesPanel.vue       # 主窗「悬浮窗设置」页：列表（开关 / 齿轮 / 排序）+ 三形态预览
│   ├── FloatingShapePreview.vue       # 三形态预览：按真实比例等比缩放，色块按需求比例分槽
│   └── tooltip/
│       ├── TooltipRow.vue             # 信息卡的通用行（标签 + 值 + 语义色），各组件复用
│       └── TooltipSection.vue         # 信息卡的通用分组容器（顶部强分隔线）
├── modules/                           # 悬浮窗组件：垂直切片，定义 / 视图 / 数据 / 样式都收在自己目录里
│   ├── index.ts                       # 模块注册表：新增组件只改这一行
│   └── agent-status/
│       ├── index.ts                   # 模块定义（id / label / 形态 / component / demand / settings / tooltip / activate）
│       ├── AgentStatusModule.vue      # 模块视图（按 layout-mode prop 适配四种形态）
│       ├── AgentStatusTooltip.vue     # 模块的悬停信息卡内容（可选）
│       ├── demand.ts                  # 模块的格子需求函数（声明式，宿主不挂载也能量窗口）
│       ├── agent-status.css           # 模块自己的样式（从 main.css 切出，见「样式归属」）
│       ├── sections/                  # 模块的设置页分区（可选）：一个分区 = 二级 tabbar 的一项
│       │   ├── GeneralSection.vue     # 常规：行为 + 窗口格子（两块短表单卡片横向并排）
│       │   ├── ExtensionsSection.vue  # 信息卡拓展（fill：占满高度，左右各自滚）
│       │   └── RecordsSection.vue     # 会话记录（同上）
│       ├── panels/                    # 分区里的大面板：endpoint 拓展 / 流程脚本编辑器 / 会话记录
│       ├── lib/                       # 模块自己的纯逻辑（含 spec）：状态灯模型 / 灯位布局 / endpoint 档案 / 会话档案 / 流程脚本
│       ├── settings.ts                # 模块自己的配置块：字段名 / 默认值 / 取值范围只在这里解释（含 spec）
│       └── stores/                    # 模块自己的 pinia store：会话快照 / endpoint 档案 / 会话档案 / 脚本结果
├── components/{MainWindow,TooltipWindow}.vue   # 另外两个窗口（外壳侧）
├── components/ui/                     # 公共控件（按钮 / 输入 / 开关 / 下拉 / 分段 / tab / 脚本编辑器）
├── lib/                               # 外壳与公共能力：floating-{layout,modules,module-context,bridge} / tooltip / snap / window / js-editor / js-highlight
└── stores/floatingModules.ts          # 悬浮窗配置（组件开关 / 顺序 / 形态上限 / 各组件配置块）：窗口级数据，属于外壳
```

配套的纯逻辑：`src/lib/floating-layout.ts`（形态判定 + 窗口尺寸口径）、`src/lib/floating-modules.ts`（模块定义类型 + 需求汇总 + 生命周期启动 `activateModules`）、`src/lib/floating-bridge.ts`（悬浮窗 ↔ 主窗的跨窗口事件契约）。

Rust 侧的对应结构见下文「Rust 侧结构」一节（组件在 `src-tauri/src/modules/<id>/`，外壳在 `src-tauri/src/floating/`）。

模块与宿主之间的契约（`src/lib/floating-module-context.ts` 的 `floatingModuleKey`）：

| 契约 | 方向 | 说明 |
|---|---|---|
| `demand(layout)`（定义字段） | 模块 → 宿主 | 声明本模块在某形态下要占几格；**声明式**是关键：宿主不挂载它也能算出任意形态的窗口大小（主窗的三形态预览就靠这个）。⚠️ 只能依赖模块自身数据与传入形态，不得反过来依赖 `allocatedCells`（会形成回路） |
| `settings.sections`（定义字段） | 模块 → 主窗 | 可选的设置页分区清单（`{ id, label, component, fill? }`）：组件列表行的齿轮、点悬浮窗上该组件都打开它。分区渲染成组件页顶部的二级 tabbar；**只给一个分区就不渲染二级**；不给 `settings` = 没有可配置项，列表里不显示齿轮。⚠️ 按**版面**切分区，不要「一个配置项一个 tab」：短表单合在一页，只有需要占满高度 / 各自滚动的列表编辑器才单独占一页并标 `fill: true`（见「信息卡拓展」节的高度链） |
| `reportHover(key)` | 模块 → 宿主 | 上报悬停对象（模块自定义 key，如会话 key），null = 离开；槽位会补上 moduleId，模块不需要知道自己的 id |
| `allocatedCells` | 宿主 → 模块 | 本模块实际分到的格子数（按需求比例切分窗口，至少 1 格），网格类模块据此决定列行 |
| `layout-mode` prop | 宿主 → 模块 | 当前形态，模块必须同时适配方形 / 四角 / 横条 / 竖条 |
| `tooltip`（定义字段） | 模块 → 提示窗 | 可选的悬停信息卡：内容组件 + `size`（可选，覆盖提示窗默认尺寸；也可给**按悬停目标算的函数** —— 内容行数随目标变化时窗口跟着长高，外壳只负责把高度夹进工作区）+ `hasContent(target)` 就绪判定；不给就悬停不弹窗 |
| `activate()`（定义字段） | 模块 → 外壳 | 可选的模块生命周期：外壳在每个窗口挂载时调一次（`activateModules`，卸载时逆序调返回值）。模块在这里启动自己的 store / 订阅跨窗口事件 —— **外壳不认识任何模块的数据源**，所以「谁需要先 start」只写在模块自己身上。⚠️ 不能改成组件 `onMounted`：提示窗的 `tooltip.hasContent` 在内容组件挂载**之前**就要答案，数据晚到一步就永远弹不出来 |
| `module_settings[<id>]`（配置块） | 模块 ↔ 外壳 | 模块自己的配置：外壳按 id 存一块 JSON（`FloatingModuleConfig.module_settings`），**不解释里面任何字段**。模块用 `useFloatingModulesStore().moduleSettings(id)` / `updateModuleSettings(id, patch)` 读写，字段名 / 默认值 / 取值范围写在自己的 `settings.ts`（如 `src/modules/agent-status/settings.ts`）。Rust 侧同理：外壳整块透传，模块在 `src-tauri/src/modules/agent_status/settings.rs` 里解析 / 认领旧字段 / 同步运行时 |

窗口尺寸的汇总规则（`moduleDemand` / `resolveWindowCells` 在 `src/lib/floating-modules.ts`，尺寸换算 `floatingWindowSize` 在 `src/lib/floating-layout.ts`，全是纯函数 + 单元测试）：

- **总格子数 = 各启用模块需求之和**，夹在 `[1, ABS_MAX_CELLS(16)]` 之间（供各模块按比例分格子用）。**但窗口的像素网格不总是这个数**：边条按个数精确取长度；方形未贴边恒 1 格；**贴角让每个模块各占满一整行竖向堆叠**（`windowGrid('corner', demands)`：行数 = 各模块行数之和、列数 = 最宽模块列数）—— 所以 agent 在四角是 1×2 时，todo / weather 也各是 1×2，三模块整窗就是 3 行 × 2 列（3×2），而不是把需求总和 6 归档成 2×3 的方格。单模块（如只有 agent）仍按它自己的阶梯网格（1×1 / 1×2 / 2×2 / 3×3…）。窗口因此随需求之和变大 —— Agent 吃到 3×3 再叠几个同级组件，整窗会在四角长到「3+ 行 × 3 列」容下它们。**格子上限不再是外壳配置**：各模块在自己的 `module_settings[<id>]` 里声明（Agent 是 `max_cells`，滑块 `{1,2,3,4,6,9}` 封顶 3×3，只限本模块）。
- 宿主用 grid 的 `fr` 分槽：**横条左右并排，竖条 / 方形 / 四角上下堆叠**；横条 / 竖条按需求（面积）分，四角逐个模块按**它自己占几行**分（`cellsGrid('corner', demand).rows`）。
- 到顶（需求之和 > `ABS_MAX_CELLS`）后窗口不再变大，各模块按比例等分收缩；模块要自行处理「分到的比需求的少」，Agent 状态灯退化成 `+N` 溢出格。
- `supportedLayouts` 不含当前形态的模块不挂载、也不参与汇总（等于自动让位）。
- 想让模块在某形态下不占空间就用主窗口的开关关掉它，不要让 `demand` 返回 0（0 至少按 1 格算）。
- ⚠️ 未贴边的方形固定「1 格」（尺寸与格子数无关），所以多组件时内容是**等比压缩**；只有贴角 / 贴边的窗口才按格子数放大。`FLOATING_BASE_CELL_PX` 与 tauri.conf.json 的初值必须同为 45 —— 那是**默认**一格，用户可在「悬浮窗设置」里改（见下条）。
- ✅ 模块按**槽位实际像素**自适应：`.floating-module-slot` 由外壳声明为 size 容器（`container-name: floating-slot`），
  组件用 `@container floating-slot (...)` 做降级 / 放大、用 `cqh` / `cqw` 写随槽位缩放的尺寸；
  ⚠️ cq 单位**只能写在 @container 块内**（块外没有槽位容器时会退化成视口单位）。
  默认一格 45px：只有外壳留 1px 描边 + `p-0.5` 内边距当呼吸位，宿主与组件一律不留 padding（可用内容 ≈39.5px）；格子大小可调后，模块更要靠 cqh/cqw 而不是写死像素。
- ⚠️ **百分比要取「限制轴」的**：窗口宽高不等时（竖条只有一格宽、行高≈一格），百分比只有落在**更短的轴**上才放得下。
  Agent 状态灯改为「灯点吃满格子 + 平台图标缩成右下角徽章」：灯点取**限制轴**的 80%（四周留约 10% margin），图标是绝对定位在右下角的小圆徽章（`width: 40%`，深色底），叠在灯点上而不并排 —— 省下的面积全还给状态灯，横条（上下贴边、纵向最紧）收益最明显。
  竖条窗口只有一格宽：行宽（45 格时 39.5px）比行高（43.2px）更窄，所以 `.agent-status-module--vertical` 把灯点改成按**行宽**取百分比（`width: 80%`）才放得下；方形 / 四角 / 横条的行宽 ≥ 行高，按行高（`height: 80%`）算本来就不会溢出。徽章绝对定位、按宽度取百分比（一格在三种形态下宽度都 ≈ 一格边长，徽章大小因此一致），`right + width ≤ 100%` 保证不溢出格子。
  降级查询覆盖两条轴：`max-height` 管横条 / 方形，`max-width` 管竖条（拆成两个 `@container` 写，不用 `or`）；槽位矮于 / 窄于 32px 时藏掉徽章、把灯点放大到吃满格子（灯色优先）。
  护栏：`src/modules/agent-status/agent-status-css.spec.ts`（灯点 ≥ 70% 且大于徽章、徽章绝对定位且不溢出、竖条按行宽、窄槽藏徽章）。
- ⚠️ **写死的设计像素必须跟着格子缩放**：宿主在 `.floating-module-host` 上声明两个变量 ——
  `--floating-cell-px`（一格实际多少像素）与 `--floating-cell-scale`（相对设计基准 `FLOATING_BASE_CELL_PX` 的倍率，1 = 设计尺寸，由 `FloatingModuleHost.vue` 按配置 `cell_px` 算出）。
  于是模块把字号 / 描边 / 光晕 / 图标上限 / 间隙都写成 `calc(<设计像素> * var(--floating-cell-scale, 1))`；
  变量缺失时回落 1（= 设计尺寸），与「没有槽位容器时 cq 单位退化、clamp 取到设计上限」的表现一致。
  ⚠️ 不乘倍率的两类：`@container` 里的 px 断点（那是**绝对可读性阈值**：槽位矮于 32/33px 就降级排版，与设计缩放无关）、以及信息卡（另一个固定尺寸的窗口，不参与格子缩放）。
  护栏：`src/components/floating/floating-module-scaling.spec.ts` 扫描所有模块样式，`clamp()` 的上限只要是 px 设计值就必须引用该变量。
- **一格边长可调**（主窗「悬浮窗设置」页左列、组件列表下方的「格子大小」卡）：配置字段 `cell_px`（物理像素，32–96，默认 45）。它是**整个悬浮窗**的尺寸基准、同级组件共享，也是外壳仅剩的窗口级尺寸参数，因此留在外壳（`FloatingModuleConfig.cell_px`，`#[serde(default = "default_cell_px")]`；⚠️ 加这个字段**不得**升 `SCHEMA_VERSION`）。前端读配置一律走 `configCellPx()` / `cellSizeOf()`（`floating-modules.ts` / `floating-layout.ts`），不要直接取字段。
  - ⚠️ 滑块**拖动时只改本地值**（预览实时跟随），**松手（change）才落盘**：否则每动一像素就要写一次盘，并让真窗口重新吸附一次。
  - 悬浮窗侧 `baseSize` 取自 store（`FloatingWindow.vue`），配置事件到达后 `scheduleSnap()` 按新尺寸重算 → 贴边中也会就地更新。
  - ⚠️ 原生 range 会把越界值截到边界，所以「落盘前 clamp」这条防线主要挡程序化调用（0 会让窗口塌成一条线），由 `clampCellPx()` 与 Rust 侧 `validate()` 各管一半。
- 各形态尺寸由纯函数 `runtimeSnapshot(current, config, definitions, layoutMaxFor)` 一次算全（`{ layout, byLayout }`）：悬浮窗广播它，主窗的列表徽标 + 三形态预览都读它。跨窗口事件在 `src/lib/floating-bridge.ts`（`FLOATING_RUNTIME_EVENT` 广播 / `FLOATING_RUNTIME_REQUEST_EVENT` 补发请求——主窗挂载时可能已错过启动广播）。

新增一个组件 = 6 步（前 3 步必做，第 4 / 5 步按需，第 6 步是验证）：

1. 建目录 `src/modules/<id>/`（视图、`lib/`、`stores/`、样式都放这里），写模块视图 + `index.ts` 导出 `FloatingModuleDefinition`（id 只允许字母 / 数字 / 短横线，后端会校验）。
2. 在 `modules/index.ts` 的 `FLOATING_MODULE_DEFINITIONS` 里加一条 —— 主窗列表、悬浮窗宿主、提示窗分派都读它，别的文件不用动。
3. 模块里 `inject(floatingModuleKey, null)` 拿 `allocatedCells` / `reportHover`（照抄 `AgentStatusModule` 即可）；按形态算的格子数写成定义里的 `demand(layout)`（照抄 `demand.ts`），`supportedLayouts` 按需要写窄。
4. （按需）`settings.sections` 给设置页（一个分区 = 二级 tab 的一项，照抄 `src/modules/agent-status/sections/`；组件自己的配置项读写走模块自己的 `settings.ts` 与外壳 store 的 `moduleSettings` / `updateModuleSettings`）、`tooltip` 给悬停信息卡。信息卡 = 内容组件 + `hasContent`（数据没到齐就返回 false，避免先弹空框）；行用 `components/floating/tooltip/TooltipRow.vue`、分组用 `TooltipSection.vue`，装不下就在 `tooltip.size` 里加尺寸。提示窗按 `moduleId` 分派，不认识任何模块语义，**不用改提示窗**。
5. （按需）`activate()` 给模块生命周期：要订阅跨窗口事件 / 起自己的 store 就写在这里（外壳按注册表调用，见上表）。**不要**改 `App.vue`。
6. 跑 `npx vitest run`（`src/components/floating/floating-module-host.spec.ts` 锁同级挂载 + 汇总 + 悬停转发，只用假模块；`src/lib/floating-modules.spec.ts` / `src/lib/floating-layout.spec.ts` 锁需求汇总、尺寸口径与 `activateModules`；`src/lib/tooltip.spec.ts` 锁信息卡分派；模块自己的集成验证放 `src/modules/<id>/*.spec.ts`）。

其它约定：

- 模块 id 后端**没有白名单**（只校验长度 / 字符集）；配置里没出现的模块按定义里的 `defaultEnabled` 兜底，新模块不需要迁移配置。
- 主窗第一页「悬浮窗设置」（方案 C：左列表 + 右三形态实时预览）**由注册表直接渲染**（`v-for` 遍历 `FLOATING_MODULE_DEFINITIONS`，名字 / 说明 / 默认态都取自定义），新增组件不用改 `MainWindow.vue`；开关走 `updateModuleEnabled(id, enabled)`，排序（拖拽或手柄上的 ↑/↓）走 `updateModuleOrder(ids)` 写同一个 `order` 字段，配置里没有该模块时就地补一条。
- 主窗是**两级导航**，两级都用 `ui/BaseTabs.vue`（variant 区分外观，键盘导航与 ARIA 关联只有一份）：
  - 一级（`variant="underline"`，下划线）= 「悬浮窗设置」+ 每个有 `settings` 的组件各一页；`MainWindow.vue` 按注册表推页面，没有硬编码的 tab 数组。
  - 二级（`variant="chips"`，胶囊）= 当前组件自己的设置分区，列它自己的 `settings.sections`。
  - ⚠️ 「悬浮窗设置」**一页到底、没有二级**：它本身就是一块整体（列表 + 预览左右并排），再切一刀会把「改组件」和「看效果」拆开。二级只服务组件设置页。
  - ⚠️ 两级 tabbar 上下相邻，外观必须保持不同（下划线 vs 胶囊），否则看不出哪条是一级。组件名在屏幕上已由两级 tab 表达，组件页里那个 h1 用 `sr-only` 只留给读屏。
- 组件自己的配置项回归组件自己的设置页：完成灯保留 / 执行中超时 / Token 单位 / 代理解析器 / **窗口格子上限**都在「Agent 状态灯」的二级 tab「常规」里（`src/modules/agent-status/sections/GeneralSection.vue`，两块卡片用 `.general-cards` 横向并排，窄窗口塌成单列），信息卡拓展与会话记录各占一个 tab（`fill: true`）。⚠️ 前四项是**本组件自己的配置**，存在 `module_settings['agent-status']` 里（字段解释见 `src/modules/agent-status/settings.ts` 与 `src-tauri/src/modules/agent_status/settings.rs`），只有页面设在状态灯页；「窗口格子上限」**只是本组件自己的配置**（`module_settings['agent-status'].max_cells`，滑块 1 / 2 / 3 / 4 / 6 / 9 封顶 3×3），只限制本模块大小、不改动其它组件；整窗仍按各组件需求之和（可超过 9）自动伸缩，页面文案已写明。⚠️ 另一个窗口级参数 **「格子大小」（`cell_px`）设在「悬浮窗设置」页左列、组件列表下方的独立卡片**（不在组件页）：它配的是「一格多少像素」，与组件无关，放组件页会让人以为只影响该组件。
- 打开组件设置有两条路，走同一个事件：① 主窗组件列表行的齿轮；② 点悬浮窗上该组件的区域（槽位根节点的 `data-module-id` + 外壳 `moduleIdAt()` → `emitTo('main', OPEN_MODULE_SETTINGS_EVENT)`，点空白处传 null = 打开组件列表页）。打开后落到该组件**第一个分区**（`activeSections` 变化时重置），不会停在上次看的那一块。⚠️ 顺序必须是「先 showMainWindow 再 emitTo」：窗口还没打开时事件没人接。`pointercancel`（触摸取消）只清状态，**不当点击**，否则会莫名弹主窗。
- 布局 mock 见 `reference/mock/floating-modules/floating-modules-index.html`（组件选择三套，采用第三套）；两级导航 mock 见 `reference/mock/settings-nav/settings-nav-index.html`（双层横条 / 左侧竖排 / 分段控件，采用第一套 A：二级用胶囊条）。
- `reference/mock/` **按功能分子目录**（`floating-modules/` / `agent-status/` / `todo/` / `main-window/` / `settings-nav/`，与 `src/modules/<id>/` 同名）：每个 mock 只与同目录的兄弟互相链接，因此整组搬动不会坏链；引用某个 mock 时写全路径。

### 外壳 / 模块的边界（加组件时的验收口径）

外壳（容器）**不认识任何具体组件**：它只认窗口 label、注册表和几份公开契约。

- 外壳文件 = `src/App.vue`、`src/components/{MainWindow,TooltipWindow}.vue`、`src/components/floating/**`、`src/lib/floating-{layout,modules,module-context,bridge}.ts`、`src/lib/tooltip.ts`、`src/lib/snap.ts`、`src/stores/floatingModules.ts`。这些文件里**不得出现任何模块 id 或模块专有词汇**（状态灯 / 会话 / endpoint / 流程脚本……），注释举例也只写 `<id>`。
- 模块自己的东西（视图 / 信息卡 / 设置分区 / 面板 / 纯逻辑 / store / 样式）全部收在 `src/modules/<id>/`，跨目录引用写 `@/modules/<id>/…`。
- 模块 → 外壳只走三份契约：注入的 `FloatingModuleApi`（`allocatedCells` / `reportHover`）、`@/lib/floating-*` 的纯函数与事件常量、外壳公开的 store（如悬浮窗配置）。
- **样式归属**：`src/assets/main.css` 只留窗口外壳 / 宿主 / 公共控件 / 公共信息卡行与分组（`.main-*` / `.floating-*` / `.ufo-*` / `.module-catalog*` / `.module-row*` / `.module-settings*` / `.shape-preview*` / `.general-cards` / `.tooltip-shell|row|section*`）；模块样式写在 `src/modules/<id>/<id>.css`，由模块定义 `import`。
  ⚠️ Tailwind v4 只给「入口 CSS」注入主题与工具类：独立 CSS 文件里用 `@apply` 必须先 `@reference "../../assets/main.css";`（在文件头），否则构建报 `Cannot apply unknown utility class`。该指令只提供编译上下文，不会重复输出入口样式。
- 验收：`rg -n "agent-status|agentStatus" src/App.vue src/components/floating src/lib/floating-modules.ts src/lib/floating-module-context.ts` 应当无命中（模块自己的目录、模块的 spec 除外）。
- **配置下放**（外壳配置只留 `modules[].enabled|order` + 窗口级尺寸参数 `cell_px`，其余字段全在各组件自己的 `module_settings[<id>]` 里 —— 连格子上限也下放给各组件自己）：
  - 前端：外壳 store 只提供 `moduleSettings(id)` / `updateModuleSettings(id, patch)`（浅合并后整份落盘）；字段名 / 默认值 / 取值范围写在模块自己的 `src/modules/<id>/settings.ts` 里，读的时候各自夹取 —— 坏一个字段只影响它自己。
  - Rust：`FloatingModuleConfig.module_settings` 整块透传（`BTreeMap<String, Value>`）；模块在 `src-tauri/src/modules/agent_status/settings.rs` 里解析（`AgentStatusSettings::from_block`，逐字段宽松取值 + 夹取），并通过组合根（`lib.rs`）登记的两个钩子接入外壳：`adopt_legacy_fields`（认领旧版扁平字段）与 `config_hook()`（配置落盘后同步给运行时，改完即时生效）。
  - 旧配置迁移：外壳把顶层多出来的未知字段（`#[serde(flatten)] legacy`）原样交给各模块的认领钩子；有认领就把清理过的配置**写回一次**（不 bump revision、不广播）。⚠️ 只在文件里还没有该模块配置块时取旧值，绝不覆盖用户新改的设置。
  - ⚠️ 外壳不得再出现模块专有字段名，验收：`rg -n "completed_retention_ms|running_timeout_ms|token_unit|proxy_resolver" src-tauri/src/floating src/lib/floating-modules.ts src/stores/floatingModules.ts` 应当无命中。

### Rust 侧结构（与前端一一对应）

前端按组件切目录，Rust 侧同样 —— 外壳不认识组件，只有组合根点名组件：

```
src-tauri/src/
├── lib.rs                  # 组合根：窗口 / 托盘 / 命令注册 / 状态装配 / 组件钩子登记
├── main.rs                 # 入口：`--status-hook` 快进快出，其余交给 Tauri
├── devtools.rs / mouse.rs  # 外壳：调试开关 / 鼠标按键状态
├── floating/               # 悬浮窗外壳（对应前端 `src/lib/floating-*` + `stores/floatingModules.ts`）
│   └── module_config.rs    # 组件配置的存取：`module_settings` 整块透传，不认字段名
└── modules/
    └── agent_status/       # Agent 状态灯（对应前端 `src/modules/agent-status/`）
        ├── settings.rs     # 本组件的配置块：解析 / 夹取 / 认领旧字段 / 推给运行时
        ├── status/         # 采集与灯态：hook 收口 / 兼容层 compat/ / 内核 / Codex rollout / token
        ├── flow/           # 信息卡拓展的流程脚本沙盒（QuickJS）
        ├── proxy/          # 代理解析器（本地代理 → 上游 endpoint）
        ├── endpoint_profiles.rs   # endpoint 档案落盘（信息卡拓展的数据源）
        └── session_store.rs       # 会话存档 SQLite（跨重启续档）
```

- 目录名 = 前端模块 id 的 Rust 写法（`agent-status` ↔ `agent_status`）；跨目录引用一律写全路径 `crate::modules::agent_status::…`，**不要**在 crate 根做 `use` 别名转发（那样会掩盖结构）。
- 加一个 Rust 侧组件 = 建目录 + `modules/mod.rs` 加一行 + 在组合根登记它的钩子 / 状态 / 命令；外壳（`floating/`）零改动。
- 验收：`rg -n "agent-status|agentStatus|token_unit|proxy_resolver|completed_retention_ms" src-tauri/src/floating src-tauri/src/mouse.rs src-tauri/src/devtools.rs` 应当无命中。

### 悬浮窗 hover 信息提示

- 指示灯 `pointerenter`/`pointerleave` → 模块上下文的 `reportHover(key)` → 宿主补 moduleId 转发给外壳 → 外壳延迟 250ms 后 `emitTo('tooltip', 'tooltip-hover', payload)`，payload 里的 `target` = `{ moduleId, key }`。
- 提示窗只当容器（定位 / 显隐 / 指针进出转发），**内容按 `moduleId` 从注册表分派**（`lib/tooltip.ts` 的 `resolveTooltipDefinition`）：模块没声明 `tooltip`、或该模块的 `hasContent` 说数据还没到，就不弹。
- 内容是模块自己的组件，数据也由它自己订阅（如状态灯订阅 `agent-status` 取会话）；payload 只传「悬停目标（模块 id + key）+ 悬浮窗外框 + 贴边状态」，避免数据陈旧。
- 信息卡的「一行」和「一组」是外壳公共件（`tooltip/TooltipRow.vue` / `tooltip/TooltipSection.vue`）：行 = 左标签 + 右值（`mono` 等宽、`tone` 语义色、`title` 完整原文、默认插槽放富内容），分组 = 顶部强分隔线。其它组件的信息卡直接复用，别各写一份 class。
- ⚠️ 提示窗内容组件用**多根节点（fragment）**渲染：`.tooltip-shell` 直接是行级 flex 容器，套一层 div 会破坏间距与 `mt-auto`。
- 定位是纯函数 `placeTooltip`（`src/lib/tooltip.ts`），优先放在悬浮窗**未贴边**的一侧，越界自动换侧。
- 拖拽 / 点击 / 贴边重算时必须 `dismissTooltip()`：`startDragging()` 后指针交给系统，`pointerleave` 可能不再触发。
- tooltip 是独立窗口，**不适用**方形/横条/竖条三形态适配；默认尺寸 300×260（`tauri.conf.json`），组件可用 `tooltip.size` 覆盖（提示窗在 `present()` 里**先 `setSize` 再量尺寸定位**）。内容用 flex + 省略号自适应。
- Agent 状态灯信息卡的展示字段（自上而下顺序固定）：provider/phase、Endpoint（见「代理解析器」）、token 用量、**流程脚本拓展字段 + 刷新标注**、`prompt`（用户提问）。拓展字段紧跟 token 行、提问永远在最后（不要插回提问下面，否则与「字段属用量类」的视觉分组错位）。**不展示 session id**。这些字段属于 `AgentStatusTooltip.vue`，不是提示窗的公共契约。
- ⚠️ **指示灯不要加 `title` 属性**：`title` 会触发 Windows 原生文字提示，与悬浮窗自己的 tooltip 重复且更简陋。无障碍信息只走 `aria-label`（不产生可见提示）。
- ⚠️ **溢出格（`+N`）上报的是合成 key**（`@overflow:<layout>:<cells>`，编解码在 `src/modules/agent-status/lib/agent-status-layout.ts`）：悬停事件只带「模块 id + key」，而提示窗是**另一个窗口**，拿不到悬浮窗的形态与格子数 —— 把它们编进 key，两侧就能用**同一个** `partitionIndicatorSessions` 重算折叠清单，屏上的占比环与清单里的行必然同源。key 由模块自己解释，外壳照旧只当字符串转发，**不改任何对外契约**。
- 溢出格本身是「**分片环**」：外圈按折叠会话的灯色占比切开（红 → 黄 → 绿，注意力优先级顺序，不是开始时间），环外两圈同色模糊光晕做层次，中间深色核心托住 `+N`。尺寸一律按**宽度**取百分比 + `aspect-ratio: 1`（三种形态下行宽都 ≤ 行高，是竖条那条约束的推广），放大倍率受槽位 `overflow: hidden` 约束不能再加。见 `reference/mock/agent-status/overflow-stack-index.html`（四套方案，采用 B）。
- 折叠清单的 `hasContent` 是「**有折叠才弹**」；清单为空（悬停期间会话结束）时卡片给一句兜底文案，不留空框。
- ⚠️ tooltip 的 token 单位来自模块配置，而 `floatingModules` 是窗口级 store，由外壳在 `App.vue` 里 `start()`（三窗都起）；否则提示窗永远停留在 `fallbackConfig` 的默认单位，主窗口切换后不生效。

### token 用量展示

- 数据链路：hook 只透传 `transcript_path` → 常驻 monitor 线程用 `TokenTracker` 增量解析 transcript → `StatusRegistry::set_tokens` → `SessionSnapshot.tokens`。**hook 进程不解析 transcript**（每次事件都要启动一次，必须快进快出）。
- 去重按 `message.id`：同一条 API 响应会写成多条 assistant 记录，每条都带同一份 usage，不去重会翻 2–3 倍。子代理 transcript（`<session-id>/subagents/*.jsonl`）并入同一会话。
- `SessionSnapshot.tokens` 为 `Option<TokenTotals>` + `#[serde(default)]`（**不得升 `SCHEMA_VERSION`**）；它不是事件派生字段，由 `snapshot()` 时合并。
- 单位可配（模块配置块 `module_settings['agent-status'].token_unit`；字段解释见 `src/modules/agent-status/settings.ts` 与 `src-tauri/src/modules/agent_status/settings.rs`）：`tokens` = 原始计数（`1,261,847`），`m_tokens` = 百万（`1.26M`，不足 0.01M 显示 `<0.01M`）。**只在主窗口改**（Agent 状态灯的设置页），悬浮窗 / 提示窗只读展示。
- 格式化在 `src/modules/agent-status/lib/agent-status.ts`：`tokenTotal` / `formatTokenCount` / `formatTokenMillions`。tooltip 里的 token 明细（title）始终用原始计数，不受单位影响。

### 会话展示字段（base_url / prompt）

两个字段都从 hook 侧采集，经 `AgentEvent` → `SessionSnapshot` → 前端 `SessionSnapshot` 传递，均为 `Option<String>` + `#[serde(default)]`（**不得升 `SCHEMA_VERSION`**，否则旧 hook 事件被静默丢弃）。

| 字段 | 来源 | 处理 |
|---|---|---|
| `base_url` | **配置文件优先**：`~/.claude/settings.json` → `<cwd>/.claude/settings.json` → `<cwd>/.claude/settings.local.json` 的 `env.ANTHROPIC_BASE_URL`（后者覆盖前者，`cwd` 取自 hook payload）；**环境变量兜底**：`ANTHROPIC_BASE_URL`。codex 读 `$CODEX_HOME/config.toml`（见下「Codex」节），环境变量兜底 | 只按名字取该 key（`env` 块里的 token 绝不读取）；不脱敏，仅 trim；上限 4096 |
| `prompt` | **仅** `UserPromptSubmit` payload 的 `prompt` 字段（`UserPromptExpansion` 的展开模板文本不认，否则会覆盖用户真正的提问） | trim + 截断；上限 4000 字符；同轮内后续事件继承，下次提交才覆盖 |

- 同轮内后续事件继承这两个值，下次提交/新事件才覆盖（`kernel.rs` 用 `.or(previous_*)`）。
- ⚠️ 二者**明文写入** `%APPDATA%/<identifier>/status/inbox/*.json`（约 500ms 内消费删除）。提问可能含密钥或私有代码，这是已知代价。

### 代理解析器（Endpoint 反查上游）

用 cc-switch 这类本地代理时，`base_url` 只是 `127.0.0.1:<port>`，真实上游写在代理自己的配置里。UFO 通过「代理解析器」把本地地址反查成上游 endpoint 显示在 tooltip 的 Endpoint 行。

- 下拉选的是**解析器工具**（`不解析(none)` / `cc-switch` / 将来更多），不是代理内部的 provider。存模块配置块 `module_settings['agent-status'].proxy_resolver`（`status/settings.rs` 解析；外壳只透传整块 JSON），旧配置缺字段回落 `none`。
- 命中解析时 `SessionSnapshot.upstream_url` 被填充；tooltip 用 `displayEndpoint`（`src/modules/agent-status/lib/agent-status.ts`）= `upstream_url ?? base_url`，**上游优先、回落本地**，两者都无才显示 `—`。
- 接口：`ProxyResolver` trait（`proxy/mod.rs`），`build_registry` 里注册一行即接入新工具，下拉与填充逻辑零改动。
- `CcSwitchResolver`：只读打开 `~/.cc-switch/cc-switch.db`，按 `proxy_config` + `providers` 反查。命中要求该 app 行**两列同时为 1**：`proxy_enabled`（全局代理服务器开关，三行镜像同值）且 `enabled`（该 app 是否被接管）——语义来自 cc-switch 源码，只看其一会在「服务器开着但该 app 未接管」时误命中。⚠️ `listen_port` 是 `INTEGER`（非 TEXT），必须按整数读，否则 rusqlite 读整行 `InvalidType` 失败、功能空转。上游取 `is_current=1` provider 的 endpoint；带 5s TTL 缓存。
- ⚠️ **两个 app 的 `settings_config` 存法不同，取上游必须按 app 分流**（`upstream_from_settings`）：claude 是 `env.ANTHROPIC_BASE_URL`；codex **没有 `env` 块**，base_url 埋在 `config` 字段那段 **TOML 文本**里（`model_provider` → `[model_providers.<name>].base_url`）。早先只读 `env.ANTHROPIC_BASE_URL`，导致 Codex 会话的 upstream 恒为 None、tooltip 一直停在本地代理地址。解析规则复用适配器那份（`status::base_url_from_config_text`），保证「tooltip 显示的 endpoint」与「反查出的上游」同源。
- ⚠️ `settings_config` 整块 JSON 会进内存解析，但**只按名字取 base_url 一个 key**（codex 侧只读 `config`，与它同级的 **`auth` 存的是 API key，绝不读取**）；token / api key 一律不读取、不缓存、不上报。
- ⚠️ `upstream_url` 是 `Option<String>` + `#[serde(default)]`，**不得升 `SCHEMA_VERSION`**。
- ⚠️ cc-switch.db 被应用持有：必须 `SQLITE_OPEN_READ_ONLY` + 短 `busy_timeout`，避免争锁。切换 provider 后最多 5s（TTL）反映到悬浮窗。

### 信息卡拓展（按 endpoint 的 tooltip 拓展字段）

tooltip 的拓展字段**跟着 endpoint 走**（同一上游共用一套配置），而会话快照里的 base_url/upstream_url 只在会话存活时存在。因此改成按 endpoint 的持久化档案，配置面板挂在「Agent 状态灯」组件设置页的二级 tab「信息卡拓展」里（`悬浮窗设置` → 组件列表的齿轮 / 点悬浮窗上的状态灯），用左右主从布局配置各 endpoint。

- 数据源是 `endpoint_profiles.rs` 的持久化档案表（`<app-data>/endpoint-profiles.json`），**不靠会话存活**；monitor 每轮采集存活会话的 `upstream_url ?? base_url` 落盘，前端 `get_endpoint_profiles` + `endpoint-profiles` 事件读取。
- ⚠️ **loopback（127.0.0.1 / localhost / ::1）一律不登记** —— 本地代理地址不是可配置对象，端口还会随重启变。有效 endpoint 口径与前端 `displayEndpoint` 一致：命中解析时登记反查出的**上游**，不是代理监听地址。
- 去重键 `id` = `proxy::endpoint_identity` 归一的 `host:port`；只在出现未见过的 id 时才写盘（同一批 endpoint 每 500ms 都进来）。
- 档案结构含 `extensions`（JSON map），**加拓展字段不改协议**。信息卡拓展的 per-endpoint 配置（流程脚本 `FlowConfig`）就整体存在 `extensions["flow"]` 下，不给 `EndpointProfile` 加字段。
- ⚠️ `normalize_host_port` 早期把 `[::1]:8080` 切成 `[::1:8080`（漏 `]`），会让 loopback 判定失效、本地地址被登记；已由 `normalize_host_port_keeps_ipv6_brackets_and_port` 锁定。
- 主窗两级 tabbar 都走 WAI-ARIA Tabs：roving tabindex + ←/→/Home/End（逻辑在 `ui/BaseTabs.vue`，两级共用）；id 约定 `${idPrefix}-tab-<key>` / `${idPrefix}-panel-<key>`，一级 idPrefix = `main`、二级 = `module`。悬浮窗设置页容器是 `.main-window--wide`（突破 `.ufo-card` 的 `max-w-md` 限宽，列表 + 预览并排），组件设置页是 `.main-window--stack`（分区内容纵向堆叠）。
- ⚠️ 这两个面板**占满内容区高度、左右两列各自滚**（`.ep-panel` / `.sr-panel`）：grid `items-stretch` + `grid-template-rows: minmax(0,1fr)`，左卡 `h-full` + flex 列（列表 `flex-1` 内部滚，底部「手动添加 / 清除全部」贴卡底），右列自己 `overflow-y: auto`。别改回 `max-h` 或让列表随内容长高：那会把右侧和底部按钮挤出可视区。
- ⚠️ 占满高度靠一条**完整的高度链**，缺一环就退化成「整页滚动、面板不占满」：
  `.main-window--wide`（flex-1 + overflow-hidden）→ `.module-page--fill` → `.module-page__section--fill` → `.ep-panel` / `.sr-panel`。
  这条链由 `FloatingModuleSection.fill` 决定（见「悬浮窗模块化」）：`fill: true` 的面板必须让每一层都带 `min-h-0` + `flex-1`（写在 `main.css` 的 `.module-page--fill` / `.module-page__section--fill`），分区自带的标题与说明加 `shrink-0` 免受挤压。
- 读写分离：读命令 `get_endpoint_profiles` 给三窗口；`add/delete_endpoint_profile` 只在 `main-module-settings.json` 授权（仅 main 窗口）。
- ⚠️ 拓展面板的反馈**不能用 `window.alert` / `confirm`**：装了 `tauri-plugin-prevent-default`，原生对话框会被拦；一律走 `role="status"` 行内提示，移除用两步确认。
- ⚠️ tick 读配置必须走 `profiles.with_profile(id, …)`（只 clone 该档案的 extensions），**不要**用 `snapshot()`——后者整表 clone 含脚本正文（单份上限 64KB × 64 条），每 500ms 一次是纯浪费。

### 流程脚本（QuickJS 沙盒，per-endpoint）

「信息卡拓展」面板（在 Agent 状态灯的设置页里）右侧让用户为某 endpoint 写一段 JS 脚本，返回 `{label,value,color}` 数组，作为 tooltip 信息卡的拓展字段（如查上游余额）。

- **执行方式**：内嵌 QuickJS（`rquickjs`，features 只 `std/parallel/classes/properties`，**刻意不含 `loader`/`dyn-load`** ⇒ 脚本 `import`/`require` 不可用，没有读文件/隐式出网路径）。沙盒三件套：内存帽 2MB + 中断超时 + 无模块加载器。见 `flow/runner.rs`。
- **注入的宿主 API**（唯一能做的事）：`http.get/post/request`（Rust 侧 `ureq` 代发）、`secrets.get`、`cache.get/set`、`format.num/pct/bytes`、只读 `ctx.endpoint/session/lastResult`。`console` 只写内存缓冲、**不广播**（防脚本 `log(secrets.get('K'))` 泄漏）。
- ⚠️ 出网只走 `http`，且 `validate_http_url` 拦住 loopback / 私网 / 链路本地 / CGNAT / 云元数据 / 无点短主机名。固有代价：域名解析到内网 IP 挡不住（不发连接无法预解），文档已注明。
- **为什么把出网收成一个 `http`**（改沙盒能力前先读这段）：脚本是用户原文，安全性完全依赖「他能碰到什么」。QuickJS 本身没有任何 syscall —— 没有 `loader`/`dyn-load` feature 就连 `import`/`require` 都不存在，JS 层也没有 fs/net/child_process 这些宿主接口。因此脚本唯一能「影响外界」的路径，就是我们主动 `globals.set` 进去的那几个闭包。收成一条通道换来三件事：① **地址策略只有一处可管** —— `validate_http_url` 把 loopback/私网/云元数据挡在发起连接之前，若同时开放 socket 或模块加载，端口扫描与 SSRF 根本无从拦起。② **资源可控** —— 单次请求 500ms–15s 超时、响应体 256KB 封顶，配 worker 串行队列，不会出现一个脚本把带宽和内存吃光。③ **可审计、可静默** —— agent 全停 = 零出网（调度只看存活会话），用户能靠「网络有没有动静」判断脚本在干什么；多一条出网路径这条不变量就破了。要加新能力（如读本地文件、走 IPC）必须同样收敛成一个带策略校验的宿主闭包，绝不能给脚本原生网络。
- **调度**：status monitor 每轮（约 500ms）调 `flow.tick`，**只对「当前存活会话涉及的 endpoint」且到点者入队**；口径是「还在 `StatusSnapshot.sessions` 里」，不看 phase（黄/绿/闪烁灯都可能被悬停）。agent 全停 = 零出网。跑在专用 worker 线程、串行执行；结果按**会话 key** 索引广播 `flow-results`，tooltip 零归一逻辑直接查表。
- ⚠️ 广播值类型是 `FlowSessionResult { fields, updated_at_ms, interval_sec }`（不是裸 `FlowField[]`）：tooltip 要在字段下方标注「更新于 HH:MM:SS」，让用户能分辨屏上是刚跑的值还是旧值（脚本挂掉时字段整块不显，更需要时间锚点）。`interval_sec` 仍随结果一路带下来（后端调度 / 退避用），但**前端已不展示刷新周期**（对用户是噪音）。`apply_result` 每次回写都刷新标注；格式化在 `src/modules/agent-status/lib/flow.ts` 的 `formatFlowUpdatedAt`；`isFlowSnapshot` 对**整份结果对象**严格校验（缺新鲜度字段的旧形态直接拒绝）。
- ⚠️ **同 endpoint 多会话只跑一次**（`running` 去抖），但结果回填到每个会话 key。连续失败按 `interval × 2^n` 指数退避、封顶 600s（否则脚本挂了还每秒打网）。
- **密钥**：`secrets.get(name)` 按名字从 OS 凭据管理器取（`keyring`，service = `<APP_ID>/flow/<endpoint_id>`）；值不落盘、不进档案、不进日志。⚠️ Windows 凭据管理器**不能按 service 列举**，因此档案 extensions 里存一份 `secret_names` 名字清单——它是删档案时唯一能级联清凭据的依据，清单丢了凭据就成了孤儿。
- ⚠️ `secret_names` 与 `script_hash` 由**服务端权威维护**：`save_endpoint_flow` 会保留旧 `secret_names`、按脚本内容重算 `script_hash`，**不接受**前端传入这两项（前端漏传会清空凭据记账）。
- `script_hash`（FNV-1a，非加密）只用于判断「改没改脚本」：脚本没改才把上次结果作为 `ctx.lastResult` 继承（增量语义），改了就不给（否则语义错位）。
- ⚠️ `parse_fields` 的 `MAX_FIELDS=4` 与 tooltip 高度（`tauri.conf.json` 的 260）绑定：加字段数要同步看窗口尺寸，超出会被 clip。一屏预算（含刷新标注）：head + Endpoint + token + 4 字段 + 标注 + 提问 1 行 ≈ 236px；提问靠 `--compact` 压到 1 行，**不要再给它加行数**，否则标注会被挤出可视区。
- ⚠️ rquickjs 闭包两个坑（已踩过）：① 参数生命周期必须写外层具名 `'js`，不能写 `'_`（否则 E0106 / lifetime may not live long enough）；② 可选参数用 `prelude::Opt` 而非 `Option`——`Option` 没实现 `FromParam`，会被当成**必填**参数。
- ⚠️ release 是 `panic = "abort"`：`flow` 全模块宿主闭包不得 panic（不 unwrap/expect/索引/slice），失败一律变 `Err(String)` 或跳过，否则一次 panic 带走整个 App。
- 前端：编辑器 `src/modules/agent-status/panels/EndpointFlowEditor.vue` + 模块的 `lib/flow.ts` + `stores/flowResults.ts`。结果 store 由**模块定义**的 `activate()` 启动（见上表），不再依赖 `App.vue`：不启动提示窗就永远收不到脚本字段。`run_flow_test` 不进调度队列，日志回显（仅主窗口）；⚠️ 它是 **`async` 命令 + `spawn_blocking`**——非 async 命令跑在主线程，一次测试最坏阻塞 15s 会冻结窗口事件循环；带引用入参的 async 命令按 tauri 宏要求必须返回 `Result`。
- 保存/删除 endpoint 后会**立刻**再广播一次 `flow-results`（字段已被 invalidate 清掉），否则 tooltip 手里仍是旧脚本的字段、要等下次轮询才刷新。
- 新命令注册三处（`#[tauri::command]` + `permissions/status.toml` + capability）：`get_flow_results`/`get_flow_snapshot` 给三窗口（`default.json` 的 `allow-get-flow-results`）；`save_endpoint_flow`/`run_flow_test`/`set_flow_secret`/`delete_flow_secret` 只在 `main-module-settings.json` 授权（仅 main）。

### 会话记录持久化（SQLite，跨重启续档）

会话状态原本完全活在内存（`StatusRegistry` 的 HashMap），重启即丢失，`Completed` 还会按 retention 清掉。后果是同一 `session_id`（如 `claude --resume`）被当成全新会话：token 归零、`started_at_ms` 变现在（灯跳位）、prompt/base_url 要等下次事件。`session_store.rs` 把「跨重启仍有意义」的字段落 `<app-data>/session-records.db`（`rusqlite` bundled，零新增依赖），事件命中存档时回填。「Agent 状态灯」设置页的二级 tab「会话记录」里有面板管理这些行（列表 + 单条删除 + 清除全部）。

- **回填三字段**：`started_at_ms` / `prompt` / `base_url` 的继承阶梯变成「事件 > 内存前驱 > 存档」，走 `apply_event_with_seed`（seed 仅在内存里没有该会话时生效）。resume 的会话因此保住原灯位（`snapshot()` 按 started 升序）。
- **token 地板**：`set_tokens` 逐字段取 max（`merge_token_floor`）—— transcript 被截断/删除时重扫值小于历史值，直接覆盖会让用户已看到的数字凭空缩小。⚠️ 副作用是**数字只增不减**，与 `tokens.rs` 既有取舍一致。seed 非零时先灌地板，resume 瞬间不必等重扫就显示历史值。
- **只 GUI 侧持有**：hook 二进制必须继续快进快出，**绝不打开** AppData 的 SQLite。
- **两套版本别混**：`SessionRecordList.schema_version` 是线协议，按硬规则**永不递增**（加字段一律 `Option`+`#[serde(default)]`）；`PRAGMA user_version` 是库内布局，加列时递增它配 `ALTER TABLE`。
- **保留策略**（本轮为常量）：`MAX_RECORDS = 200`（超出按 `last_seen` 淘汰最旧，存活会话豁免）+ 启动删 `last_seen` 超 30 天的行。
- ⚠️ **prompt 现在长期明文驻留磁盘**：原先只在 inbox 里约 500ms 就被消费删除，落库后会长期留在 `session-records.db` 且在主窗口「会话记录」页可见。提问可能含密钥/私有代码，这是已知代价（见下）。
- ⚠️ **「清除全部」只删归档、不强清内存**：仍在运行的会话会在下一个 hook 事件后被重新记录（面板文案与详情都按此写明）。要连灯一起清是另一种语义（需 `registry.remove`），本轮按需求选择不这么做。
- **存活标记在前端 join**：列表行的「灯亮着」按 `agentStatus.sessions` 现算，不给存档加存活态字段。
- **Endpoint 展示 = `upstream_url ?? base_url`**（与 tooltip 的 `displayEndpoint` 同一口径）：`base_url` 是会话实际连的地址（用 cc-switch 时就是 `127.0.0.1:<port>`），`upstream_url` 是刷盘那一刻代理解析器反查出的上游。**两者都要存** —— 只存 base_url 会让面板一直显示 localhost；只存上游则丢失「经哪个本地代理转发」的副标注（`recordLocalNote` 用它）。前端读法是 `src/modules/agent-status/lib/session-records.ts` 的 `recordEndpoint` / `recordLocalNote`。
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
- **事件源目前有两个**：hook（inbox 里的规范 JSON）与 Codex rollout 补漏（`codex_rollout.rs` 合成 `Failed`）。两者都产出 `AgentEvent`，在 monitor 里走**同一条入口** `apply_status_event`（广播调试事件 → 登记 transcript → 取存档回填 → 进内核）。加第三个源时照着接这个函数，别绕开它。⚠️ inbox 一批事件按**接收时刻**（`received_at_ms`，同刻按事件 id）排序后再消费 —— `read_dir` 顺序由文件系统决定，批内乱序会让迟到的「子代理收尾」先于更早的「开工」生效；平台事件多数不带 `sequence`，批与批之间的乱序识别不了（已知边界）。
- `AgentEvent.kind` 是 `#[serde(default)]` 新增字段，缺省 `Unknown`（**不得升 `SCHEMA_VERSION`**）；旧事件反序列化后为 Unknown = 不驱灯色，与「无法识别」一致。
- **hook 输入上限 4 MiB**：Codex 的工具事件会把 `tool_input`（apply_patch 补丁等）原样带上，卡 64 KiB 会丢掉有效生命周期事件。落盘的 inbox 事件仍只带提取字段。

### Codex（已接入：读 rollout，不用 hook）

**结论先说**：Codex 的**事件与 token 全部来自 rollout**（Codex 自己写的会话日志）；hook 只保留**一个极窄的口子** —— `PermissionRequest`，因为那是 rollout 里唯一拿不到的状态。原因如下：

- **没有错误类事件**（Claude 有 `StopFailure`）：429/502 这类以 error 结束的轮次不触发任何 hook；
- **中断路径也不触发**：从 Codex Desktop 的停止按钮取消（app-server 的 `turn/interrupt`），rollout 里 `turn_aborted` 已落盘，hook 侧零调用；
- **会静默失效**：hooks 要在设置页逐条信任、按内容哈希记账（改一次重新信任），实测出现过「一段时间一次都不触发、四处日志无报错」。

**因此 `~/.codex/hooks.json` 只需配一个事件**：

```json
{ "hooks": { "PermissionRequest": [
  { "hooks": [{ "type": "command", "command": "ufo --status-hook --provider codex", "async": true, "timeout": 3 }] }] } }
```

⚠️ 不要顺手把其它事件也加进来：`compat::codex::CodexPermissionAdapter` 会 `Ok(None)` 静默忽略它们（不落盘、不算失败），但每个事件都会**多起一个进程**空转 —— 白花钱。

#### 机制（`status/codex_rollout.rs`）

| 环节 | 做法 |
|---|---|
| 发现 | 扫 `$CODEX_HOME/sessions/<年>/<月>/<日>/`（回溯 2 天，目录名字典序即时间序，**不需要本地日期库**）；会话 id 取文件名末尾 uuid（实测 == `session_meta.id`） |
| 播种 | 注册时读一遍已有内容，**只为**拿 `cwd`（→ endpoint）与 token 累计，**不产出事件**（否则一启动就复读当天所有历史会话） |
| 映射 | `session_meta`/`task_started`/`user_message` → Running；`task_complete` 无 error → TurnFinished；**带 error → Failed**；`turn_aborted` → WaitingForUser（黄不闪） |
| 增量 | 只读新增字节；**文件在长 = 有活动**（绿），只有生命周期记录才改灯态 |
| 去抖 | **批内连续**同灯态压成一条；但同一批里灯态真的变化多次时（如中断后的下一轮「恢复执行 → 跑完」）**按文件里的先后顺序各产出一条** —— 只留最后一条会让内核停在「等你操作」上，把完成信号整条吞掉（内核在 WaitingForUser 下不认主线程 Stop，那是 Claude 阻塞等待的保护）。「绿」每 30 秒补一条保活（长命令期间 Codex 一条记录都不写，不续命会被「执行中超时」清掉）。⚠️ 去抖基线必须跟着**实际灯态**走：hook 把灯抬成黄的那一刻巡检器并不知情，所以 monitor 对**来源是 hook** 的 Codex 事件调 `note_light()` 回灌基线（`apply_status_event` 的 `EventSource` 参数就是干这个的）。不回灌的话，批准后 rollout 恢复写入的那条「绿」会被当成重复整个吃掉，灯卡在黄直到 30 秒保活 |
| 结束 | 归档 = rollout 被**移走**到 `archived_sessions/`，连续两轮读不到即判 SessionFinished |
| **等你授权** | rollout **没有**这个状态的任何记录（记录类型 / item 字段 / output 正文全查过）→ 只能靠 hook 的 `PermissionRequest` 补：它带 `tool_name` / `tool_input.command` / `tool_input.description`（Codex 自己写的审批理由）。映射为 WaitingForUser（黄灯不闪），理由进 `waiting_reason` 显示在 tooltip 的 head 行 |
| 预筛 | `compat::codex::cares()` 先做子串扫描：97% 的字节是命令输出/消息正文（单条可达 100 KB），只有命中小标记的行才做 JSON 解析 |

⚠️ 不要把 Codex 的 hook 适配器「顺手」补回来：两路同时驱动同一会话会让灯态互顶（`compat::tests::only_claude_uses_the_hook_adapter` 锁着这条）。

#### 取不到的东西

- **子代理**：子线程有自己的 rollout（带 `parent_thread_id` + `source.subagent`），天然不会污染父会话的灯，所以**不需要** Claude 那条 `agent_id` 守卫。
- **hook 仍可能静默失效**：实测出现过「某个会话一次 hook 都不触发、四处日志无报错」。所以 `PermissionRequest` 只是**锦上添花** —— 它挂了就退化成黄灯不显（绿灯），rollout 那条链路完全不受影响。两条来源不冲突的根据：**等审批期间 rollout 一条记录都不写**（实测），批准后命令一跑就有新记录 → 恢复绿灯（由 `note_light` 保证「立刻」而不是 30 秒后，见上表「去抖」行）。
- ⚠️ **monitor 每轮的处理顺序是「rollout 先、inbox 后」**，别调回去：hook 事件代表更晚发生的事实（弹审批框之前 rollout 已经写过 `function_call` 记录），放后手它才能在同一轮里压过由「文件在长」推出的绿；反过来会让刚抬起的黄灯被同一批旧字节顶回绿。

#### endpoint / token

- **Endpoint**：读 `$CODEX_HOME/config.toml` → `<cwd>/.codex/config.toml` 覆盖，取顶层 `model_provider` 指向的 `[model_providers.<name>].base_url`；环境变量 `OPENAI_BASE_URL` / `OPENAI_API_BASE` 兜底。只读这一个 key（`auth.json` 一律不碰）。用 `basic-toml`（只读，刻意不用官方 `toml` 以免拖进 toml_edit）。同一套解析规则被代理解析器复用（`status::base_url_from_config_text`），保证 tooltip 与上游反查同源。
- **token**：`event_msg`/`token_count` 的 `info.total_token_usage` 是**会话级累计值**（不是增量），同一文件内取 max、同会话多 rollout 各自相加。⚠️ **口径与 Anthropic 相反**：`input_tokens` 是 prompt 总量、**已含** `cached_input_tokens` / `cache_write_input_tokens`（实测 `total_tokens == input + output`），映射时必须减出来，否则前端 `tokenTotal()` 把缓存算两遍（实测缓存占比可达 1/3）。`output_tokens` 已含 `reasoning_output_tokens`，不重复加。
- ⚠️ **Codex 的用量不在 `tokens.rs`**：那个模块只管 Claude 的 transcript（按 `message.id` 去重累加）。

### Agent 状态指示灯语义
- 🟢 常亮 = Running（正在执行）
- 🟡 = Waiting（等待用户操作：Stop / PermissionRequest / Elicitation / 等待类 Notification）
- 🟢 + 闪烁 = 两种完成态
  - `Stop`：一轮执行完成 → 先绿闪，retention 后落成黄常亮（对话未结束）
  - `SessionEnd`：对话结束 → 绿闪后移除
- 🔴 = Error（StopFailure 等；红灯不粘，任何能映射出别的阶段的事件都可转灯，`Unknown` 事件除外）
- ⚠️ **子代理（subagent）事件不得把「等用户操作」的黄灯顶回绿**：子代理事件与主会话**共用同一个 `session_id`**（实测确认），只有 payload 里的 `agent_id` 能区分（主线程事件不带该字段）。否则主 agent 弹出提问框（`PermissionRequest`→黄）时，后台仍在跑的子代理事件会立刻把它顶回 Running —— 表现为「Claude 已经在问我了，灯却一直是绿色」。`kernel.rs` 的 `apply_event_with_seed` 按这几条办事，**改它时务必一起保住**：
  - 主线程正在等用户操作（WaitingForUser）、或会话已结束 / 已出错（Completed / Failed）时，子代理事件**只**维护后台记账（把该 `agent_id` 加入 / 移出活跃集合）并刷新 `last_seen_at_ms`，灯色与计时（`is_flashing` / `remove_at_ms` / `error_code` / `waiting_reason`）原样保留；
  - 只有主线程已停、处于「等 prompt」空闲时，子代理活动才把灯拉回「后台运行」（对外绿常亮），最后一个活跃子代理结束时落回黄；
  - 带 `agent_id` 的 `Failed`（子代理自己的工具失败）**不代表父会话出错**：不得把父灯染红、也不得清空其它仍在跑的子代理；
  - 主线程出错时活跃子代理集合**不清空**（后台事实要留到恢复之后，否则灯会误落黄）；
  - UFO 中途启动、只先收到子代理活动时，灯的起点是「后台运行」而不是「主线程在跑」，否则最后一个子代理收尾后无处可退、灯会一直绿；
  - 子代理的 `sequence` 有自己的水位（按 `agent_id` 分开比），**不与主线程的序号互相比较**，也不写进对外快照的 `last_sequence`。
- ⚠️ **「新一轮开始」才清旧黄灯，而且只清空闲黄、只认主线程**：主线程自己的 `Running`（`agent_id` 为空）触发一次同 provider 清理，目标是**内核私有状态为「等 prompt」**的会话（一轮跑完落下的黄），**不碰**「等用户操作」的审批黄；判据必须用私有状态而不是投影后的阶段 —— 主线程 Stop 后仍有子代理在跑时对外也是 Running，那不是新一轮。删除要**一整套**（对外快照 + 私有状态 + 子代理集合 + token + 序号水位），只删 `sessions` 会让同一个 key 再次出现时读到上一世的状态。
- 事件去重是**有界 FIFO 窗口**（`SEEN_EVENTS_CAPACITY`，重复命中不把条目推回队尾）：会话被删后窗口内重放不会重新点灯；窗口外的旧事件不承诺 exactly-once。
- 计时类字段（`is_flashing` / `remove_at_ms` / `completed_at_ms`）由 `resolve_timing` **显式三分支**决定：事件没构成转灯信号（`Unknown` 保持上一状态）时**原样继承上一版**，否则按新状态重算 —— 顺手重算会把 Stop 的绿闪提前掐掉（灯再也不落黄）。
- ⚠️ **`AskUserQuestion`（选择框提问）不走 `PreToolUse`**：它走 `PermissionRequest` + `Notification(permission_prompt)`，作答后发 `PostToolUse`。不需要按工具名特判。
- ⚠️ `notification_type` 的「等人操作」清单必须与 Claude Code 枚举对齐（详见 `reference/doc/agent-status-hooks.md`），**漏列会退化成绿色**。
- 详情见 `reference/doc/agent-status-hooks.md`
