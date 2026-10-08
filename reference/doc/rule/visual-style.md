# 全 App 视觉规范（Matte Panel · 方案 A）

> **管什么**：全 App 的设计 token（颜色 / 字体 / 圆角 / 叠层）、三窗的透明与滚动策略、交互控件的外观约定，以及主窗设置页的**版面规范**（两级导航、页面容器、`fill` 高度链、解释性 tips 的落点）。以上四件事以本文为**唯一权威来源**。
> **不管什么**：悬浮窗组件在槽位内的缩放与三形态适配（`cell_px` / `--floating-cell-px` / `--floating-cell-scale` / cq 单位 / 格子需求）→ `../guide/floating-module-guide.md`；组件独立样式文件的写法（`@reference`、样式归属边界、类名命名、物理像素与 `scaleFactor` 陷阱）→ 同上 §10；各业务面板自己的具体链路（信息卡拓展 / 会话记录 / 推送）→ `./agent-status-endpoint-flow.md`、`./agent-status-session-data.md`、`../guide/agent-status-push-guide.md`。
> **与其它文档的关系**：token 的**取值**只在这里解释一遍。guide §10.2 的 token 速查表是本文的子集，§7.4 是组件设置页的落地清单，两处若与本文不一致，以本文为准并回改那边。

## 目录

1. [设计意图](#1-设计意图)
2. [Token 的定义位置与双主题机制](#2-token-的定义位置与双主题机制)
3. [颜色 token](#3-颜色-token)
4. [字体与圆角](#4-字体与圆角)
5. [窗口透明策略（三窗）](#5-窗口透明策略三窗)
6. [滚动容器与滚动条槽位](#6-滚动容器与滚动条槽位)
7. [交互控件约定](#7-交互控件约定)
8. [主窗两级导航](#8-主窗两级导航)
9. [主窗页面容器与塌缩](#9-主窗页面容器与塌缩)
10. [`fill` 分区的高度链](#10-fill-分区的高度链)
11. [设置页的解释性 tips](#11-设置页的解释性-tips)
12. [护栏与验收](#12-护栏与验收)

---

## 1. 设计意图

- **窗口（主窗）走极简**：没有渐变、没有厚阴影、不靠装饰性色块分层；层次由「明度差 + 一条 1px 描边 + 一层落影」表达，能少一个视觉元素就少一个。
- **悬浮窗与提示窗走毛玻璃**：半透明浅灰底 + `backdrop-filter`（`--ufo-bg` + `blur(6px)`），落影用中性灰而不是纯黑，深色下才不会发脏（`--ufo-floating-shadow`）。
- **深色是默认档，浅色是同套 token 的另一组取值**，不是第二套设计（见 §2）。
- ⚠️ 后续页面 / 组件**必须复用同一套 token**，禁止再散落硬编码灰色系；`@theme` 镜像与 `:root` 真值不一致时，以 `:root` 为准并顺手修平（见 §2）。

---

## 2. Token 的定义位置与双主题机制

定义全在 `src/assets/main.css`，两处、两种用途：

| 位置 | 前缀 | 给谁用 |
|---|---|---|
| `:root` | `--ufo-*` / `--jse-*` | 运行时真值，所有 CSS 与内联样式都 `var()` 取它 |
| `@theme` | `--color-ufo-*` / `--radius-ufo` / `--font-*` | Tailwind v4 的工具类（`bg-ufo-surface`、`rounded-ufo` 等） |

- ⚠️ `--color-ufo-*` 是 `--ufo-*` 的**镜像**，一一对应、必须一起改。护栏 `src/assets/theme-css.spec.ts` 会检查镜像两边都声明过。（现状：全仓没有一处用 `bg-ufo-surface` 这类工具类，屏幕上的颜色全部经 `var(--ufo-*)` 取得 —— 因此两边不一致时**渲染出来的是 `:root` 那一份**，镜像只是还没爆的雷。）
- **浅色主题**：`:root[data-theme='light']` 逐条覆盖深色档取值。主题名是 `'dark' | 'light'`，默认深色，属于纯表现层偏好，存 localStorage 并由 `src/lib/theme.ts` 落到文档根的 `data-theme` 上，跨窗口靠事件同步（`src/stores/theme.ts`）。⚠️ `color-scheme` 不在 JS 里写，由 CSS 的 `:root[data-theme]` 规则给出，避免两处真相。
- ⚠️ **新增 token 必须同时给浅色档**，否则表现是「深色下一切正常、切浅色后某一处仍然发暗」，肉眼很容易漏。确实与主题无关的（圆角、状态色填充档等）要显式加进 `theme-css.spec.ts` 的 `THEME_INDEPENDENT` 白名单 —— 加进白名单等于声明「我知道它不需要浅色档」，比默默漏掉安全。
- **填充档与文字档是两套**：同一支色当底色填充（按钮 / 灯点 / 色条）与当文字用，需要的明度不同。浅色底上翠绿当文字对比度只有约 1.9:1，因此所有会当文字用的色都有 `-text` 变体（`--ufo-accent-text`、`--ufo-status-*-text`）。⚠️ 写文字色一律取 `-text`，不要图省事用填充档。
- ⚠️ 浅色覆盖块**不在任何 `@layer` 里**（Tailwind 的 `@theme` 变量生成在 `@layer theme`，未分层的规则永远赢过分层规则），别把它塞进 layer，那样整套浅色会静默失效。
- `--jse-*`（脚本编辑器语法高亮）刻意**只进 `:root` 不进 `@theme`**：这套色不对外生成工具类，只给编辑器 CSS 引用。

---

## 3. 颜色 token

以下**「值」列给的是深色档（`:root`）**；浅色档的取值见 `src/assets/main.css` 的 `[data-theme='light']` 块，§2 已说明两边必须成对维护。

### 3.1 背景与表面

| token | 值 | 用途 |
|---|---|---|
| `--ufo-bg` | `rgba(61, 61, 61, 0.95)` | 悬浮窗 / 提示窗的毛玻璃底（配合 `blur(6px)`） |
| `--ufo-bg-main` | `#3D3D3D` | 主窗实色底（无 alpha、无 blur） |
| `--ufo-surface` | `#2A2A2A` | 卡片表面 |
| `--ufo-surface-2` | `#323232` | 次级表面（列表行等） |
| `--ufo-input` | `#1F1F1F` | 输入框底 |
| `--ufo-window-backdrop` | `radial-gradient(circle at top left, #4a4a4a 0%, #1c1c1c 55%), #141414` | `html` 的桌面底纹（渐变 + 压暗）；三窗根类会把 html 设回 `transparent`，因此它是「窗口没打透明」时的兜底，不是可见层 |

⚠️ `--ufo-bg` 的两处写法**当前并不一致**：`:root` 是 `0.95`，`@theme` 镜像 `--color-ufo-bg` 仍是 `0.8`。屏幕上的实际通透感由 `:root` + blur 决定（`bg-ufo-bg` 工具类目前全仓没有消费者）。改这个值时两边一起改，别只动一处。

### 3.2 边框、描边与叠层

| token | 值 | 用途 |
|---|---|---|
| `--ufo-border` | `rgba(255, 255, 255, 0.08)` | 常规边框与分隔线 |
| `--ufo-border-strong` | `rgba(255, 255, 255, 0.2)` | 强边框（输入框 / 幽灵按钮 / 强调分隔线） |
| `--ufo-shell-border` | `rgba(255, 255, 255, 0.14)` | **窗口外壳**那圈描边（主窗 / 悬浮窗 / 提示窗共用） |
| `--ufo-floating-shadow` | `rgba(142, 142, 142, 0.19) 0 6px 15px 0` | 毛玻璃窗（悬浮窗 / 提示窗）的落影 |
| `--ufo-overlay-sunken` | `rgba(0, 0, 0, 0.18)` | 下沉叠层：标题栏、弹窗工具条 |
| `--ufo-overlay-sunken-weak` | `rgba(0, 0, 0, 0.1)` | 下沉最轻一档：一级 tabbar |
| `--ufo-overlay-sunken-strong` | `rgba(0, 0, 0, 0.14)` | 下沉次轻一档：二级胶囊条 |
| `--ufo-overlay-raised` | `rgba(255, 255, 255, 0.06)` | 抬升叠层：hover 底 |
| `--ufo-overlay-float` | `rgba(0, 0, 0, 0.42)` | 浮在编辑区之上的小按钮 |
| `--ufo-overlay-float-strong` | `rgba(0, 0, 0, 0.62)` | 同上，加重档 |

⚠️ 原本写在「边框 / 强边框 `0.14`」名下的那一档，**语义已经拆分**：`--ufo-border-strong` 现在加深到 `0.2`（组件内强调描边），`0.14` 归给了 `--ufo-shell-border`（外壳描边）。选错会让外壳描边莫名变亮或变暗。

### 3.3 文本与强调

| token | 值 | 用途 |
|---|---|---|
| `--ufo-text` | `#F3F4F6` | 主文本 |
| `--ufo-heading` | `#E5E5E5` | 卡片标题：比正文略压暗一档 |
| `--ufo-muted` | `#A3A3A3` | 次文本（说明、副标注） |
| `--ufo-accent` | `#34D399` | 强调**填充**：开关开态、主按钮底、下划线、色块 |
| `--ufo-accent-text` | `#34D399`（浅色档加深为 `#047857`） | 强调**文字**：徽标字、链接 |
| `--ufo-accent-soft` | `rgba(52, 211, 153, 0.16)` | 强调浅底（选中行、分段控件的选中块） |
| `--ufo-accent-on` | `#052E1D` | 强调底上的文字（深绿） |
| `--ufo-danger` | `#F87171` | 危险 / 错误（文字与填充各有档） |
| `--ufo-danger-soft` | `rgba(248, 113, 113, 0.18)` | 危险浅底（关闭按钮 hover 等） |
| `--ufo-ring` | `rgba(52, 211, 153, 0.55)` | 焦点环（`outline` / ring 展开层） |
| `--ufo-selection` | `rgba(52, 211, 153, 0.3)` | 文本选中底 |
| `--ufo-mono-icon-filter` | `none`（浅色档 `invert(1)`） | **仅外壳 / 公共控件**的纯白单色图标反相（close / gear / sun / moon / 主窗 tab）；`.ufo-mono-icon` 消费它 |

⚠️ 反相只对「纯白单色」成立：非单色图标（如 Claude 的橙、Qoder 的白 + 品牌绿）**不得**套 `.ufo-mono-icon` —— 会把彩色翻成青蓝 / 品红。
平台图标这类多档彩色资源一律按档各存一份 svg（见 `src/modules/agent-status/lib/provider-registry.ts` 的 `icon` / `iconLight`），不走滤镜。
- 浅色档的墨色与 token 同值，改 token 时记得一起改文件：`codex-light.svg` / `qoder-light.svg` 的主体 `#18181b` = `--ufo-heading`，Qoder 的绿压深为 `#047857` = `--ufo-accent-text`（`#2ADB5C` 在白底上太飘）；`claude-light` 不需要 —— 品牌橙两档通用。

### 3.4 状态灯色（与指示灯语义一致）

| token | 值 | 语义 |
|---|---|---|
| `--ufo-status-green` | `#34D399` | 常亮 = 正在执行 |
| `--ufo-status-yellow` | `#FBBF24` | 等待用户操作 |
| `--ufo-status-red` | `#F87171` | 出错 |
| `--ufo-status-blue` | `#60A5FA` | 中性 / 信息态（后加的第四档） |

四个填充色各有 `-text` 变体（`--ufo-status-green-text` 等）：⚠️ 填充档两个主题都保持鲜艳（灯点、色条、边框、光晕），文字档在浅色下必须加深（黄、蓝、红在白底上都会糊）。语义的完整定义不在这里，见 `./agent-status-hooks.md`。

### 3.5 控件实体件与滚动条

| token | 值 | 用途 |
|---|---|---|
| `--ufo-switch-track` | `#404040` | 开关的**关闭态轨道**（实色，不是叠层） |
| `--ufo-control-thumb` | `#F4F4F5` | 开关拇指、滑块滑片：一块**实心**圆片 |
| `--ufo-control-thumb-hover` | `#FFFFFF` | 同上 hover 态 |
| `--ufo-scroll-thumb` | `rgba(255, 255, 255, 0.28)` | 滚动条滑块（轨道透明） |
| `--ufo-scrollbar-w` | `auto` | 滚动条粗细的唯一旋钮（`thin` ≈ 8px 才真的变细） |

⚠️ 这些是**实色**而不是半透明白：深色底上是浅色片，浅色底上必须换成白色片，用半透明白去表达会在浅底上直接消失。
⚠️ `scrollbar-width` 只接受关键字：`auto ≈ 15px`、`thin ≈ 8px`；写 px 长度不合法，会**静默**落回 `auto`。

---

## 4. 字体与圆角

| token | 值 | 用途 |
|---|---|---|
| `--font-sans` | Plus Jakarta Sans（回落 system-ui 等） | 全站主字体，`html` 上基准 `14px` |
| `--font-mono` | Cascadia Code → Consolas / Menlo … | 等宽：endpoint、ID、版本号、数字读数 |
| `--ufo-radius` | `12px` | 卡片 / 浮层 / 提示窗的圆角基准（`@theme` 镜像 `--radius-ufo`） |

- 控件（输入框、按钮）用**比卡片更收的一档** `10px`，读起来是「能点的东西」而不是「一块面板」。
- 等宽数字（宽度不抖）用 `tabular-nums`：token 读数、版本号、时间标注都必须带上。

---

## 5. 窗口透明策略（三窗）

| 窗口 | `transparent` | 外壳类 | 底 |
|---|---|---|---|
| `floating` 悬浮窗 | `true` | `.floating-shell` | 半透明 `--ufo-bg` + `blur(6px)`（毛玻璃） |
| `tooltip` 提示窗 | `true` | `.tooltip-shell` | 与悬浮窗**同款**毛玻璃 token 复用 |
| `main` 主窗 | `true`（仅为圆角裁切） | `.main-shell` | 实色 `--ufo-bg-main` |

- ⚠️ 主窗**不加** `backdrop-filter`、**不加** alpha：`transparent: true` 在这里只用来把窗口切成圆角，实色底避开毛玻璃合成开销。想要「玻璃感」是悬浮窗与提示窗的事。
- 三窗各自的根类（`html.is-main-window` / `is-floating-window` / `is-tooltip-window`）把 html / body / `#app` 的 `background` 设回 `transparent`，底只由 `.main-shell` / `.floating-shell` / `.tooltip-shell` 在壳层画出 —— 于是圆角处不外溢。⚠️ 这三个根类**同时**覆盖掉 `html` 上的 `--ufo-window-backdrop`（选择器更具体），所以那层桌面底纹在三个窗口里都不生效；把它当「窗口圆角外的可见底纹」来设计是错的，圆角外透出来的是桌面本身。
- 主窗去原生边框后**自绘标题栏**（`.main-titlebar`）并调用 `startDragging`；关闭按钮走**隐藏到托盘**，不是真退出。
- 三窗外壳描边统一取 `--ufo-shell-border`（§3.2），不要各自写死一条白边。

---

## 6. 滚动容器与滚动条槽位

- 主窗的外壳与 `html` / `body` / `#app` **一律不滚动**（`html.is-main-window` 那组规则锁死 `height: 100%` + `overflow: hidden`）；只有内部的 `.main-window` 滚动，并使用 Matte 细滚动条（透明轨道 + `--ufo-scroll-thumb` 半透明滑块，靠 `scrollbar-width` + `scrollbar-color` 两个标准属性实现）。⚠️ 「细」的程度完全由 `--ufo-scrollbar-w` 决定，它当前是 `auto`（≈15px）而不是 `thin`（≈8px）；想改粗细动这个 token，不要去补 webkit 块（见下条）。
- ⚠️ **所有可滚动容器**（列表 / 详情面板 / 日志 / JS 编辑框）都必须带 `scrollbar-gutter: stable`：滚动条「出现 / 消失」不得改变内容盒宽度，否则列表行与表单会在滚动条出现那一刻被挤窄、长文本重新折行、内容跳动。新增任何 `overflow-y: auto` 容器时把这条一起带上。
- JS 编辑框额外把量出来的槽宽写进 `--jse-gutter`（`ScriptEditorFrame.syncGutter`），高亮层按 `calc(100% - var(--jse-gutter, 0px))` 收窄 —— 两层叠加时**内容盒逐像素重合**，字符与光标才不会错位。常驻槽位（`scrollbar-gutter: stable`）正是为了让这个宽度是个常量。
- ⚠️ 全 App 的滚动条只走标准属性这一套：元素只要设了 `scrollbar-color`，它的 `::-webkit-scrollbar` 自定义规则就整体失效。不要再补 webkit 块 —— 那是改了你看不到的死代码。
- 提示窗 `.tooltip-shell` 自身 `overflow: hidden`，内容靠 flex + 省略号在固定尺寸内自适应（尺寸预算见 `../guide/floating-module-guide.md` §8.3）。

---

## 7. 交互控件约定

| 控件 | 约定 |
|---|---|
| 开关 | 关闭态灰轨 `--ufo-switch-track` → 开态翠绿实心填充；拇指是白圆片（`--ufo-control-thumb`） |
| 输入 | 圆角 `10px`、底 `--ufo-input`、边框 `--ufo-border-strong`；focus 时翠绿边 + `--ufo-accent-soft` 的 3px 软环 |
| 主按钮 | 实心 `--ufo-accent` + 深绿字 `--ufo-accent-on` |
| 次按钮 | 描边幽灵（透明底 + 边框），hover 只抬一档叠层 |
| 危险动作 | `--ufo-danger` 描边 / 文字，`--ufo-danger-soft` 浅底；⚠️ 不得只靠红底色表达危险（行内反馈与两步确认的规矩见 `../guide/floating-module-guide.md` §7.4） |
| 焦点 | 键盘焦点必须可见：`outline: 2px solid var(--ufo-ring)`；`tabpanel` 自己拿焦点时用内嵌细环（`outline-offset: -2px`），否则会完全看不见 |
| 字体 | `--font-sans`；读数与地址走 `--font-mono` |

⚠️ 这些约定都落在 `src/components/ui/` 的公共控件里（`BaseButton` / `BaseInput` / `BaseSelect` / `BaseSwitch` / `BaseSlider` / `BaseSegmentedControl`）。新页面**复用公共控件**，不要另起一套同语义的样式。

---

## 8. 主窗两级导航

两级**都用** `ui/BaseTabs.vue` —— 外观靠 `variant` 区分，键盘导航与 ARIA 关联只有一份实现。

| 级别 | `variant` | 内容 | `idPrefix` |
|---|---|---|---|
| 一级 | `underline`（下划线） | 每个有 `settings` 的组件各一页 + 右端「悬浮窗设置」齿轮页签（`end: true`） | `main` |
| 二级 | `chips`（胶囊） | 当前组件自己的 `settings.sections` | `module` |

- **一级页面由注册表推出来**（`MainWindow.vue`），⚠️ 没有硬编码的 tab 数组；加组件不改这里。
- 主题切换是一个**动作**，走 `BaseTabs` 的 `#actions` 插槽，不是 tab。⚠️ 它必须留在 `role="tablist"` **外面**：tablist 的子项只能是 tab，混进普通按钮会让读屏的「共 N 个标签页、当前第 M 个」计数与方向键漫游都不准，动作钮也不参与 roving tabindex。
- 二级只服务**组件设置页**：只有一个分区就不渲染二级；⚠️「悬浮窗设置」页**一页到底、没有二级** —— 它本身是一块整体（列表 + 预览左右并排 + 版本卡），再切一刀会把「改组件」和「看效果」拆开。
- ⚠️ 两级 tabbar 上下相邻，**外观必须保持不同**（下划线 vs 胶囊），否则看不出哪条是一级。
- 组件名在屏幕上已由两级 tab 表达，⚠️ 组件页里那个 `<h1>` 用 `sr-only`，只留给读屏，不再占一块可见标题。
- **无障碍**：两级都走 WAI-ARIA Tabs —— roving tabindex + `←`/`→` 循环 + `Home`/`End` 跳首尾；id 约定 `${idPrefix}-tab-<key>` / `${idPrefix}-panel-<key>`（`aria-controls` 要指得到面板，调用方按同一规则给面板加 id）。

---

## 9. 主窗页面容器与塌缩

滚动区是 `.main-window`（§6），它按**页面类型**换容器类，两者的差别是「谁滚」：

| 容器类 | 语义 | 用在 |
|---|---|---|
| `.main-window`（默认） | 居中、卡片限宽 `max-w-md`，整页滚 | 内容很少的页 |
| `.main-window--stack` | `flex-1` + `items-stretch` + `overflow-y: auto`，横向铺满、**整页滚** | 「悬浮窗设置」页、无 `fill` 的组件分区 |
| `.main-window--wide` | `flex-1` + `items-stretch` + `overflow: hidden`，横向铺满、**外层不滚** | 带 `fill: true` 的组件分区 |

- **「悬浮窗设置」页（方案 C）**：容器 `.main-window--stack`，一页到底。左列「组件列表 + 格子大小卡 + 版本卡」三张卡纵向堆叠，右列只有三形态预览。两列**等高**（`.module-catalog` 用 `align-items: stretch`）：预览卡 `.module-catalog__preview` 带 `flex-1 + min-h-0` 吃满列高，多出来的空间全给画布。⚠️ 画布里的窗口图示是**绝对定位**的（`.shape-preview__canvas .shape-preview__window`）—— 它的大小由 JS 量画布算出，若参与布局就会「画布变大 → 图示变大 → 卡片更高 → 画布又变大」自激，摘出固有高度这条链当场断掉。窄窗口（`max-width: 760px`）塌成单列。
- ⚠️ 卡片默认带 `max-w-md` 限宽，**横向并排时必须放开**，否则右侧留一大片空白。放开的方式有坑（模块 CSS 排在 `main.css` 之前，单类覆盖会被压回去），见 `../guide/agent-status-push-guide.md` 的「两个类权重」条。
- `.main-window--wide` 内部各列自己滚、占满剩余高度，靠的是 §10 那条高度链。
- 判据一句话：**「内容要多高算多高、滚外层」= stack；「面板占满高度、内层各自滚」= wide**，选错就同时踩 §10 的坑。

---

## 10. `fill` 分区的高度链

- 分区是否占满高度由 `FloatingModuleSection.fill` 声明（`settings.sections` 里的 `fill: true`，版面切法见 `../guide/floating-module-guide.md` §7.1）。
- ⚠️ **凡是 `fill: true` 的分区，高度链的每一层都必须带 `min-h-0` + `flex-1`**：
  `.main-window--wide`（`flex-1` + `overflow: hidden`）→ `.module-page--fill` → `.module-page__section--fill` → 面板自身（`grid` + `flex-1` + `grid-template-rows: minmax(0, 1fr)`）→ 左右两列各自 `overflow-y: auto`。
  公共两层已写在 `main.css` 的 `.module-page--fill` / `.module-page__section--fill`；**面板与列是各分区自己的责任**。
- 缺一环的表现是「退化成整页滚动、面板不占满」；`min-h-0` 不能省，否则 flex 子项被内容顶开。
- 分区自带的标题与说明加 `shrink-0`，免受挤压。
- ⚠️ 别把「占满高度」改回 `max-h`，也别让列表随内容长高：那会把相邻列和贴底按钮挤出可视区。
- **具体面板的完整链路不在这里**：信息卡拓展 → `./agent-status-endpoint-flow.md`；会话记录 → `./agent-status-session-data.md`；推送 → `../guide/agent-status-push-guide.md`。

---

## 11. 设置页的解释性 tips

设置页的所有**解释性**文字一律走问号角标 `ui/SettingsHint.vue`（光标悬浮 / 键盘聚焦 / 点按才展开），不常驻占位。

- **落点只有两个**：
  - 字段级 → 交给 `SettingsField` 的 `hint`（自动渲染在该字段的标签行里）。⚠️ 标签行下面不再放常驻灰字提示。
  - 区块级 → 挂在卡片 / 区块标题旁。⚠️ 标题与角标必须是 `.ufo-heading-row` 的**兄弟节点**，角标**不能塞进标题元素里面**：设置页普遍用 `aria-labelledby` 指向标题，塞进去会让标题的无障碍名称把角标的名字「说明」一起读成「窗口格子 说明」。
- **分工判据**（这条决定了文字该不该藏）：

  | 内容 | 归处 |
  |---|---|
  | 「这东西干什么 / 取值范围 / 为什么这样填」 | tip → 收进问号角标 |
  | 「**当前**是什么状态」= 读数 | **常驻**：`SettingsField` 的 `state`，或 `.ufo-card__hint` 那类徽标 |

  ⚠️ 把读数藏进 hover 等于没写 —— 用户必须不悬浮就知道现状。
- **浮层纪律**：角标面板 `Teleport` 到 body，用纯函数 `placeContextMenu` 定位（贴着锚点摆、越界夹回视口），几何不重写。⚠️ 主窗里**任何**浮层（角标面板 / 下拉 / 右键菜单）都必须 `Teleport`：从 `.main-shell` 到 `.main-window--wide` 有多层 `overflow: hidden`，各分区面板又各自内滚，留在原地的绝对定位一定被裁掉。
- **z-index 阶梯**：`BaseSelect` `40` → 右键菜单 `60` → **说明角标 `70`**（同一套黑投影，浮层看着是同一层东西，角标必须盖住右键菜单）。整窗大浮层（脚本编辑 / 日志）同样落 `z-40`，但它们 `Teleport` 进 `#main-content` 这块相对定位的宿主，边界由它给出 —— 盖住 tabbar 允许，画到 `.main-shell` 圆角外不允许。⚠️ `../guide/floating-module-guide.md` §7.4 写的「modal 50」在代码里没有对应值（只有 40 / 60 / 70），以代码为准。
- ⚠️ 勾选项**自身**的一句话说明与风险告知（如「提问可能含私有代码」）继续用原生 `title` 附着在选项上：每项文案不同，且风险必须落在勾选处 —— 挪到别处就等于没告知。（只对设置页成立；悬浮窗指示灯与信息卡**禁** `title`，见 `../guide/floating-module-guide.md` §8.2。）
- ⚠️ 反馈类文字不是 tip：不要用 `SettingsHint` 装状态提示，行内反馈走 `role="status"` 那套。

---

## 12. 护栏与验收

- `src/assets/theme-css.spec.ts`（`?raw` 读 `main.css`，断言「文件里写着什么」）：
  - 深色档声明的每个 token，要么在浅色档有覆盖，要么在 `THEME_INDEPENDENT` 白名单里；白名单项必须真实存在且**没有**在浅色档重复声明。
  - `--jse-*` 整套必须有浅色对应。
  - 浅色档里每个 `--color-ufo-*` 镜像都必须能在 `@theme` 找到声明。
  - `.ufo-mono-icon` 必须吃 `--ufo-mono-icon-filter`。
- 视觉自查（改 token 后必做）：**深 / 浅两档各过一遍**主窗设置页、悬浮窗三形态、提示窗；只看深色等于没测（§2 漏浅色档的典型表现就是浅色下某处仍发暗）。
- ⚠️ 代码里出现新的硬编码灰色（`#2a2a2a` 一类字面值）就是违规，改成 `var(--ufo-*)`；确实需要中间色时用 `color-mix()` 基于 token 调，而不是另起一个十六进制值。
- 组件侧的样式写法（独立 CSS 文件的 `@reference`、外壳与组件的样式归属边界、类名命名、动画需可被 `prefers-reduced-motion` 关掉、物理像素陷阱）见 `../guide/floating-module-guide.md` §10；组件在槽位内的缩放见同文 §5.5 / §6。
