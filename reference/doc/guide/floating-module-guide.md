# 悬浮窗组件开发规范

> 适用范围：`src/modules/<id>/`（前端）与 `src-tauri/src/modules/<id>/`（Rust）下的所有悬浮窗组件。
> 参考实现：`agent-status` —— 前端 `src/modules/agent-status/`，Rust `src-tauri/src/modules/agent_status/`。
> 与 AGENTS.md 的关系：AGENTS.md 只留 ≤100 行的**稳定规则**（项目概览 / 命令 / 代码规范 / 禁区），悬浮窗架构与契约的细则**唯一落点是本文**，不要再往 AGENTS.md 里塞子系统细节。冲突时以**代码**为准（本文点到的文件名 / 行号可能滞后于实现）。
> 版面与视觉规范（配色 token、主窗两级 tabbar、设置页高度链与卡片版面）见 `../rule/visual-style.md`；本文只管架构与契约，涉及版面处一律写成指向它的交叉引用，不在此展开。
> 一句话版本：**悬浮窗与 tooltip 是容器，组件是插件；新增组件不应当修改任何外壳文件。**

## 目录

1. 第一原则：外壳是容器，组件是插件
2. 目录结构与归属
3. 注册组件：`src/modules/index.ts`
4. `FloatingModuleDefinition` 字段逐个说明
5. 视图组件契约（形态适配）
6. 格子需求 `demand`
7. 设置页与组件配置
8. 信息卡（tooltip）契约
9. 生命周期 `activate`
10. 样式规范
11. 配置持久化与迁移
12. Rust 侧规范
13. 跨窗口通信与 capability
14. 测试规范
15. 验收清单
16. 新增组件 checklist
17. 常见错误与反例
18. 附录：常量与不变量速查

---

## 1. 第一原则：外壳是容器，组件是插件

### 1.1 两个概念

- **外壳（shell / container）**：悬浮窗、主窗口、提示窗的窗口级能力 —— 窗口尺寸与贴边、三/四形态推导、模块挂载与分槽、组件列表、二级导航、tooltip 定位与显隐、配置文件的读写与广播。
- **组件（module / plugin）**：用户实际看到的功能 —— 状态灯、将来的 CPU 监视、闹钟等等。组件自己声明「怎么显示、要多大、有什么设置、悬停给我看什么、启动时要订阅什么」。

### 1.2 三条硬性不变量

1. **外壳不得认识任何具体组件。** 外壳文件里不允许出现组件 id（如 `agent-status`）、组件字段名（如 `token_unit`）、组件的 store / 命令；**注释里举例也只写 `<id>`**（注释同样是外壳文本的一部分，写实名会诱导下一个人去点名组件）。
2. **新增一个组件不得修改外壳。** 如果做新组件时需要改 `src/components/floating/**`、`src/lib/floating-modules.ts`、`src/stores/floatingModules.ts`、`src-tauri/src/floating/**`，正确的动作是**扩展外壳契约**（往 `FloatingModuleDefinition` 加一个通用字段），而不是加一个 `if (id === 'xxx')`。
3. **组件之间互不认识。** 组件不得 import 另一个组件的目录。共用逻辑要下沉到公共层（`src/lib/`、`src/components/ui/`、`src/components/floating/tooltip/`）或各自复制一小段。

组件能触碰外壳的只有**三处**（反过来，外壳能触碰组件的只有注册表）：

1. 注入的 `FloatingModuleApi` —— `allocatedCells` / `reportHover`（`src/lib/floating-module-context.ts` 的 `floatingModuleKey`）。
2. `@/lib/floating-*` 与 `@/lib/tooltip` 导出的纯函数、类型与事件常量。
3. 外壳公开的 store（`useFloatingModulesStore`：组件开关、顺序、`cell_px`、各组件自己的配置块）。

组件目录被外部引用时一律写别名路径 `@/modules/<id>/…`，别让外壳用相对路径钻进组件内部。

### 1.3 落地检查

下面三条命令必须**零命中**（`rg` 无输出 = 退出码 1，这是期望值）：

```powershell
# 外壳与前端的模块 id 泄漏
rg -n "agent-status|agentStatus" src/App.vue src/components/floating src/lib/floating-modules.ts src/lib/floating-module-context.ts
# 外壳与前端通用层里的组件字段名泄漏
rg -n "completed_retention_ms|running_timeout_ms|token_unit|proxy_resolver" src-tauri/src/floating src/lib/floating-modules.ts src/stores/floatingModules.ts
# Rust 外壳里的组件字样
rg -n "agent-status|agentStatus|token_unit|proxy_resolver|completed_retention_ms" src-tauri/src/floating src-tauri/src/mouse.rs src-tauri/src/devtools.rs
```

唯一被允许「点名组件」的地方是**组合根**：

- 前端：`src/modules/index.ts`（注册表）
- Rust：`src-tauri/src/lib.rs`（登记配置钩子、运行时状态、命令）

---

## 2. 目录结构与归属

### 2.1 前端

```text
src/
├─ App.vue                        # 按窗口 label 分派；只认「label + 注册表」两样东西
├─ modules/
│  ├─ index.ts                    # ★ 注册表：新增组件在这里加一条
│  └─ <id>/                       # 一个组件一个目录（id 用短横线）
│     ├─ index.ts                 # 组件定义（FloatingModuleDefinition）
│     ├─ <Name>Module.vue         # 悬浮窗里的视图
│     ├─ <Name>Tooltip.vue        # 悬停信息卡内容（可选）
│     ├─ demand.ts                # 格子需求（纯函数）
│     ├─ settings.ts              # 组件自己的配置：字段 / 默认值 / 范围 / 解析
│     ├─ <id>.css                 # 组件样式（自己的）
│     ├─ lib/                     # 组件的纯逻辑 + 单测
│     ├─ stores/                  # 组件自己的 Pinia store
│     ├─ sections/                # 设置页分区组件
│     └─ panels/                  # 分区里的面板（体积较大、给某个分区专用）
├─ components/
│  ├─ floating/                   # 外壳（容器侧，零组件语义）
│  │  ├─ FloatingWindow.vue       #   拖拽 / 贴边 / 形态 / 窗口尺寸 / 提示窗路由 / 点组件开设置页
│  │  ├─ FloatingModuleHost.vue   #   宿主：挂载启用组件，汇总格子需求 → 窗口尺寸，按需求分槽
│  │  ├─ FloatingModuleSlot.vue   #   槽位：注入 floatingModuleKey，根节点标 data-module-id
│  │  ├─ FloatingModulesPanel.vue #   「悬浮窗设置」页：组件列表（开关 / 齿轮 / 排序）+ 格子大小 + 版本卡
│  │  ├─ FloatingShapePreview.vue #   三形态预览：当前形态放大 + 颜色↔组件图例 + 其余两态缩略
│  │  └─ tooltip/                 #   TooltipRow.vue / TooltipSection.vue（信息卡通用行与分组）
│  ├─ ui/                         # 通用控件（BaseButton / BaseSelect / BaseTabs / ScriptEditor…）
│  ├─ MainWindow.vue              # 主窗外壳：一级 + 二级 tabbar
│  └─ TooltipWindow.vue           # 提示窗外壳：毛玻璃 + 定位 + 显隐
├─ lib/                           # 外壳公共层
│  ├─ floating-layout.ts          #   形态判定 + 窗口尺寸口径（纯函数）
│  ├─ floating-modules.ts         #   定义类型 + 需求汇总 + runtimeSnapshot + activateModules
│  ├─ floating-module-context.ts  #   floatingModuleKey（宿主 ↔ 组件的注入契约）
│  ├─ floating-bridge.ts          #   悬浮窗 ↔ 主窗的跨窗口事件常量与载荷
│  ├─ tooltip.ts                  #   提示窗分派与定位（resolveTooltipDefinition / placeTooltip）
│  └─ snap.ts / window.ts / js-*  #   吸附、窗口动作、脚本编辑器公共件
├─ stores/floatingModules.ts      # 悬浮窗配置（开关 / 顺序 / cell_px / 各组件配置块）：窗口级数据，属于外壳
└─ assets/main.css                # ★ 只留全局 token 与外壳样式
```

**归属判断口诀**：把某个文件删掉，会不会有第二个组件受影响？

- 会 → 放公共层（`src/components/ui/`、`src/components/floating/tooltip/`、`src/lib/`）。
- 不会 → 放组件目录。

⚠️ `src/App.vue` 是**唯一**按窗口 label 分派三窗的地方，同时拉起两份窗口级 store（`useFloatingModulesStore().start()` 与 `useThemeStore().start()`）再调 `activateModules`。三窗都会各起一次，所以漏起某个窗口，那个窗口就会永远停在 `fallbackConfig` 的默认值上（历史坑：提示窗不起配置 store → token 单位永远不跟随主窗切换）。加组件**不要**改这个文件。

### 2.2 Rust

```text
src-tauri/src/
├─ lib.rs                         # ★ 组合根：唯一点名组件的地方（窗口 / 托盘 / 命令注册 / 状态装配 / 钩子登记）
├─ main.rs                        # 入口：`--status-hook` 快进快出，其余全交给 Tauri
├─ devtools.rs / mouse.rs         # 外壳：调试开关 / 鼠标按键状态
├─ floating/                      # 悬浮窗外壳（对应前端 `src/lib/floating-*` + `stores/floatingModules.ts`）
│  ├─ mod.rs
│  └─ module_config.rs            # 配置存取 / 迁移钩子 / 变更钩子：module_settings 整块透传，不认字段名
└─ modules/
   ├─ mod.rs                      # pub mod <id>; 一行一个组件
   └─ <id>/                       # 目录名 = 组件 id 的下划线写法（agent-status → agent_status）
      ├─ mod.rs                   # 只写模块声明与文档注释
      ├─ settings.rs              # 组件配置的解释者（与前端 settings.ts 同口径）
      └─ …                        # 状态、命令、采集、存储
```

⚠️ 目录名映射：前端 id `agent-status` ↔ Rust 目录 `agent_status`。这是唯一的命名换算点，别在别处再造花样。引用约定见 12.1。

### 2.3 哪些东西属于外壳（不要往组件里搬）

| 能力 | 位置 |
|---|---|
| 按 label 分派三窗 + 起窗口级 store + 调 `activateModules` | `src/App.vue`（加组件不需要动它） |
| 窗口尺寸 / 贴边 / 形态推导 | `src/components/floating/FloatingWindow.vue`、`src/lib/snap.ts`、`src/lib/floating-layout.ts` |
| 模块挂载与分槽（按需求比例切窗口） | `FloatingModuleHost.vue`、`FloatingModuleSlot.vue` |
| 需求汇总 / 运行时快照的纯函数 | `src/lib/floating-modules.ts`（`moduleDemand`、`resolveWindowCells`、`moduleCellShares`、`allocatedCells`、`runtimeSnapshot`） |
| 跨窗口事件常量与载荷类型 | `src/lib/floating-bridge.ts` |
| 组件启用 / 排序 / 一格边长 / 各组件配置块 | `src/stores/floatingModules.ts`、`src-tauri/src/floating/module_config.rs` |
| 组件列表与齿轮入口、窗口级「格子大小」卡（左列，组件列表下方） | `src/components/floating/FloatingModulesPanel.vue` |
| 三形态预览（读同一份运行时快照） | `src/components/floating/FloatingShapePreview.vue` |
| 一级 / 二级导航（版面规范见 `../rule/visual-style.md`） | `src/components/MainWindow.vue` |
| 提示窗分派 / 定位 / 显隐 / 尺寸 / 点击穿透 | `src/components/TooltipWindow.vue`、`src/lib/tooltip.ts` |
| 通用信息卡行与分组 | `src/components/floating/tooltip/{TooltipRow,TooltipSection}.vue` |

---

## 3. 注册组件：`src/modules/index.ts`

注册表长这样（新增组件就是在数组里加一条）：

```ts
import type { FloatingModuleDefinition } from '@/lib/floating-modules'
import { agentStatusModule } from './agent-status'
import { clockModule } from './clock'

/** 数组顺序 = 主窗口组件列表的展示顺序；悬浮窗内的先后按配置里的 order。 */
export const FLOATING_MODULE_DEFINITIONS: FloatingModuleDefinition[] = [agentStatusModule, clockModule]
```

注册即生效的四处：

1. 悬浮窗宿主挂载它（启用 + 形态支持时）。
2. 主窗口组件列表出现一行（含启停开关、齿轮）。
3. 主窗口一级 tabbar 多一页（**仅当**声明了 `settings`）。
4. 三窗口挂载时调用它的 `activate`。

⚠️ 列表顺序只是**首次出现的顺序**；用户拖拽排序后以配置文件里的 `order` 为准（`orderedModuleDefinitions`）。

主窗的组件列表与页面**都由注册表直接渲染**（`FloatingModulesPanel.vue` / `MainWindow.vue` 遍历定义，名字 / 说明 / 默认态全部取自定义），所以加一条注册就多一行、多一页，**不用改这两个文件**。配套的三个外壳口子：

- 启停：`useFloatingModulesStore().updateModuleEnabled(id, enabled)`。
- 排序：`updateModuleOrder(ids)` —— 拖拽与手柄上的 ↑/↓ 写的是同一个 `order` 字段，配置里还没有该组件时就地补一条。
- 兜底：配置里没有这个 id 时按定义的 `defaultEnabled` 走（`isModuleEnabled`），因此**新组件不需要写任何配置迁移**。

「悬浮窗设置」页的版面与一级 tabbar 齿轮页签规范见 `../rule/visual-style.md`；本文只补一条实现事实：原「关于」页已删，版本号是左列最下面那张卡（`AboutPanel`）。

---

## 4. `FloatingModuleDefinition` 字段逐个说明

定义在 `src/lib/floating-modules.ts`。类型是外壳的一部分，**新增字段 = 改外壳契约**，改完所有组件都要能编译通过。

| 字段 | 必填 | 说明 | 约束与坑 |
|---|---|---|---|
| `id` | ✅ | 组件唯一 id，同时是配置里的键 | 只允许字母 / 数字 / 短横线；一旦发布**不要再改**（改 = 丢掉用户配置与存档）。后端**没有 id 白名单**，只校验长度（≤64）与字符集（不含 `/` `\`），因此新组件不需要后端登记 |
| `label` | ✅ | 组件列表里的名字 | 4–8 个汉字最佳，别写成句子 |
| `description` | ✅ | 组件列表里的一句话说明 | 说清「它显示什么」，不要写实现细节 |
| `tint` | ❌ | 三形态预览里的身份色（任意 CSS 颜色，建议给 token 而不是字面色值） | 外壳**不解释**取值，只原样画进色块与图例点；省略时预览回落强调色。⚠️ 只服务预览，不参与组件自己在悬浮窗里的配色 |
| `version` | ✅ | 组件自己的版本号 | 供将来做组件级配置迁移；与配置的 `schema_version` 不是一回事 |
| `defaultEnabled` | ✅ | 配置里没有该组件时的兜底启用状态 | 新组件建议 `false`（避免用户升级后被突然出现的东西吓到）；状态灯这类核心组件为 `true` |
| `supportedLayouts` | ✅ | 支持哪些形态 | 至少要支持 `square`；不含当前形态时宿主**不挂载它、它也不参与格子汇总**（等于自动让位，因此别指望它在别的形态下存活） |
| `component` | ✅ | 悬浮窗里的视图组件 | 见第 5 节契约 |
| `demand` | ❌ | 该形态下要占几格（纯函数） | 见第 6 节；省略 = 恒占 1 格。⚠️ 只能读组件自己的数据与传入形态，**不得**反过来依赖 `allocatedCells`（回路） |
| `settings` | ❌ | 设置页（分区数组） | 省略 / 空数组 = 列表里不显示齿轮，一级 tabbar 也不出现该组件页。⚠️ 按**版面**切分区，不是一个配置项一个 tab（见 7.1） |
| `tooltip` | ❌ | 悬停信息卡（`component` / `size` / `hasContent`） | 见第 8 节；省略 = 悬停不弹窗。`size` 可以给**按悬停目标算的函数**（行数随目标变化时窗口跟着长高） |
| `activate` | ❌ | 生命周期（三窗口挂载时各调一次） | 见第 9 节；**返回清理函数**，卸载时逆序调用 |

### 4.1 完整骨架（可以直接抄）

```ts
import type { FloatingModuleDefinition } from '@/lib/floating-modules'
import { useClockStore } from './stores/clock'
import { clockDemand } from './demand'
import ClockModule from './ClockModule.vue'
import ClockTooltip from './ClockTooltip.vue'
import ClockSection from './sections/ClockSection.vue'
import './clock.css'

export const clockModule: FloatingModuleDefinition = {
  id: 'clock',
  label: '时钟',
  description: '当前时间与日期，悬停看时区与秒。',
  version: 1,
  defaultEnabled: false,
  // 只支持能正常显示的形态；不支持就别勉强声明，宿主会按需跳过。
  supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
  component: ClockModule,
  demand: clockDemand,
  settings: {
    sections: [{ id: 'general', label: '常规', component: ClockSection }],
  },
  activate: () => {
    const store = useClockStore()
    store.start()
    return () => store.stop()
  },
  tooltip: {
    component: ClockTooltip,
    size: { width: 260, height: 180 },
    hasContent: () => true,
  },
}
```

---

## 5. 视图组件契约（形态适配）

### 5.1 输入

视图组件只从两个通道拿信息：

```ts
// 1) prop：当前形态（宿主强制传入，不要自己读贴边状态）
const props = defineProps<{ layoutMode: FloatingLayout }>()

// 2) inject：宿主注入的运行时上下文
const moduleApi = inject(floatingModuleKey, null)
// moduleApi.allocatedCells : ComputedRef<number>  本模块实际分到几格
// moduleApi.reportHover(key | null)               上报 / 取消悬停对象
```

`inject` 必须给默认值（`null`）：组件要能被单独渲染（单测、预览页），缺上下文时退化成「独占窗口」。

### 5.2 四种 layout（对用户是三形态）

| layout | 触发条件 | 形状 | 备注 |
|---|---|---|---|
| `square` | 未贴边 | 1×1 格（默认 45×45） | **与格子数无关、恒定一格**：多组件时窗口不放大，内容按比例压缩；只有贴角 / 贴边才按格子数长尺寸。一格边长可配（见 11.1 的 `cell_px`） |
| `corner` | 贴四角 | 各模块**竖向堆叠**，每个占满一整行：行数 = 各模块行数之和、列数 = 最宽模块的列数（`windowGrid`） | 空间比 `square` 宽裕；**单模块**仍是它自己的阶梯网格 1×1…3×3，**多模块**时每个至少占满一整行 |
| `horizontal` | 贴上下边 | 横条，宽 = cells × 1 格 | **高度只有 1 格**，纵向极其紧张 |
| `vertical` | 贴左右边 | 竖条，高 = cells × 1 格 | **宽度只有 1 格**，横向极其紧张 |

> **四角的多模块堆叠**（写 `demand` / 适配 `corner` 前必读）：四角里宿主把各模块**上下堆叠**，每个模块占满一整行，**窗宽由最宽的模块决定**，其余模块哪怕只要 1 格也会被拉到同一宽度。举例：agent 在四角是 1×2、todo / weather 也各是 1×2，三模块整窗就是 **3 行 × 2 列（3×2）** —— 而不是把「需求之和 6」归档成 2×3 的方格。因此：
> - 你的模块在四角**横向会和最宽的同伴一样宽**（可能比你 `demand` 需要的更宽）→ 横向用 grid `1fr` / 省略号铺满，别写死宽度；
> - 每个模块**至少占一整行**（行高 = 一格）；`demand` 想要 2×2 / 3×3 的多行块也支持（占满连续的几行）；
> - 窗口形状**不是**「总数开平方」→ 算自己那块的行列一律用 `cellsGrid(layout, cellBudget)`，别自己推整窗。

> ⚠️ `FLOATING_BASE_CELL_PX`（45）与 `src-tauri/tauri.conf.json` 里 floating 窗口的初值**必须同为 45**：那是「用户还没配过」时的默认一格，只改一边会让启动瞬间的尺寸与吸附口径对不上（先塌一下再重算）。它只是默认值，用户可在「悬浮窗设置」里调，见 11.1。

**必须做与必须不做**：

- ✅ 用 grid / flex 比例、`minmax(0, 1fr)`、`clamp()`、省略号自适应。
- ✅ 用 `cellsGrid(layout, cells)`（`src/lib/floating-layout.ts`）换算行列，别自己写公式。
- ❌ 不要写死 `width: 45px` / `height: 240px` 这类固定像素布局（要按槽位写 `clamp`，见 5.5）。
- ❌ 不要假设「窗口一定比我需要的大」。
- ❌ 不要给元素加 `title`（会触发 Windows 原生提示，和 tooltip 重复且更简陋）；无障碍信息只走 `aria-label`。

### 5.3 被挤小时必须优雅退化

窗口总格数 = 各模块需求之和，夹在 `[ABS_MIN_CELLS(1), 该形态上限]`（`resolveWindowCells`；地板取 1 而不是 `DEFAULT_WINDOW_CELLS`，否则边条按个数缩到 1 格会被抬回默认值 —— 那个常量只是宿主算出真实格子数**之前**的兜底）。地板之上窗口随需求之和长大（Agent 吃到 3×3 再叠几个同级组件，整窗会长到 3×4 / 4×4 容下它们），到顶后各模块**按需求比例等分收缩**，所以你拿到的 `allocatedCells` 可能小于自己的 `demand`：

宿主分槽的方式：`moduleCellShares(demands)` 把各模块需求折成整数权重，grid 用 `fr` 按权重切 —— **横条左右并排，竖条 / 方形 / 四角上下堆叠**。组件不需要参与这件事，只要接受「我可能只分到比需求少的那一刀」。

```ts
// 自身需求与宿主分配取小 —— 一旦被同级模块挤小，就少显示几个
const cellBudget = computed(() => Math.min(demand.value, moduleApi?.allocatedCells.value ?? demand.value))
// 放不下的用最后一格显示 +N，绝不允许溢出裁切或撑破窗口
const partitioned = computed(() => partitionIndicatorSessions(store.sessions, props.layoutMode, cellBudget.value))
```

参考实现：`src/modules/agent-status/lib/agent-status-layout.ts` 的 `partitionIndicatorSessions`。

### 5.4 DOM 约定

- 根节点 class 用 `.<id>-module`，形态修饰类 `.<id>-module--<layout>`（如 `.agent-status-module--horizontal`）。
- 根节点**不要**自己加 `data-module-id`：那是 `FloatingModuleSlot` 的职责（外壳靠它把「点在我这块」翻译成「打开这个组件的设置」）。
- 悬停：`@pointerenter="moduleApi?.reportHover(key)"` / `@pointerleave="moduleApi?.reportHover(null)"`。`key` 由组件自己定义与解释，外壳只当字符串转发。
- 组件不要主动 popup / 弹窗 / 自己定位 tooltip —— 那是外壳的能力。

### 5.5 按槽位尺寸自适应（容器查询）

槽位 `.floating-module-slot` 由外壳声明为 **size 容器**（`container-name: floating-slot`），组件因此能知道「宿主这一刀切给我多少像素」，并按槽位的真实尺寸降级 / 放大。这是唯一可靠的依据 —— `allocatedCells` 只说分到几格，而同样 1 格在 `square`（45×45）与 `vertical`（45×h）里宽度相同、高度完全不同。

```css
/* 槽位矮到摆不下正常排版：退化成「图标 + 一行数值」，任何尺寸都不裁切 */
@container floating-slot (max-height: 33px) {
  .demo-module {
    flex-direction: row;
    gap: 6cqw;
  }
  .demo-module__icon {
    width: clamp(10px, 78cqh, calc(24px * var(--floating-cell-scale, 1)));
    height: clamp(10px, 78cqh, calc(24px * var(--floating-cell-scale, 1)));
  }
  .demo-module__value {
    font-size: clamp(11px, 58cqh, calc(19px * var(--floating-cell-scale, 1)));
  }
}
```

规矩：

- ✅ 悬浮窗里的尺寸统一写 `clamp(下限, Xcqh, 上限)`（`cqh` = 槽位高、`cqw` = 槽位宽），一套规则覆盖「未贴边 1 格」到「贴边 8 格」，不用为每种格子数再写一层。
- ✅ **上限本身也要跟着格子缩放**：上限写死的 px 会让组件在用户调大「格子大小」后停止长大（`cqh` 变大、clamp 却卡住）。宿主按配置的 `cell_px` 在 `.floating-module-host` 上声明两个变量：
  - `--floating-cell-px`：一格实际多少像素（原始值，便于将来按格数排版）；
  - `--floating-cell-scale`：相对设计基准 `FLOATING_BASE_CELL_PX`（45px）的倍率，1 = 设计尺寸。

  于是所有**设计像素**（字号、图标上限、描边、光晕、间隙）都写成 `calc(<设计像素> * var(--floating-cell-scale, 1))`。变量缺失时回落 1（= 设计尺寸），正好与「没有槽位容器时 cq 退化、clamp 取到设计上限」的表现对齐，所以在主窗预览 / 单测里也安全。
  ⚠️ 别把 `var()` 的兜底写成别的数：1 之外的兜底会让「没有外壳」的场景跟设计尺寸对不上。
- ✅ 降级要有取舍：先丢次要信息（城市、湿度、平台图标），再缩尺寸。
- ✅ **百分比尺寸要取「限制轴」的**：槽位宽高不等时（竖条只有一格宽、行高≈一格），百分比只有落在**更短的轴**上才不会溢出。实测竖条 45 格：行宽 39.5px、行高 43.2px，灯点 + 间隙 + 图标按行高算是 42.4px —— 超出的近 3px 被槽位的 `overflow:hidden` 切掉，图标缺一角。做法是按形态换轴、比例不动；竖条另外按格子倍数留左右内边距，别让灯点贴住边框（一格宽的形态没有额外余量，光靠居中不够），内边距同样按 `--floating-cell-scale` 缩放。
- ⚠️ 降级查询要覆盖两条轴：`@container (max-height: …)` 管横条 / 方形，`@container (max-width: …)` 管竖条；拆成两个块写（`or` 语法要求较新的内核，拆开在任何版本都不会整块失效）。
- ⚠️ 形态规则的选择器如果比降级规则更长（如 `.demo-module--vertical .demo-module__dot`），降级块里必须**带上同一条选择器**才盖得住它 —— 否则降级静默失效（灯点该吃满一行时仍是小圆点）。
- ⚠️ **cq 单位只在有槽位容器的前提下才是「槽位尺寸」**：没有祖先容器时（主窗口预览、单测直接挂组件）`cqh` 会按规范退化成**视口**单位，图标能瞬间撑满整屏。所以规则是「`@container` 块内放心用」+「块外一律用 `clamp(下限, Xcqh, 上限)` 把退化情形夹住」——配上面那条 `--floating-cell-scale` 的兜底 1，退化时就正好是设计尺寸。
- ⚠️ **不乘 `--floating-cell-scale` 的只有两类**：① `@container` 里的 px 断点（`max-height: 32px` 这类）—— 它的语义是「槽位小于这么多像素就看不清」，是**绝对可读性阈值**，跟设计缩放无关，乘了倍率会让大格子下的降级永远不触发；② **信息卡（tooltip）** —— 它是另一个固定尺寸的窗口，不参与格子缩放（见 8.3）。除此之外组件里任何 px 设计值都要乘。
- ⚠️ **留白只有外壳那一层**：默认一格 45px 时可内容只有 ≈39.5px —— 外壳在窗口最外层 `.floating-shell` 上留 1px 描边 + `p-0.5` 内边距当呼吸位（本项目 `--spacing` 是 3.5px，即 ≈1.75px），**宿主与组件一律不再加 padding**（组件叠一层 padding 会把内容挤出槽位，而槽位是 `overflow: hidden`，表现为右边整列被切掉）。一格边长可调之后更要靠 `cqh` / `cqw` 而不是写死像素。
- ⚠️ 槽位是 `overflow: hidden`：任何方向都不允许出现滚动条或被裁的元素，宁可降级。
- ❌ 不要用 `@media`（那是视口，不是槽位）；不要用 JS 量尺寸（外壳不为组件做布局，容器查询在布局期就有答案）。
- ❌ 不要因为「槽位看起来很小」就把内容整块藏掉 —— 用户什么都看不到，比看得少更糟。
- 🧪 护栏：`src/components/floating/floating-module-scaling.spec.ts` 会用 `?raw` 读所有模块样式，**`clamp()` 的上限只要是 px 设计值就必须引用 `--floating-cell-scale`**（上限是 cq 单位的不管）；`src/modules/agent-status/agent-status-css.spec.ts` 另外锁住「一行百分比之和 < 100%」与「竖条按行宽取百分比 + 窄槽降级盖得住它」。

参考实现：`src/modules/weather/weather.css`（正常排版按 cqh 缩放 + 矮槽位兜底成一行）、`src/modules/agent-status/agent-status.css`（竖条换成按行宽取百分比；槽位过矮 / 过窄时藏掉平台图标、把灯点吃满一格）。

---

## 6. 格子需求 `demand`

### 6.1 为什么声明在定义里，而不是组件自己上报

宿主需要在**不挂载组件**的前提下算出任意形态下窗口该多大 —— 主窗口的组件列表徽标与三形态预览就是这么来的。所以需求是「定义的一部分」，不是「组件的运行时状态」。

一次算全的入口是纯函数 `runtimeSnapshot(current, config, definitions, layoutMaxFor)`（`src/lib/floating-modules.ts`），返回 `{ layout, byLayout }`：`byLayout` 逐形态给出「moduleId → 需求格数」与该形态的窗口总格数。悬浮窗把它广播出去（`ufo-floating-runtime`，补发请求见 13.2），主窗的列表徽标与 `FloatingShapePreview.vue` 读的都是同一份 —— 因此**预览与真窗口永远不会算出两个尺寸**。

预览的画法是**方案 C**：只把**当前形态**按真实比例放大画进画布，旁边配一份「颜色 ↔ 组件 ↔ 需求格数」图例，其余两态降成缩略索引（各带自己的像素读数）。预览只列**三组**：四角 / 横条 / 竖条 —— 未贴边的方形折进「四角」那一组（它整窗就一格，就是那一组最短的形态），但当前那一组按**实际形态**画：未贴边时标题写「四角 · 未贴边」，读数仍是 1×1 · 一格像素，不会拿贴角的比例冒充现状。⚠️ 早期版本把三态各自等比缩进同样大的框，于是 1 格与 16 格看着一样大（比例不可比）、三块同色也读不出谁是谁；改画布尺寸时**不要**再引入「每组一个上限」的算法。色块的分槽权重与宿主同源（横条按列宽、其余按行高），⚠️ **块的大小只由格数决定，名字不参与**：`.shape-preview__block` 必须留 `flex-basis: 0`（flex 默认 `auto` 会把标签的固有宽度算进起点，长名字的块就比它实际占的格数更宽，「按真实比例画」当场作废）。名字太窄放不下时不渲染（宽 <22px 或高度连一行 11px 都装不下），放得下就**在块内折行**，行数按块高度算好塞进 `--label-lines`；不许改回 `nowrap` + 省略号。完整信息另外走 `title`。

```ts
// src/modules/<id>/demand.ts
import type { FloatingLayout } from '@/lib/floating-layout'
import { useClockStore } from './stores/clock'

/**
 * 时钟模块的格子需求：只在方形 / 四角要 2 格（要放两行），边条形态 1 格。
 * 纯函数：只依赖形态与组件自己的配置，**不依赖宿主分配了多少格**。
 */
export function clockDemand(layout: FloatingLayout): number {
  return layout === 'square' || layout === 'corner' ? 2 : 1
}
```

### 6.2 规则

- **纯函数**：只读形态与组件自己的 store / 配置。
- ❌ 绝对不要读 `allocatedCells` 或宿主给的任何运行时值 —— 那会形成回路（窗口大小 ← 需求 ← 分配 ← 窗口大小），表现为尺寸抖动或死循环。
- 返回值：正整数格数；`NaN` / `Infinity` / 小数 / **0** 会被 `moduleCellWeight` 归一成「至少 1 格的整数」。
- ❌ 不要用「返回 0」表达「这个形态下我不想占地方」—— 0 照样按 1 格算。要让某形态不占空间，用主窗口的开关把组件关掉，或把 `supportedLayouts` 写窄（不支持的形态不挂载、不汇总，见第 4 节）。
- 省略 `demand` = 恒占 1 格，这是大多数小部件的正解。
- 需求之和决定各模块**分到多少格**：`resolveWindowCells(demands, ABS_MAX_CELLS)`，夹在 `[1, 16]`。**单组件自己要几格的封顶不在外壳配置里**：各组件在自己的 `module_settings[<id>]` 声明（如 Agent 的 `max_cells`，滑块 `{1,2,3,4,6,9}` 封顶 3×3），只限本组件、不改整窗口径（见 11.1）。
- 但**窗口的像素形状**另算（`windowGrid` / `floatingWindowSize`）：边条按个数精确取长度、方形未贴边恒 1 格、**四角逐模块竖向堆叠**成「行数之和 × 最宽列数」（见 5.2）；只有**单模块**的四角才把需求归档到阶梯 `{1,2,3,4,6,9,12,16}`。所以别用「总数开平方」推整窗形状。
- 需求变大只影响「窗口能多大」，**不保证**你拿到这么多（见 5.3）。

### 6.3 什么时候该多要格子

| 情形 | 建议 |
|---|---|
| 单一图标 / 单行文字 | 1 格 |
| 2×2 网格、两行两列 | 2–4 格 |
| 需要横条里并排多项 | 横条形态按「项数 × 1 格」申请 |
| 数据量随会话数变化 | 用「上限格数」申请，用溢出格 `+N` 兜底（**别**让窗口随数据无界增大） |

---

## 7. 设置页与组件配置

### 7.1 设置页结构：分区（section）

```ts
settings: {
  sections: [
    { id: 'general',    label: '常规',     component: GeneralSection },
    { id: 'extensions', label: '信息卡拓展', component: ExtensionsSection, fill: true },
    { id: 'records',    label: '会话记录',  component: RecordsSection,    fill: true },
  ],
}
```

- 分区 = 二级 tabbar 的一项。**切法是按版面**，不是「一个配置项一个 tab」：短表单一页装完，需要占满高度、左右各自滚动的面板各占一页。
- 分区顺序 = tab 顺序，**第一个是默认落地分区**（点齿轮、点悬浮窗该组件都落到它）。
- 只有 1 个分区时**不渲染二级 tabbar**（一个 tab 的 tabbar 是纯噪音）。
- `fill: true` 的含义：该分区占满内容区高度（外层不滚）。列表 / 编辑器这类「左右两列各自撑满、各自滚动」的版面**必须**设 `fill: true`，否则高度链断掉，退化成「有多高算多高 + 整页滚动」。
- 设置页组件只拿到「我自己这一页」，拿不到别的分区；跨分区共享状态请放组件自己的 store。

### 7.2 组件配置的读写

外壳只提供两个口子，组件**不得**直接改配置对象：

```ts
const modules = useFloatingModulesStore()
// 读：原样返回自己那块 JSON（没配置过 = 空对象）
const raw = modules.moduleSettings(AGENT_STATUS_MODULE_ID)
// 写：浅合并到自己的块上，外壳负责落盘 + 广播 + 即时生效
await modules.updateModuleSettings(AGENT_STATUS_MODULE_ID, { token_unit: 'm_tokens' })
```

- 外壳**不解释**字段：加一个配置项不需要改外壳，也不需要写迁移（读不到键就回落到组件默认值）。
- 写入失败的 `invoke` 会 throw，设置页要自己 catch 并给行内提示。

### 7.3 `settings.ts`：组件配置的唯一解释者

一个组件一个 `settings.ts`，内容固定是这五样：

```ts
export const CLOCK_MODULE_ID = 'clock'                 // 1. 配置键（= 注册表 id）
export interface ClockSettings { use_24h: boolean }     // 2. 强类型
export const DEFAULT_CLOCK_SETTINGS: ClockSettings = …  // 3. 默认值
export function clockSettings(config: FloatingModuleConfig | null): ClockSettings { … } // 4. 解析 + clamp
export function fetchClockLocales(): Promise<string[]> { … } // 5. 按需拉取下拉数据
```

**解析函数必须逐字段宽松取值**（而不是整块 `as ClockSettings`）：

```ts
export function clockSettings(config: FloatingModuleConfig | null): ClockSettings {
  const raw = (config?.module_settings?.[CLOCK_MODULE_ID] ?? {}) as Record<string, unknown>
  return {
    use_24h: typeof raw.use_24h === 'boolean' ? raw.use_24h : DEFAULT_CLOCK_SETTINGS.use_24h,
  }
}
```

原因：用户手改配置文件把某个值写错时，**只该回落那一个字段**，不能让整块配置失效、连带丢掉其它设置。范围类字段（秒 / 毫秒 / 格数）一律在这里 `clamp`。

⚠️ **Rust 侧必须有一份同口径实现**（`src-tauri/src/modules/<id>/settings.rs`）：字段名、默认值、上下限、解析宽松度都要对齐，否则会出现「主窗口显示 30 秒、实际生效 60 秒」这类灵异现象。两份实现的单测要点见第 14 节。

### 7.4 设置页 UI 规范

- 布局走 `.ufo-card` / `.ufo-card__title` 这类外壳 token 类；两列卡片用 `grid` + `items-start`，**不要让左右互相拉伸**。
- 左列表卡固定高（`height: clamp(300px, 46vh, 520px)` + flex 列 + 列表 `flex-1` 内滚），宽度随网格列 flex。别用 `max-h` —— 那会让卡片随内容长高、撑开右列，并把底部按钮挤出可视区。
- 可滚动容器必须带 `scrollbar-gutter: stable`，滚动条出现时不得挤占内容宽度。
- 反馈**不得**用 `window.alert` / `window.confirm`（项目装了 `tauri-plugin-prevent-default`，原生对话框会被拦）。一律走 `role="status"` 行内提示；删除类操作走两步确认。
- 主窗口 `.main-window` 是唯一滚动容器（外壳与 html/body 不滚）。
- **窗口级参数放「悬浮窗设置」页，组件级参数放组件自己的页**：判据是「换个组件它还成不成立」。一格多少像素（`cell_px`）是前者（整个悬浮窗的设置）；格子上限已下放给各组件自己（判据：换个组件它就不成立），放组件自己的设置页。
- 滑块（`ui/BaseSlider.vue`）**拖动时只改本地值、松手（change）才落盘**：拖一次会连发几十个 input 事件，每个都写盘 + 触发一次真窗口重新吸附是不可接受的。配套规矩是「预览跟着本地值走」——用户拖的时候必须看得见结果，否则滑块就是个盲操控件。
- ⚠️ 「落盘前 clamp」这条防线**主要挡程序化调用**，不是挡用户：原生 `range` 自己会把越界值截到边界，所以界面上拖不出 0；但 `updateCellPx(0)` 这类直接调用会让窗口塌成一条线。前端 `clampCellPx()` 与 Rust 侧 `validate()` 各管一半，两边都要留（读配置一律走 `configCellPx()` / `cellSizeOf()`，别直接取字段）。
- ⚠️ 测试里注意：`@vue/test-utils` 的 `setValue()` 会连发 input + change（等于「拖到底并松手」）。只想模拟拖动就手动改 `element.value` 再 `trigger('input')`。
- **解释性 tips 收进问号角标**（`ui/SettingsHint.vue`：光标悬浮 / 键盘聚焦 / 点按才展开）。落点只有两个：字段级交给 `SettingsField` 的 `hint`（自动渲染在标签行里），区块级挂在卡片 / 区块标题旁。**分工判据**：「这东西干什么 / 取值范围 / 为什么要这样填」= tip → 收进角标；「当前是什么状态」= 读数 → **必须常驻**（`SettingsField` 的 `state`，或 `.ufo-card__hint` 那类徽标），把读数藏进 hover 等于没写。
- ⚠️ 角标是标题的**兄弟节点**（两者一起包在 `.ufo-heading-row` 里），不能塞进标题元素里面：设置页普遍用 `aria-labelledby` 指向标题，标题的无障碍名称会把按钮的名字「说明」一起读进去。
- ⚠️ 主窗里任何浮层（角标面板、下拉、右键菜单）都必须 `Teleport` 到 body：从 `.main-shell` 到 `.main-window--wide` 多层 `overflow: hidden`，各分区面板又各自内滚，留在原地的绝对定位浮层一定被裁掉。定位复用纯函数 `placeContextMenu`（越界夹回视口），z-index 阶梯 = select 40 / modal 50 / 右键菜单 60 / **说明角标 70**。

---

## 8. 信息卡（tooltip）契约

### 8.1 分工

| 谁 | 负责 |
|---|---|
| 组件 | 悬停时 `reportHover(key)`；声明 `tooltip.component` / `size` / `hasContent`；写内容组件、订阅自己的数据 |
| 宿主（悬浮窗） | 给悬停目标补上 `moduleId` 再转给外壳（组件不需要知道自己的 id） |
| 外壳 | 250ms 延迟、跨窗口 `emitTo('tooltip', 'tooltip-hover', payload)`、定位（`placeTooltip`，未贴边一侧优先、越界换侧）、显隐、点击穿透、尺寸 `setSize` |
| 提示窗容器 | 毛玻璃外壳、**按 `moduleId` 从注册表分派内容**（`resolveTooltipDefinition`，`src/lib/tooltip.ts`）、滚动与省略号 |

- 提示窗只当容器，**不认识任何组件语义**：组件没声明 `tooltip`、或它的 `hasContent` 说数据还没到，就不弹。加一个新信息卡**不用改提示窗**，只在组件定义里写。
- payload 里的 `target` = `{ moduleId, key }`，另外只带**悬浮窗外框矩形 + 贴边状态**（定位要用）。⚠️ 不带数据本体：数据由内容组件自己订阅，否则跨窗口缓存会让信息卡显示陈旧值。

### 8.2 内容组件规范

```vue
<script setup lang="ts">
// 内容根节点不接收透传属性：提示窗只传 target，避免属性落到 fragment 根上报警告
defineOptions({ inheritAttrs: false })

const props = defineProps<{ target: FloatingHoverTarget }>()
// ⚠️ 数据必须自己订阅：payload 只传「哪个对象」，不传数据本体，避免主窗口切配置后显示旧值
const store = useClockStore()
</script>

<template>
  <div class="clock-tooltip__head">…</div>
  <TooltipRow label="时区" :value="…" />
</template>
```

- ⚠️ **多根节点（fragment）是约定，不是巧合**：`.tooltip-shell` 本身就是行级 flex 容器，外面再套一层 `<div>` 会让行距失效、`mt-auto`（把提问压到底部）也失效。所以内容组件直接平铺若干根节点。
- 用 `TooltipRow` / `TooltipSection`（`src/components/floating/tooltip/`）保证行距与分组一致，别自造 class。行的契约是「左标签 + 右值」：`mono` 给等宽值（endpoint / id）、`tone` 给语义色、`title` 放超长值的完整原文、默认插槽放富内容；分组（`TooltipSection`）只多一条顶部强分隔线。
- 内容用 flex + 省略号自适应，不要假设自己能换行、能滚动。
- **不要**在内容组件里做定位 / 显隐 / `setSize`。
- ⚠️ 关于 `title` 的两条规则别混：悬浮窗上的**指示灯 / 组件元素一律不加 `title`**（会触发 Windows 原生文字提示，与我们自己的信息卡重复且更简陋，见 5.2）；而 `TooltipRow` 的 `title` 属性**是给信息卡内部用的**——它把超长值的完整原文挂在值上，供读屏与查看，不产生第二层弹窗语义。无障碍名称继续走 `aria-label`。

### 8.3 尺寸与预算（硬约束）

`size` 是**物理像素**，省略 = 用 `tauri.conf.json` 的默认 300×260；高度是有预算的：

```text
head + Endpoint + token + 4 个拓展字段 + 刷新标注 + 提问 1 行 ≈ 236px（窗口高 260px）
```

- 加字段数 / 加行数必须同步核算窗口高度，超出会被裁掉。
- **`size` 也可以给函数** `(target) => { width, height }`：内容行数随悬停目标变化时（例如折叠清单里有几个会话），窗口要跟着长高才装得下全部行。外壳只负责取值（`resolveTooltipSize`）并把结果**夹进工作区**（`clampTooltipSize`），不认识它是按什么算的 —— 组件不许自己调 `setSize`。
- 提示窗在 `present()` 里**先 `setSize` 再量尺寸定位**：反过来的话定位算的是旧窗口的几何，第一次弹出总会偏。
- ⚠️ 信息卡**不参与格子缩放**：`--floating-cell-scale` 只属于悬浮窗槽位（见 5.5），提示窗是另一个固定尺寸窗口，里面的字号写设计像素即可。
- 参考实现的字段顺序（`AgentStatusTooltip.vue`，属于该组件自己的账，不是提示窗契约）：provider/phase → Endpoint → token 用量 → 拓展字段 + 刷新标注 → 提问。拓展字段紧跟 token 行、提问永远压在最底（插到别处会和「这几行属用量类」的分组错位），并且**不展示 session id**。
- `hasContent` 用来避免「先弹一个空框、再填内容」：提示窗是独立 webview，事件异步到达，数据晚一步就永远弹不出来（`hasContent` 在内容组件挂载**之前**求值）。所以**数据订阅必须写在 `activate` 里**（第 9 节），不能等组件 `onMounted`。
- 提示窗是独立窗口，**不适用**方形 / 横条 / 竖条三形态适配，尺寸由 `size` 与 `tauri.conf.json` 决定。

### 8.4 悬停与关闭

- 拖拽 / 点击 / 贴边重算时必须 `dismissTooltip()`：`startDragging()` 之后指针交给系统，`pointerleave` 可能不再触发。
- 悬停目标消失（数据被清）时，`hasContent(target)` 返回 false → 不弹 / 自动收起。

---

## 9. 生命周期 `activate`

```ts
activate: () => {
  const store = useClockStore()
  void store.start()          // 订阅跨窗口数据 / 起轮询
  return () => store.stop()   // 返回清理函数（可选）
}
```

规则：

1. **外壳在每个窗口挂载时调用一次**（悬浮窗 / 主窗 / 提示窗各一份前端实例，因此各调一次）。
2. **必须能重复调用**：多个窗口各自 `start()`，store 内部要有 `started` 标志防重入（见 `useFloatingModulesStore.start()`）。
3. 返回值可选；返回了就登记，窗口卸载时**逆序**调用。
4. 单个组件抛错只 warn，不影响其它组件与窗口 —— 但别指望这层保护，自己 try/catch 更清楚。
5. 需要按窗口区分行为时，组件自己读 `getCurrentWindow().label`（`floating` / `main` / `tooltip`），**不要**让外壳替组件判断。
### 9.1 三窗口都会激活：数据源只能有一个 owner

- `activate` 会被**三个窗口各调一次**（各自独立的前端实例与 Pinia），别假设「只调用一次」。
- 同一份外部数据（轮询 / 定时请求 / 外部事件订阅）**只允许一个窗口当 owner**。UFO 里选 `floating`：它常驻、总是存在；`main` 与 `tooltip` 可能一直隐藏。
- owner 判定用 `getCurrentWindow().label`，**不要**用「谁先启动谁负责」或 `setTimeout` 猜窗口。
- owner 必须同时满足「组件已启用」+「我是 owner 窗口」；组件被禁用后**必须停掉定时器**，其它窗口不得偷偷接管轮询。
- `activate` 可能在配置异步加载完成**之前**被调用：需要配置才能开跑的组件，先 `await useFloatingModulesStore().start()`（或订阅配置变化）再启动请求，别拿 `fallbackConfig` 直接打网。外壳已经在 `App.vue` 里为**三个窗口各起一次**这份窗口级 store（见 2.1，幂等），组件的 `await start()` 只是自保；真正要记住的是反例 —— 若外壳漏起某个窗口的配置 store，那个窗口会永远停在 `fallbackConfig` 的默认值上（历史坑：提示窗不读配置 → 主窗改了 token 单位也不跟随）。
- `start()` / `stop()` 必须幂等：重复调用不得产生第二个监听器、第二个定时器或第二条广播链路。

---

## 10. 样式规范

### 10.1 组件样式放组件目录

```css
/* src/modules/<id>/<id>.css 的第一行必须是这句 */
@reference "../../assets/main.css";
```

⚠️ Tailwind v4 只给「入口 CSS」注入主题与工具类；组件把样式拆成独立文件后必须显式引用入口，否则文件里的 `@apply` 全部报错。`@reference` 只提供编译上下文，不会重复输出入口样式。

引入方式：在 `src/modules/<id>/index.ts` 里 `import './<id>.css'`（随注册表加载，未注册的组件样式不会进包）。

### 10.2 颜色只走 token

一律使用 `src/assets/main.css` 的 `--ufo-*` token，**禁止**再散落硬编码灰色：

| 用途 | token | 值 |
|---|---|---|
| 悬浮窗背景（毛玻璃） | `--ufo-bg` | `rgba(61,61,61,0.8)` |
| 主窗背景（实色，无 blur） | `--ufo-bg-main` | `#3D3D3D` |
| 卡片表面 / 次级表面 | `--ufo-surface` / `--ufo-surface-2` | `#2A2A2A` / `#323232` |
| 输入底 | `--ufo-input` | `#1F1F1F` |
| 边框 / 强边框 | `--ufo-border` / `--ufo-border-strong` | `rgba(255,255,255,0.08)` / `0.14` |
| 主文本 / 次文本 | `--ufo-text` / `--ufo-muted` | `#F3F4F6` / `#A3A3A3` |
| 强调 / 强调浅底 / 强调上文字 | `--ufo-accent` / `--ufo-accent-soft` / `--ufo-accent-on` | `#34D399` / `rgba(52,211,153,0.16)` / `#052E1D` |
| 危险 | `--ufo-danger` | `#F87171` |
| 状态灯 | `--ufo-status-green` / `-yellow` / `-red` | `#34D399` / `#FBBF24` / `#F87171` |
| 圆角 / 焦点环 | `--ufo-radius` / `--ufo-ring` | `12px` / `rgba(52,211,153,0.55)` |

### 10.3 风格约定

- 悬浮窗：`transparent: true` + 半透明 `--ufo-bg` + backdrop blur（毛玻璃，模糊度 0.8，浅灰底）。
- 主窗：外壳与 html/body/#app 不滚，只有 `.main-window` 滚；滚动条用 Matte 细滚动条 + `scrollbar-gutter: stable`。
- 字体：`--font-sans`（Plus Jakarta Sans）。
- 命名：块用 `.<id>-module` / `.<id>-tooltip`，元素用 `__`，修饰用 `--`，状态类用 `is-*`（如 `is-flashing`）。
- 动画要能被 `prefers-reduced-motion` 关掉。

### 10.4 样式归属边界（别把外壳样式搬进组件）

以下类名属于**外壳**，继续留在 `src/assets/main.css`，组件不得重定义：

```text
.main-*          主窗口外壳（标题栏 / 卡片 / 导航 / 滚动条）
.floating-*      悬浮窗外壳（外壳 / 宿主 / 槽位 / 组件列表 / 形态预览）
.ufo-*           全局 token 与通用控件
.module-catalog / .shape-preview     组件列表与三形态预览
.tooltip-shell / .tooltip-row / .tooltip-section*   提示窗容器与通用行
```

判断方法：这个类的样式，删掉某个组件后还需要吗？需要 → 外壳；不需要 → 组件自己的 `<id>.css`。

### 10.5 单位陷阱
Tauri 的窗口尺寸是**物理像素**，CSS 是逻辑像素。高分屏（scale factor 2）下 `setSize(45)` 的窗口在 CSS 里是 22.5px。因此：

- 组件布局不要跟窗口尺寸「对数字」，只按比例自适应。
- 需要精确对齐时，用 `getCurrentWindow().scaleFactor()` 换算，别凭经验乘常数。
- 槽位内部的比例一律走 `clamp(下限, Xcqh, 上限)`（见 5.5），不要按「一格 45px」反推固定像素。

---

## 11. 配置持久化与迁移

### 11.1 文件与结构

位置：`<app-data>/floating-modules.json`（Windows：`%APPDATA%\<identifier>\floating-modules.json`）。

```jsonc
{
  "schema_version": 1,                 // 外壳线协议版本；**不得随手升**
  "revision": 7,                       // 单调递增，用于丢弃迟到 / 乱序的旧配置
  "modules": [{ "id": "agent-status", "enabled": true, "order": 0 }],
  "cell_px": 45,                       // 一格多少像素（32–96）：外壳唯一保留的窗口级尺寸参数
  "module_settings": {                 // ★ 组件的家：键 = 组件 id，值 = 组件自己解释的对象
    "agent-status": { "max_cells": 9, "completed_retention_ms": 30000, "token_unit": "tokens" }
  }
}
```

| 字段 | 归属 | 说明 |
|---|---|---|
| `modules[].enabled` / `.order` | 外壳 | 仅这两项与具体组件无关，故留外壳 |
| `cell_px` | 外壳 | 一格边长（像素，32–96，默认 45）：外壳**唯一**保留的窗口级尺寸基准。`#[serde(default)]` —— 旧配置缺字段按默认值走，**不得**为它升 `schema_version`。前端读它一律走 `configCellPx()` / `cellSizeOf()`，别直接取字段 |
| `module_settings[<id>]` | **组件** | 外壳只按 id 原样存取，**不解释**任何字段。格子上限也在这里（如 Agent 的 `max_cells`，滑块 `{1,2,3,4,6,9}` 封顶 3×3）—— 它只限该组件自己的大小，整窗仍按各组件需求之和自动伸缩 |

### 11.2 写入链路（Rust）

```text
前端 updateModuleSettings(id, patch)
  → invoke('set_floating_module_config')          [仅 main 窗口有权限]
  → revision = 当前 revision + 1
  → validate()：校验 schema_version / 模块 id（≤64 字符、不含 / 与 \）/ clamp 格子上限与一格边长（cell_px）
  → persist()：先写 floating-modules.tmp 再 rename（原子，不留半截文件）
  → emit('floating-module-config')                 [三窗口都收，前端 applyConfig]
  → notify()：按登记顺序调用各组件的 config_hook  [组件据此刷新运行时状态，改完即时生效]
```

收到 `floating-module-config` 后，悬浮窗侧的一格边长不是写死的：`FloatingWindow.vue` 的 `baseSize` 取自 store（`configCellPx()`），配置事件到达就 `scheduleSnap()` 按新尺寸重算贴边参数 —— **正在贴边时也会就地更新**，不需要用户把窗口拖下来再吸回去。组件侧同理：`--floating-cell-scale` 是宿主按 `cell_px` 现算的，配置一变、缩放跟着变。

### 11.3 读盘与旧配置迁移

```text
ModuleState::load(path, &[组件认领钩子])
  → FloatingModuleConfigWire：用 #[serde(flatten)] 收下顶层所有「外壳不认识的字段」
  → 逐个调用组件认领钩子：migration(&mut module_settings, &legacy) -> bool
  → 只要有组件认领 → 把清理后的配置写回一次（**不 bump revision、不广播**：对使用者而言内容没变）
```

组件认领钩子（Rust）示例：

```rust
/// 旧版外壳把本组件字段平铺在顶层时的键名 → 本组件配置块里的键名。
const LEGACY_FLAT_FIELDS: &[(&str, &str)] = &[
    ("completed_retention_ms", "completed_retention_ms"),
    ("token_unit", "token_unit"),
];

pub fn adopt_legacy_fields(settings: &mut ModuleSettingsMap, legacy: &BTreeMap<String, Value>) -> bool { … }
```

登记位置（组合根）：`ModuleState::load(module_config_path, &[adopt_legacy_fields])`、`module_state.on_config_change(config_hook())`。

**迁移的三条硬规则**：

1. **已有新配置块时不覆盖用户当前值** —— 旧字段只被「认领」（好让外壳把它从文件里清掉），不参与取值。
2. **只认领自己认识的键**，别把别的组件遗留的字段顺手带走（有单测锁这条）。
3. **迁移写回不 bump revision、不广播**：迁移不是用户操作，广播会触发无意义的刷新。

### 11.4 加 / 改 / 删配置项的规矩

| 动作 | 要做的事 | 不要做的事 |
|---|---|---|
| 加字段 | 组件字段：前端 `settings.ts` + Rust `settings.rs` 各加一处 + 默认值，组件自己 clamp；**外壳字段**（整个窗口的，如 `cell_px`）：只改 `floating-modules.ts` / `module_config.rs`，加 `#[serde(default)]` + clamp | 不改 `schema_version`；组件字段不得写进外壳 |
| 改字段语义 | 同时改两份实现与两份单测，写清上限 / 下限 | 不要只改一边（会出现「显示与实际不一致」） |
| 删字段 | 保留读取兼容（读到就忽略），停止写入 | 不要在 `from_block` 里直接 panic / 报错 |
| 改名 | 旧名写进 `LEGACY_FLAT_FIELDS` 迁移表 | 不要指望用户手改配置文件 |
| 加枚举值 | 前端解析处给出兜底分支（未知值 → 默认） | 不要让未知值直通运行时 |

### 11.5 线协议版本（重要）

- 配置的 `schema_version` 与各类快照的 `SCHEMA_VERSION` 是**线协议**：新增字段一律 `Option<T>` + `#[serde(default)]`，**永不递增**，否则旧 hook / 旧文件会被静默丢弃。
- 只有在「不兼容重写」时才升，并且必须同时提供迁移与回滚说明。
- 需要库内布局版本时用另一个机制（如 SQLite 的 `PRAGMA user_version`），别混用。

---

## 12. Rust 侧规范

### 12.1 组件目录

```rust
// src-tauri/src/modules/mod.rs
//! 悬浮窗组件在 Rust 侧的家。一个组件一个目录，目录名 = 前端模块 id 的 Rust 写法。
pub mod agent_status;
pub mod clock;
```

```rust
// src-tauri/src/modules/<id>/mod.rs —— 只写模块声明与文档注释，不写逻辑
//! 时钟组件（Rust 侧）。与前端 `src/modules/clock/` 一一对应。
pub mod settings;
```

- 目录名 = 前端 id 的下划线写法（`agent-status` → `agent_status`）。
- 组件之间的引用必须走 `crate::modules::<other>` 吗？**不要**：组件互不认识，共用逻辑下沉到 `crate::floating`（外壳通用能力）或各自实现。
- 从组件内部引用自己用 `crate::modules::<self>::…`（`lib.rs` 是唯一允许点名的地方）。跨目录引用一律写**全路径**，⚠️ **不要**在 crate 根（`lib.rs`）做 `use` 别名转发 —— 别名会把结构藏起来，读代码的人看不出这个能力属于哪个组件，也看不出它其实越过了外壳边界。

### 12.2 组件配置解释器（`settings.rs`）

固定四件套：常量、结构体、解析、钩子。

```rust
pub const MODULE_ID: &str = "clock";
const DEFAULT_TICK_MS: u64 = 1_000;
const MIN_TICK_MS: u64 = 200;
const MAX_TICK_MS: u64 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockSettings { pub tick_ms: u64 }

impl ClockSettings {
    /// 从整份外壳配置里取出本组件的块（缺块 = 全默认）。
    pub fn from_config(config: &FloatingModuleConfig) -> Self { Self::from_block(config.module_settings.get(MODULE_ID)) }
    /// 逐字段宽松取值 + clamp：某个字段写错只影响它自己。
    pub fn from_block(block: Option<&Value>) -> Self { … }
}

/// 配置落盘后同步到运行时（组合根登记）：改完即时生效，不用重启。
pub fn config_hook() -> ModuleConfigHook { Box::new(|app, config| { … }) }
```

对齐要求：字段名、默认值、上下限必须与前端 `settings.ts` **逐字一致**，并用同构的单测锁住。

### 12.3 运行时状态与线程

- 需要跨线程共享的状态：`Arc` + `Mutex`，通过 `app.manage(...)` 注册；线程里用 clone 的句柄。
- 长任务（网络 / 解析 / 数据库）放专用线程**串行**执行（如 status monitor、flow worker），别在命令里干重活。
- `async` 命令里做阻塞操作必须 `spawn_blocking`：非 async 命令跑在主线程，一次最坏 15s 会冻结整个窗口事件循环。
- 带引用入参的 `async` 命令按 tauri 宏要求必须返回 `Result`。

### 12.4 panic 与保密（两条不可谈判的红线）

- **release 是 `panic = "abort"`**：组件代码里**禁止** `unwrap` / `expect` / 直接索引 / `slice` / `panic!`。失败一律变 `Err(String)` 或跳过本轮，否则一次 panic 带走整个 App。
- **不读、不缓存、不上报凭据**：token / API key 一律不读不打印；需要凭据时走 OS 凭据管理器（keyring），值不落盘、不进档案、不进日志。

### 12.5 命令注册三处（漏一处就是「命令不存在」）

| 位置 | 内容 |
|---|---|
| 组件实现 | `#[tauri::command] pub fn …` |
| `src-tauri/permissions/status.toml` | `[[permission]]` + `identifier` + `commands.allow` |
| `src-tauri/capabilities/*.json` | 把 identifier 加进对应用户窗口的 `permissions` 数组 |
| `src-tauri/src/lib.rs` | `generate_handler![]` 里挂上 |

**权限拆分策略**：

- **只读命令** → `capabilities/default.json`（`windows: ["main","floating","tooltip"]`）：悬浮窗与提示窗都要读数据。
- **写命令**（改配置、删记录、存脚本）→ `capabilities/main-module-settings.json`（`windows: ["main"]`）：只有主窗口有写权限。
- 新增窗口时**必须**把 label 加进某份 capability 的 `windows` 数组，否则该窗口**完全没有 IPC 权限**（连 `listen` 都收不到；细节见 13.1）。

### 12.6 新增源 / 平台

组件内部若有多数据源（hook、rollout、轮询…），让它们**汇进同一个入口函数**再进内核，别各写一条链路绕开。新增平台 = 新增一个适配器实现 + 注册表加一个分支，其它代码零改动。

---

## 13. 跨窗口通信与 capability

### 13.1 三个窗口

| label | 用途 | 关键配置 |
|---|---|---|
| `floating` | 悬浮窗 | 初始 45×45（= 默认一格；读到配置后按 `cell_px` 重算）、transparent、alwaysOnTop、不可缩放 |
| `main` | 设置主窗 | 1000×750、`visible: false`、自绘标题栏、关闭 = 隐藏到托盘 |
| `tooltip` | 悬停信息卡 | 300×260、`visible: false`、`focusable: false`、点击穿透 |

窗口都在 `src-tauri/tauri.conf.json` 声明；`src/App.vue` 按 label 分派组件。**没有运行时建窗**：

- `tooltip` 走「**声明 + 隐藏 + 按需显示**」，因此**不需要** `core:webview:allow-create-webview-window` 这类建窗权限。加新窗口照着做（在 conf 里声明成 `visible: false`，用时 `show()`），不要改成运行时创建 —— 那会连带引入建窗权限与窗口生命周期管理。
- 点击穿透在 Rust 侧 `setup` 里设（`window.set_ignore_cursor_events(true)`）。⚠️ **Rust 调用不经过 capability 校验**，所以穿透不需要额外权限；但同一件事若改用前端 `invoke` 就要授权，别绕。
- ⚠️ **新增窗口必须把 label 加进 `src-tauri/capabilities/default.json` 的 `windows` 数组**，否则该窗口**完全没有 IPC 权限**（连 `listen` 事件都不行，表现为「窗口开了但什么都收不到」）。写命令另放 `main-module-settings.json`（见 12.5）。
- 跨窗口控制别的窗口（悬浮窗指挥 `tooltip`）是允许的：ACL 校验的是**调用方**的权限，目标 label 只是参数。

### 13.2 事件契约

- 事件名一律 `ufo-` 前缀（避免与 Tauri 内置事件撞名），常量集中在 `src/lib/floating-bridge.ts` 或组件自己的 `lib/`，**不要**在组件里写裸字符串。
- 载荷类型必须导出 interface（如 `OpenModuleSettingsPayload`）。
- 现有窗口级事件：`ufo-floating-runtime`、`ufo-floating-runtime-request`、`ufo-open-module-settings`、`tooltip-hover`、`tooltip-interaction`、`floating-module-config`。
- 跨窗口 `emitTo(label, event, payload)`：ACL 校验的是**调用方**窗口的权限，目标 label 只是参数。
- 监听方在挂载时 `listen(...)`，卸载时调用返回的 `unlisten`（用 `await` 拿到的句柄别丢）。
- 主窗口「可能错过广播」时用**请求补发**模式：主窗 `emitTo('floating', REQUEST)`，悬浮窗收到后重播一次（见 `startRuntimeBridge`）。
- **订阅顺序固定为「先 listen，再取数」**：先 `listen(...)` 再 `invoke('get_...')` / 请求快照。反过来会丢掉两件事之间到达的广播，窗口永久停在空状态。
- **payload 只传展示所需的最小数据**，不传 store、DOM、全量配置或密钥；接收端要校验形状（`isXxxPayload()` 这类守卫），不要把 `any` 直接灌进 store。
- 组件自己的事件名以组件 id 作命名空间（如 `clock-update`），外壳不解释其字段。

### 13.3 组件要开的通道，走外壳的现成能力

打开某组件的设置页有两条路，**走的是同一个事件**：① 「悬浮窗设置」页组件列表行的齿轮；② 点悬浮窗上该组件的区域 —— 外壳读槽位根节点的 `data-module-id`（`moduleIdAt()`）翻译成 `moduleId`，再 `emitTo('main', OPEN_MODULE_SETTINGS_EVENT, { moduleId })`；点空白处传 `null` = 切到「悬浮窗设置」页。两条路都落在该组件的**第一个分区**（见 7.1）。

- 悬停：组件只调 `reportHover(key)`，跨窗口转发由外壳负责。
- 组件**不要**自己 `emitTo` 主窗口、也不要自己创建窗口；要新能力先扩外壳契约。

⚠️ 外壳这条链上有三个坑，扩展它时一起保住：

1. 顺序必须是**先 `showMainWindow` 再 `emitTo`**：主窗还没打开时没有任何监听者，事件直接丢失（表现为「点了一下没反应，再点一下才跳到上次的页面」）。
2. `pointercancel`（触摸被系统取消）**只清状态，不当作点击**：把它算成点击会莫名弹出主窗。
3. 拖拽 / 点击 / 贴边重算时都要 `dismissTooltip()`（见 8.4）—— `startDragging()` 之后指针交给系统，`pointerleave` 可能永远不会来。

---

## 14. 测试规范

### 14.1 位置与命名

| 类型 | 位置 | 命名 |
|---|---|---|
| 前端单测 | 与实现同目录 | `<name>.spec.ts`（vitest 的 include 是 `src/**/*.spec.ts` + `tests/unit/**/*.test.ts`） |
| Rust 单测 | 同文件底部 | `#[cfg(test)] mod tests { … }` |

**外壳侧已有的护栏**（改外壳或加组件都要跑，它们只用假组件，不依赖任何真实模块）：

| 文件 | 锁住什么 |
|---|---|
| `src/components/floating/floating-module-host.spec.ts` | 同级挂载 + 需求汇总分槽 + 悬停转发（补 `moduleId`） |
| `src/lib/floating-modules.spec.ts` | 需求汇总 / `resolveWindowCells` / `runtimeSnapshot` / `activateModules`（逆序清理、单组件抛错隔离） |
| `src/lib/floating-layout.spec.ts` | 形态判定与窗口尺寸口径（含四角逐模块堆叠、阶梯归档） |
| `src/lib/tooltip.spec.ts` | 信息卡按 `moduleId` 分派、`resolveTooltipSize` / `clampTooltipSize` / `placeTooltip` |
| `src/components/floating/floating-modules-panel.spec.ts`、`floating-shape-preview.spec.ts` | 「悬浮窗设置」页与三形态预览（预览必须与真窗口同口径） |
| `src/components/floating/floating-module-scaling.spec.ts` | 所有模块样式的 `--floating-cell-scale`（见 5.5） |

组件自己的集成验证放 `src/modules/<id>/*.spec.ts`（含样式护栏 `<id>-css.spec.ts`）。

### 14.2 组件必须覆盖的最小集

1. **配置解析**：缺字段回落默认值；越界值被 clamp；类型写错只影响该字段；未知枚举值回落默认。
2. **迁移**：旧扁平字段被认领进自己的块；已有新块时**不被覆盖**；没有旧字段时是 no-op；端到端跑一次真外壳配置（含写回后顶层旧字段消失）。
3. **`demand`**：四种形态的返回值；非有限值被归一。
4. **视图**：形态切换下的渲染数量；被挤小时出现溢出格 `+N`；无 `provide` 上下文时可单独渲染。
5. **生命周期**：`activate` 的清理函数在卸载时被调用（外壳侧有顺序 / 逆序 / 单模块抛错隔离的用例）。
6. **跨窗口契约**：事件名与载荷结构（防止改名漏改调用方）。

### 14.3 写法要点

- Pinia：用 `@pinia/testing` 的 `createTestingPinia`，别让单测真的去 `invoke`。
- 注入替身：`mount(Comp, { global: { provide: { [floatingModuleKey]: { allocatedCells: computed(() => 4), reportHover: vi.fn() } } } })`。
- Tauri API 一律 mock（`@tauri-apps/api/core` 的 `invoke`、`/event` 的 `emitTo` / `listen`）——单测不启动真窗口。
- 不要为了测试新增依赖；`@vue/test-utils` + `vitest` + `@pinia/testing` 已经够用。
- 测试名用一句话说清断言（中文亦可），别写 `it('works')`。

### 14.4 命令

```powershell
npx vitest run                     # 前端单测（改前端必跑）
npm run build                      # vue-tsc 类型闸门 + 打包（改前端必跑）
cd src-tauri; cargo test           # Rust 单测（改 Rust 必跑）
npm run check                      # cargo check（快速编译闸门）
```

⚠️ 这些测试**不覆盖**真实 Tauri 窗口行为（贴边、多窗口、点击穿透、真机 blur），也不覆盖外部依赖（cc-switch 数据库、凭据管理器、真实 rollout 文件）。这些只能真机回归，别把「单测绿了」当成「功能好了」。

---

## 15. 验收清单

### 15.1 自动化

- [ ] `npx vitest run` 全绿
- [ ] `npm run build`（含 vue-tsc）通过
- [ ] `cd src-tauri; cargo test` 全绿、`cargo check` 无 warning
- [ ] 第 1.3 节三条结构 grep 零命中

### 15.2 三形态人工核对（必做）

在真机上把悬浮窗依次摆成：未贴边（方形）/ 贴上下边（横条）/ 贴左右边（竖条）/ 贴四角（各模块竖向堆叠成多行区块），逐个确认：

- [ ] 内容不被裁切、不溢出窗口
- [ ] 文字省略号正常，没有把相邻元素挤变形
- [ ] 空间不够时出现溢出提示（如 `+N`），而不是硬挤
- [ ] 多个组件同时启用时，各自按比例分到空间，互不重叠
- [ ] 形态切换过程中没有闪烁 / 抖动（需求函数是纯函数，不应随分配变化）

### 15.3 交互人工核对

- [ ] 组件列表的齿轮 → 打开主窗，落在该组件**第一个分区**
- [ ] 点击悬浮窗里该组件的区域 → 同上（靠 `data-module-id` 翻译）
- [ ] 只有 1 个分区的组件**没有**二级 tabbar
- [ ] 悬停出信息卡、移开即收；贴边 / 拖拽 / 点击时信息卡立刻消失
- [ ] 信息卡不会弹空框（`hasContent` 生效）
- [ ] 主窗口改配置后**即时生效**（不用重启）
- [ ] 「悬浮窗设置」的格子大小滑块：拖动时预览跟着变，松手后真窗口按新尺寸重算（贴边中也一样）；把值调回来点「默认」能回到 45
- [ ] 设置页的解释性 tips 只在问号角标上：悬浮 / 聚焦展开、面板外按下与滚动即收；**状态读数仍然常驻**（没被塞进角标）

### 15.4 真机回归（只有人能做）

- [ ] 旧配置文件能自动迁移，迁移后旧字段从文件里消失、设置值不丢
- [ ] 旧配置（没有 `cell_px`）打开后窗口仍是 45，不塌成一条线
- [ ] 重启 App 后设置仍在
- [ ] 高分屏（scale factor ≠ 1）下尺寸与命中区域正常

---

## 16. 新增组件 checklist

按顺序做，每一步都能单独验证：

1. **定 id 与形态**：id 用小写短横线（如 `clock`）；确定 `supportedLayouts`（拿不准就四个全写上，先把布局做自适应）。
2. **建前端目录** `src/modules/<id>/`，先写 `index.ts`（定义）+ `<Name>Module.vue`（视图）+ `<id>.css`（先写 `@reference "../../assets/main.css";`）。
3. **写视图**：接 `layoutMode` prop、`inject(floatingModuleKey, null)`、`reportHover`；用 grid + `minmax(0, 1fr)` 做自适应。
4. **写 `demand.ts`**（若不止 1 格）：纯函数，只依赖形态与自己的配置。
5. **写 `settings.ts`**（若有配置项）：id 常量、类型、默认值、范围、解析 + clamp、按需的 `fetch*`。
6. **写设置页分区** `sections/<Name>Section.vue`：短表单一页；大面板设 `fill: true` 并自己处理内部滚动 + `scrollbar-gutter: stable`。
7. **写信息卡**（若需要）：`<Name>Tooltip.vue`，多根节点、`inheritAttrs: false`、只用 `TooltipRow` / `TooltipSection`、按预算定 `size`、给 `hasContent`。
8. **写 `activate`**：store `start()` / `stop()`，返回清理函数；组件数据订阅必须在这里。
9. **注册**：`src/modules/index.ts` 加一条。
10. **Rust 侧**（若需要后端）：`src-tauri/src/modules/<id>/`（`mod.rs` + `settings.rs` + 逻辑），`modules/mod.rs` 加 `pub mod <id>;`，在 `lib.rs` 登记 `ModuleState::load(..., &[<id>::settings::adopt_legacy_fields])`、`on_config_change(<id>::settings::config_hook())`、`app.manage(...)`、`generate_handler![]`。
11. **命令权限**：`permissions/status.toml` 加 `[[permission]]`；只读进 `capabilities/default.json`，写命令进 `capabilities/main-module-settings.json`。
12. **测试**：按第 14.2 节最小集补 `.spec.ts` 与 Rust `mod tests`。
13. **跑验收**：第 15 节全部过一遍。
14. **补文档**：架构与契约的改动写进**本文**；组件自己的实现细节写进对应 guide（同级直接写文件名，如 `agent-status-log-guide.md`）；平台 / 灯色口径写进 rule 目录，跨目录引用一律 `../rule/agent-status-hooks.md` 这样带相对路径。**AGENTS.md 只留 ≤100 行的稳定规则**，只在稳定规则真的变化时补一行指针（「某主题见本文」），不要再抄一遍细则；`CLAUDE.md` 现在只是一行 `@AGENTS.md` 导入，**不存在双写同步**，也不要对它们跑 prettier。

---

## 17. 常见错误与反例

| 做法 | 后果 |
|---|---|
| 外壳里写 `if (id === 'agent-status')` | 第二个组件出现时外壳变成 if 地狱；结构验收 grep 破线 |
| 外壳注释举例写真实组件名 / 字段名 | 一样是泄漏（grep 只查代码字符串查不到注释），并且诱导下一个人去点名组件；见 1.2 |
| 新组件 / 新窗口没进 `capabilities/*.json` 的 `windows` 数组 | 该窗口**零 IPC 权限**：能显示但收不到任何事件，排查半天像广播丢了（见 13.1） |
| 运行时 `WebviewWindow.create` 建窗 | 白白引入 `core:webview:allow-create-webview-window` 与窗口生命周期管理；本项目一律「声明 + 隐藏 + 按需显示」（见 13.1） |
| 组件 `demand` 返回 0 想「这个形态不占地方」 | 0 照样按 1 格算，窗口还是那么大；该用开关或 `supportedLayouts`（见 6.2） |
| 组件在槽位里再加 `padding` | 外壳已经只留 1px 描边 + `p-0.5`，槽位 `overflow: hidden`，多出来的内边距直接把内容挤被裁（见 5.5） |
| 信息卡内容外面套一层 `<div>` | `.tooltip-shell` 的行级 flex 与 `mt-auto` 失效，间距散掉、提问不再贴底（见 8.2） |
| 预览 / 真窗口各算一套尺寸 | 明明没改配置，预览和贴边结果对不上；两者必须共读 `runtimeSnapshot`（见 6.1） |
| 组件直接改 `modules.config.module_settings` | 绕过 revision 与落盘，刷新即丢 |
| 组件把配置字段塞进外壳 store 的 state | 外壳被迫认识组件字段，契约失效 |
| `demand` 里读 `allocatedCells` | 尺寸回路：抖动 / 死循环 |
| 组件自己 `emitTo('main', ...)` 开设置 | 绕过外壳的「点击打开设置」通道，权限与行为不一致 |
| 订阅写在 `onMounted` | tooltip 弹不出来（`hasContent` 先于挂载求值） |
| `activate` 不可重入 | 三窗口各调一次，重复订阅导致数据翻倍 |
| 布局写死 `width: 56px` | 贴边变形态后内容被裁 |
| 给元素加 `title` | 触发 Windows 原生提示，与 tooltip 重复 |
| 用 `window.alert` / `confirm` | 被 `tauri-plugin-prevent-default` 拦掉，按钮点了没反应 |
| 可滚容器没有 `scrollbar-gutter: stable` | 滚动条出现时内容横向跳动 |
| Rust 里 `unwrap()` / 索引 | release 下 panic = abort，一次崩掉整个 App |
| 顺手升 `SCHEMA_VERSION` | 旧 hook 事件 / 旧配置被静默丢弃 |
| 迁移时覆盖已有的新配置块 | 用户当前设置被旧文件顶掉 |
| 单测依赖真实 Tauri / 网络 / 凭据管理器 | CI 与本地互相打脸；改成 mock |

---

## 18. 附录：常量与不变量速查

| 名称 | 值 / 位置 | 说明 |
|---|---|---|
| 形态枚举 | `square` / `corner` / `horizontal` / `vertical` | `src/lib/floating-layout.ts`；对用户是三形态，四角单独一档 |
| 默认窗口格数 | `DEFAULT_WINDOW_CELLS = 4` | **只是宿主算出真实格子数之前的兜底**（2×2 容量）；汇总的地板是 `ABS_MIN_CELLS`，别拿它当最小值 |
| 格子数范围 | `ABS_MIN_CELLS = 1` / `ABS_MAX_CELLS = 16` | 绝对范围；单组件自己的上限更小（Agent 封顶 9），且在各组件的 `module_settings[<id>].max_cells` 里 |
| 方形 / 四角归档阶梯 | `CELL_STEPS = {1,2,3,4,6,9,12,16}` | 只有方形 / **单组件**的四角走阶梯归档；边条按个数精确取长度，不归档 |
| 需求汇总 / 分槽 | `moduleDemand` → `resolveWindowCells` → `moduleCellShares` / `allocatedCells` | 全在 `src/lib/floating-modules.ts`：纯函数 + 单测，宿主按权重用 grid `fr` 切槽 |
| 运行时快照 | `runtimeSnapshot(current, config, definitions, layoutMaxFor)` → `{ layout, byLayout }` | 一次算全四形态；悬浮窗广播、主窗徽标与三形态预览共读这一份 |
| 单格像素（默认） | `FLOATING_BASE_CELL_PX = 45` | ⚠️ 与 `tauri.conf.json` 里 floating 窗口的初值**必须同为 45**，改一边就对不上。可内容只有 ≈39.5px：外壳留 1px 描边 + `p-0.5`，宿主与组件不留 padding |
| 单格像素范围 | `MIN_CELL_PX = 32` / `MAX_CELL_PX = 96` | 用户在「悬浮窗设置」的滑块里调；`clampCellPx()` 与 Rust `validate()` 各 clamp 一次 |
| 单格像素换算 | `cellSizeOf(px)` / `configCellPx(config)` | 前者包成 `{ width, height }`，后者从配置取（缺字段回落默认）；⚠️ 别直接读 `config.cell_px` |
| 格子缩放变量 | `--floating-cell-px` / `--floating-cell-scale`（宿主在 `.floating-module-host` 上声明） | 组件的 px 设计值一律 `calc(N * var(--floating-cell-scale, 1))`；两类例外见 5.5 |
| 槽位容器 | `.floating-module-slot`（size 容器，`container-name: floating-slot`） | 组件用 `@container floating-slot (...)` 与 `cqh` / `cqw` 按槽位自适应（见 5.5）；槽位 `overflow: hidden` |
| 方形 / 四角 / 横条 / 竖条尺寸 | `floatingWindowSize(layout, demands)` / `windowGrid(layout, demands)` | 传**各模块需求数组**（非单个总数）：四角逐模块竖向堆叠成 N 行 × 最宽列数；主窗预览与贴边共用同一口径 |
| 自身行列换算 | `cellsGrid(layout, cellBudget)` | 组件算自己那块用这个，别「总数开平方」 |
| 悬停延迟 | `SHOW_DELAY_MS = 250`（`FloatingWindow.vue`） | 到点才发 `tooltip-hover`；拖拽 / 点击 / 贴边必须先 `dismissTooltip()` |
| 提示窗默认尺寸 | 300×260（`tauri.conf.json`） | 组件用 `tooltip.size` 覆盖（可给按目标算的函数）；外壳 `resolveTooltipSize` + `clampTooltipSize` 夹进工作区，`present()` 里先 `setSize` 再定位 |
| 提示窗内容分派 | `resolveTooltipDefinition(moduleId, …)`（`src/lib/tooltip.ts`） | 提示窗不认识组件语义；加信息卡不用改提示窗 |
| 事件名 | `ufo-floating-runtime` / `-request` / `ufo-open-module-settings` / `tooltip-hover` / `tooltip-interaction` / `floating-module-config` | `src/lib/floating-bridge.ts`、`src/lib/tooltip.ts`；窗口级事件走 `ufo-` 前缀（`tooltip-*` / `floating-module-config` 是历史命名，照用即可） |
| 窗口 label | `floating` / `main` / `tooltip` | 新窗口必须同时进 `src-tauri/capabilities/default.json` 的 `windows` 数组，否则**零 IPC 权限** |
| 权限分工 | 只读命令 → `default.json`（三窗）；写命令 → `main-module-settings.json`（仅 main） | 见 12.5 的「三处注册」 |
| 配置键 | `modules[].enabled|order`、`cell_px`、`module_settings[<id>]` | 外壳只解释这三类；其余字段全在各组件自己的块里 |
| 组件可用契约 | `FloatingModuleApi`（`allocatedCells` / `reportHover`）+ `@/lib/floating-*` + `useFloatingModulesStore` | 就这三处，见 1.2 |
| 注释要求 | 每个方法写功能；接口 / type 描述字段；关键代码写清在干什么 | 属稳定规则，落点在 `../rule/` 的代码规范那节；本文不重复 |
