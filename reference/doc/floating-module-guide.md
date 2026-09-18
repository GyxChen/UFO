# 悬浮窗组件开发规范

> 适用范围：`src/modules/<id>/`（前端）与 `src-tauri/src/modules/<id>/`（Rust）下的所有悬浮窗组件。
> 参考实现：`agent-status` —— 前端 `src/modules/agent-status/`，Rust `src-tauri/src/modules/agent_status/`。
> 与 AGENTS.md 的关系：AGENTS.md 写「结论与坑」，本文写「从零到验收的全过程」，冲突时以 AGENTS.md 为准（更靠近项目当前状态）。
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

1. **外壳不得认识任何具体组件。** 外壳文件里不允许出现组件 id（如 `agent-status`）、组件字段名（如 `token_unit`）、组件的 store / 命令。
2. **新增一个组件不得修改外壳。** 如果做新组件时需要改 `src/components/floating/**`、`src/lib/floating-modules.ts`、`src/stores/floatingModules.ts`、`src-tauri/src/floating/**`，正确的动作是**扩展外壳契约**（往 `FloatingModuleDefinition` 加一个通用字段），而不是加一个 `if (id === 'xxx')`。
3. **组件之间互不认识。** 组件不得 import 另一个组件的目录。共用逻辑要下沉到公共层（`src/lib/`、`src/components/ui/`、`src/components/floating/tooltip/`）或各自复制一小段。

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
│  ├─ floating/                   # 外壳：宿主 / 槽位 / 组件列表 / 形态预览 / 提示窗容器
│  ├─ ui/                         # 通用控件（BaseButton / BaseSelect / BaseTabs / ScriptEditor…）
│  ├─ MainWindow.vue              # 主窗外壳：一级 + 二级 tabbar
│  └─ TooltipWindow.vue           # 提示窗外壳：毛玻璃 + 定位 + 显隐
├─ lib/                           # 外壳公共层：floating-*、tooltip、snap、window、js-*
└─ assets/main.css                # ★ 只留全局 token 与外壳样式
```

**归属判断口诀**：把某个文件删掉，会不会有第二个组件受影响？

- 会 → 放公共层（`src/components/ui/`、`src/components/floating/tooltip/`、`src/lib/`）。
- 不会 → 放组件目录。

### 2.2 Rust

```text
src-tauri/src/
├─ lib.rs                         # ★ 组合根：唯一点名组件的地方
├─ floating/                      # 悬浮窗外壳
│  ├─ mod.rs
│  └─ module_config.rs            # 配置存取 / 迁移钩子 / 变更钩子
└─ modules/
   ├─ mod.rs                      # pub mod <id>; 一行一个组件
   └─ <id>/                       # 目录名 = 组件 id 的下划线写法（agent-status → agent_status）
      ├─ mod.rs                   # 只写模块声明与文档注释
      ├─ settings.rs              # 组件配置的解释者（与前端 settings.ts 同口径）
      └─ …                        # 状态、命令、采集、存储
```

⚠️ 目录名映射：前端 id `agent-status` ↔ Rust 目录 `agent_status`。这是唯一的命名换算点，别在别处再造花样。

### 2.3 哪些东西属于外壳（不要往组件里搬）

| 能力 | 位置 |
|---|---|
| 窗口尺寸 / 贴边 / 形态推导 | `src/components/floating/FloatingWindow.vue`、`src/lib/snap.ts`、`src/lib/floating-layout.ts` |
| 模块挂载与分槽 | `FloatingModuleHost.vue`、`FloatingModuleSlot.vue` |
| 组件启用 / 排序 / 上限 | `src/stores/floatingModules.ts`、`src-tauri/src/floating/module_config.rs` |
| 组件列表与齿轮入口 | `src/components/floating/FloatingModulesPanel.vue` |
| 一级 / 二级导航 | `src/components/MainWindow.vue` |
| tooltip 定位 / 显隐 / 点击穿透 | `src/components/TooltipWindow.vue`、`src/lib/tooltip.ts` |
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

---

## 4. `FloatingModuleDefinition` 字段逐个说明

定义在 `src/lib/floating-modules.ts`。类型是外壳的一部分，**新增字段 = 改外壳契约**，改完所有组件都要能编译通过。

| 字段 | 必填 | 说明 | 约束与坑 |
|---|---|---|---|
| `id` | ✅ | 组件唯一 id，同时是配置里的键 | 只允许字母 / 数字 / 短横线；一旦发布**不要再改**（改 = 丢掉用户配置与存档） |
| `label` | ✅ | 组件列表里的名字 | 4–8 个汉字最佳，别写成句子 |
| `description` | ✅ | 组件列表里的一句话说明 | 说清「它显示什么」，不要写实现细节 |
| `version` | ✅ | 组件自己的版本号 | 供将来做组件级配置迁移；与配置的 `schema_version` 不是一回事 |
| `defaultEnabled` | ✅ | 配置里没有该组件时的兜底启用状态 | 新组件建议 `false`（避免用户升级后被突然出现的东西吓到）；状态灯这类核心组件为 `true` |
| `supportedLayouts` | ✅ | 支持哪些形态 | 至少要支持 `square`；不支持时宿主**不挂载**（因此也别指望它在别的形态下存活） |
| `component` | ✅ | 悬浮窗里的视图组件 | 见第 5 节契约 |
| `demand` | ❌ | 该形态下要占几格（纯函数） | 见第 6 节；省略 = 恒占 1 格 |
| `settings` | ❌ | 设置页（分区数组） | 省略 / 空数组 = 列表里不显示齿轮，一级 tabbar 也不出现该组件页 |
| `tooltip` | ❌ | 悬停信息卡 | 见第 8 节；省略 = 悬停不弹窗 |
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
| `square` | 未贴边 | 56×56（1×1 格） | 与格子数无关，恒定 |
| `corner` | 贴四角 | 正方形，边长 = `ceil(√cells)` 格 | 空间比 `square` 宽裕，可显示更多 |
| `horizontal` | 贴上下边 | 横条，宽 = cells × 1 格 | **高度只有 1 格**，纵向极其紧张 |
| `vertical` | 贴左右边 | 竖条，高 = cells × 1 格 | **宽度只有 1 格**，横向极其紧张 |

**必须做与必须不做**：

- ✅ 用 grid / flex 比例、`minmax(0, 1fr)`、`clamp()`、省略号自适应。
- ✅ 用 `cellsGrid(layout, cells)`（`src/lib/floating-layout.ts`）换算行列，别自己写公式。
- ❌ 不要写死 `width: 56px` / `height: 240px` 这类固定像素布局。
- ❌ 不要假设「窗口一定比我需要的大」。
- ❌ 不要给元素加 `title`（会触发 Windows 原生提示，和 tooltip 重复且更简陋）；无障碍信息只走 `aria-label`。

### 5.3 被挤小时必须优雅退化

窗口总格数 = 各模块需求之和，夹在 `[默认 4 格, 该形态上限]`。到顶后各模块**按需求比例等分收缩**，所以你拿到的 `allocatedCells` 可能小于自己的 `demand`：

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
- 组件不要主动 popup / 弹窗 / 自己定位 tooltip —— 那是外壳的能力。---

## 6. 格子需求 `demand`

### 6.1 为什么声明在定义里，而不是组件自己上报

宿主需要在**不挂载组件**的前提下算出任意形态下窗口该多大 —— 主窗口的组件列表徽标与三形态预览就是这么来的。所以需求是「定义的一部分」，不是「组件的运行时状态」。

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
- 返回值：正整数格数；`NaN` / `Infinity` / 小数会被 `moduleCellWeight` 归一成「至少 1 格的整数」。
- 省略 `demand` = 恒占 1 格，这是大多数小部件的正解。
- 需求之和决定窗口大小：`resolveWindowCells(demands, maxCellsFor(layout))`，夹在 `[4, 该形态上限]`。
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

---

## 8. 信息卡（tooltip）契约

### 8.1 分工

| 谁 | 负责 |
|---|---|
| 组件 | 悬停时 `reportHover(key)`；声明 `tooltip.component` / `size` / `hasContent`；写内容组件 |
| 外壳 | 250ms 延迟、跨窗口 `emitTo('tooltip', …)`、定位（`placeTooltip`，未贴边一侧优先、越界换侧）、显隐、点击穿透、尺寸 `setSize` |
| 提示窗容器 | 毛玻璃外壳、滚动与省略号 |

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

- **多根节点**是允许且推荐的（与历史 DOM 结构一致，CSS 不用改）。
- 用 `TooltipRow` / `TooltipSection`（`src/components/floating/tooltip/`）保证行距与分组一致，别自造。
- 内容用 flex + 省略号自适应，不要假设自己能换行、能滚动。
- **不要**在内容组件里做定位 / 显隐 / `setSize`。
- 内容里**不要**加 `title`。

### 8.3 尺寸预算（硬约束）

`size` 是**物理像素**，与 `tauri.conf.json` 的 default 一致；高度是有预算的：

```text
head + Endpoint + token + 4 个拓展字段 + 刷新标注 + 提问 1 行 ≈ 236px（窗口高 260px）
```

- 加字段数 / 加行数必须同步核算窗口高度，超出会被裁掉。
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
- `activate` 可能在配置异步加载完成**之前**被调用：需要配置才能开跑的组件，先 `await useFloatingModulesStore().start()`（或订阅配置变化）再启动请求，别拿 `fallbackConfig` 直接打网。
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
Tauri 的窗口尺寸是**物理像素**，CSS 是逻辑像素。高分屏（scale factor 2）下 `setSize(56)` 的窗口在 CSS 里是 28px。因此：

- 组件布局不要跟窗口尺寸「对数字」，只按比例自适应。
- 需要精确对齐时，用 `getCurrentWindow().scaleFactor()` 换算，别凭经验乘常数。---

## 11. 配置持久化与迁移

### 11.1 文件与结构

位置：`<app-data>/floating-modules.json`（Windows：`%APPDATA%\<identifier>\floating-modules.json`）。

```jsonc
{
  "schema_version": 1,                 // 外壳线协议版本；**不得随手升**
  "revision": 7,                       // 单调递增，用于丢弃迟到 / 乱序的旧配置
  "modules": [{ "id": "agent-status", "enabled": true, "order": 0 }],
  "max_cells_by_layout": { "square": 8, "corner": 8, "horizontal": 8, "vertical": 8 },
  "module_settings": {                 // ★ 组件的家：键 = 组件 id，值 = 组件自己解释的对象
    "agent-status": { "completed_retention_ms": 30000, "token_unit": "tokens" }
  }
}
```

| 字段 | 归属 | 说明 |
|---|---|---|
| `modules[].enabled` / `.order` | 外壳 | 仅这两项与具体组件无关，故留外壳 |
| `max_cells_by_layout` | 外壳 | 整个悬浮窗的尺寸参数，同级组件共享 |
| `module_settings[<id>]` | **组件** | 外壳只按 id 原样存取，**不解释**任何字段 |

### 11.2 写入链路（Rust）

```text
前端 updateModuleSettings(id, patch)
  → invoke('set_floating_module_config')          [仅 main 窗口有权限]
  → revision = 当前 revision + 1
  → validate()：校验 schema_version / 模块 id（≤64 字符、不含 / 与 \）/ clamp 格子上限
  → persist()：先写 floating-modules.tmp 再 rename（原子，不留半截文件）
  → emit('floating-module-config')                 [三窗口都收，前端 applyConfig]
  → notify()：按登记顺序调用各组件的 config_hook  [组件据此刷新运行时状态，改完即时生效]
```

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
| 加字段 | 前端 `settings.ts` + Rust `settings.rs` 各加一处 + 默认值；组件自己 clamp | 不改 `schema_version`，不改外壳 |
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
- 从组件内部引用自己用 `crate::modules::<self>::…`（`lib.rs` 是唯一允许点名的地方）。

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
- 新增窗口时**必须**把 label 加进某份 capability 的 `windows` 数组，否则该窗口连事件监听都没有权限。

### 12.6 新增源 / 平台

组件内部若有多数据源（hook、rollout、轮询…），让它们**汇进同一个入口函数**再进内核，别各写一条链路绕开。新增平台 = 新增一个适配器实现 + 注册表加一个分支，其它代码零改动。

---

## 13. 跨窗口通信与 capability

### 13.1 三个窗口

| label | 用途 | 关键配置 |
|---|---|---|
| `floating` | 悬浮窗 | 初始 56×56、transparent、alwaysOnTop、不可缩放 |
| `main` | 设置主窗 | 1000×750、`visible: false`、自绘标题栏、关闭 = 隐藏到托盘 |
| `tooltip` | 悬停信息卡 | 300×260、`visible: false`、`focusable: false`、点击穿透 |

窗口都在 `src-tauri/tauri.conf.json` 声明；`src/App.vue` 按 label 分派组件。

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

- 打开设置页：悬浮窗 → `emitTo('main', OPEN_MODULE_SETTINGS_EVENT, { moduleId })`，载荷里的 `moduleId` 由槽位的 `data-module-id` 翻译而来，组件自己不需要知道。
- 悬停：组件只调 `reportHover(key)`，跨窗口转发由外壳负责。
- 组件**不要**自己 `emitTo` 主窗口、也不要自己创建窗口；要新能力先扩外壳契约。

---

## 14. 测试规范

### 14.1 位置与命名

| 类型 | 位置 | 命名 |
|---|---|---|
| 前端单测 | 与实现同目录 | `<name>.spec.ts`（vitest 的 include 是 `src/**/*.spec.ts` + `tests/unit/**/*.test.ts`） |
| Rust 单测 | 同文件底部 | `#[cfg(test)] mod tests { … }` |

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

在真机上把悬浮窗依次摆成：未贴边（方形）/ 贴上下边（横条）/ 贴左右边（竖条）/ 贴四角（大方块），逐个确认：

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

### 15.4 真机回归（只有人能做）

- [ ] 旧配置文件能自动迁移，迁移后旧字段从文件里消失、设置值不丢
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
14. **补文档**：需要时更新 `reference/doc/`（组件自己的说明），并把 AGENTS.md / CLAUDE.md 两处**逐字同步**（两个文件内容必须完全一致，CRLF；不要对它们跑 prettier）。

---

## 17. 常见错误与反例

| 做法 | 后果 |
|---|---|
| 外壳里写 `if (id === 'agent-status')` | 第二个组件出现时外壳变成 if 地狱；结构验收 grep 破线 |
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
| 形态枚举 | `square` / `corner` / `horizontal` / `vertical` | `src/lib/floating-layout.ts` |
| 默认窗口格数 | `DEFAULT_WINDOW_CELLS = 4` | 等于 2×2 或 4×1 |
| 格子上限范围 | `ABS_MIN_CELLS = 4` / `ABS_MAX_CELLS = 8` | 用户可配，后端也会 clamp |
| 单格像素 | `FLOATING_BASE_CELL_PX = 56` | 与 floating 窗口初值一致 |
| 方形 / 四角 / 横条 / 竖条尺寸 | `floatingWindowSize(layout, cells)` | 主窗预览与贴边共用同一口径 |
| 悬停延迟 | `SHOW_DELAY_MS = 250`（`FloatingWindow.vue`） | 到点才发 `tooltip-hover` |
| 提示窗默认尺寸 | 300×260（`tauri.conf.json`） | 组件可用 `tooltip.size` 覆盖 |
| 事件名 | `ufo-floating-runtime` / `-request` / `ufo-open-module-settings` / `tooltip-hover` / `tooltip-interaction` / `floating-module-config` | `src/lib/floating-bridge.ts`、`src/lib/tooltip.ts` |
| 窗口 label | `floating` / `main` / `tooltip` | 与 capability 的 `windows` 数组对应 |
| 配置键 | `modules[].enabled|order`、`max_cells_by_layout`、`module_settings[<id>]` | 外壳只解释前三项 |
| 注释要求 | 每个方法写功能；接口 / type 描述字段；关键代码写清在干什么 | AGENTS.md |