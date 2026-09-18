# 悬浮窗组件制作规范

本文是 UFO 悬浮窗组件（module）的权威制作规范。新增组件、修改组件生命周期、接入数据源、增加设置页或 tooltip、调整悬浮窗布局时，必须先按本文检查；本文与 `AGENTS.md` 冲突时，以 `AGENTS.md` 的全局约束为准。

## 1. 核心原则

悬浮窗不是一个「Agent 状态灯专用窗口」，而是一块可以拼装多个组件的容器。组件必须满足以下铁律：

1. **外壳不认识任何具体组件。** `src/App.vue`、`src/components/floating/**`、`src/components/MainWindow.vue`、`src/components/TooltipWindow.vue`、`src/lib/floating-*`、`src/stores/floatingModules.ts` 内不得出现组件 id、组件专有字段或组件专有文案。
2. **组件自己负责自己的垂直切片。** 视图、纯逻辑、字体/样式、store、设置分区、面板、tooltip 内容都放在 `src/modules/<id>/`。
3. **新增组件只改注册表，不改外壳。** 前端只需要在 `src/modules/index.ts` 增加一条定义；提示窗按 `moduleId` 分派，外层不需要知道组件内部。
4. **声明式需求，不反向依赖分配结果。** `demand(layout)` 只能依赖组件自己的数据和传入形态，不能依赖 `allocatedCells`，否则主窗预览和悬浮窗实际运行会形成回路。
5. **组件必须适配所有声明支持的形态。** 至少考虑 `square`、`corner`、`horizontal`、`vertical`；被声明支持的形态必须在最小可用格子和被压缩分配下都不裁切关键内容。
6. **一个外部数据源只能有一个轮询 owner。** 同一组件需要在三个窗口中存活时，只允许一个明确窗口发起定时请求；其他窗口订阅广播。不能让悬浮窗、主窗、tooltip 各跑一套定时器。
7. **配置到达前不能假定配置可用。** 组件生命周期可能在配置异步加载完成前被调用。数据 owner 必须等待外壳配置就绪，或者订阅配置修订号后再启动网络/定时任务。
8. **关掉组件就是停止组件的后台工作。** 组件被禁用后不得继续轮询、监听外部业务事件或保持高成本定时器；可以保留轻量级的事件订阅，但必须停止数据源。
9. **密钥不进配置块、不进事件、不进日志。** API Key、Token、密码等使用 OS 凭据管理器；配置里只存非敏感字段和必要的布尔状态。
10. **阻塞 I/O 不跑主线程。** Rust 侧网络、文件、数据库、凭据管理器操作使用异步命令配合 `spawn_blocking`；命令不得 `unwrap` / `expect` / panic，失败返回可展示的错误字符串。
11. **样式只用统一 token。** 毛玻璃、表面、边框、文本、状态色、圆角都从 `src/assets/main.css` 读取，不在组件里重新发明灰色系或透明色。
12. **可交互元素必须有语义和键盘入口。** 不把可点击 `div` 当按钮；不在悬浮窗使用 `title` 代替自己的 tooltip；不使用 `window.alert` / `window.confirm`。

## 2. 目录结构

前端组件固定放在：

```text
src/modules/<id>/
├── index.ts                         # 组件定义、注册入口、生命周期
├── <Name>Module.vue                 # 悬浮窗视图
├── <Name>Tooltip.vue                # 可选：悬停信息卡内容
├── <id>.css                         # 组件自己的样式
├── demand.ts                        # 可选：格子需求函数；不写表示固定 1 格
├── settings.ts                      # 组件配置字段、默认值、解析、夹取
├── settings.spec.ts                 # 可选：配置解析测试
├── lib/                             # 可选：纯逻辑、格式化和对应测试
│   ├── <logic>.ts
│   └── <logic>.spec.ts
├── sections/                        # 可选：主窗组件设置分区
│   └── GeneralSection.vue
├── panels/                          # 可选：需要占满高度的编辑面板
│   └── <Panel>.vue
└── stores/                           # 可选：组件自己的 Pinia store
    └── <id>.ts
```

组件 id 规则：

- 只允许 ASCII 字母、数字、短横线，例如 `agent-status`、`weather`、`git-branch`。
- id 一旦发布，不得因为改文案或改组件名而更换；它是配置键、事件命名空间和 Rust 目录映射的一部分。
- Rust 目录使用 snake_case，例如 `agent-status` 对应 `src-tauri/src/modules/agent_status/`。
- 不要在组件目录外建同名逻辑文件；跨目录导入使用 `@/modules/<id>/...` 或 Rust 的全路径 `crate::modules::<id>::...`。

## 3. 前端组件契约

组件定义的类型是 `FloatingModuleDefinition`。字段含义如下：

| 字段 | 必填 | 说明 |
|---|---:|---|
| `id` | 是 | 组件唯一标识；配置键、Rust 目录和 tooltip 分派都依赖它 |
| `label` | 是 | 主窗组件列表中的名称 |
| `description` | 是 | 主窗列表中的一句话说明，只描述结果，不写快捷键或操作教程 |
| `version` | 是 | 组件自己的配置/数据版本；用于未来迁移，不要随 UI 文案随意变化 |
| `defaultEnabled` | 是 | 配置中没有该组件时是否默认启用；新组件通常从 `false` 开始 |
| `supportedLayouts` | 是 | 支持挂载的 `FloatingLayout[]`；不支持的形态不挂载、不参与尺寸汇总 |
| `component` | 是 | 悬浮窗视图组件 |
| `demand` | 否 | `(layout) => number`，该形态下希望占用的格子数；省略表示 1 格 |
| `settings` | 否 | 主窗设置页分区；没有可配置项就不要给 |
| `tooltip` | 否 | 悬停信息卡；没有悬停内容就不要给 |
| `activate` | 否 | 生命周期函数；每个窗口挂载时调用一次，返回清理函数 |

### 3.1 注册方式

新增前端组件只修改 `src/modules/index.ts`：

```ts
import type { FloatingModuleDefinition } from '@/lib/floating-modules'
import { agentStatusModule } from './agent-status'
import { weatherModule } from './weather'

export const FLOATING_MODULE_DEFINITIONS: FloatingModuleDefinition[] = [
  agentStatusModule,
  weatherModule,
]
```

不要在 `App.vue`、`FloatingWindow.vue`、`FloatingModuleHost.vue`、`MainWindow.vue` 里写组件分支。

### 3.2 `index.ts` 推荐骨架

```ts
import type { FloatingModuleDefinition } from '@/lib/floating-modules'
import MyModule from './MyModule.vue'
import MyTooltip from './MyTooltip.vue'
import GeneralSection from './sections/GeneralSection.vue'
import { myDemand } from './demand'
import { useMyModuleStore } from './stores/my-module'
import './my-module.css'

/**
 * 组件定义：所有和组件有关的注册信息只放在这里。
 */
export const myModule: FloatingModuleDefinition = {
  id: 'my-module',
  label: '我的组件',
  description: '一句话说明用户在悬浮窗里能看到什么。',
  version: 1,
  defaultEnabled: false,
  supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
  component: MyModule,
  demand: myDemand,
  settings: {
    sections: [
      { id: 'general', label: '常规', component: GeneralSection },
    ],
  },
  tooltip: {
    component: MyTooltip,
    size: { width: 300, height: 260 },
    hasContent: () => hasUsableData(),
  },
  activate: () => {
    const store = useMyModuleStore()
    void store.start()
    return () => store.stop()
  },
}
```

`activate()` 不要用 `onMounted` 替代。提示窗在某些情况下要在内容组件挂载之前调用 `tooltip.hasContent`，因此事件订阅和数据启动必须在模块定义生命周期里完成。

## 4. 生命周期与数据源

### 4.1 调用时序

应用启动时，外壳会：

1. 创建 `floating`、`main`、`tooltip` 三类窗口。
2. 在每个窗口调用 `activateModules(FLOATING_MODULE_DEFINITIONS)`。
3. 组件的 `activate()` 被调用；返回值在窗口卸载时清理。

重要事实：

- `activate()` 不保证组件已经挂载，也不保证用户能看见窗口。
- `activate()` 不保证 `floatingModules.start()` 已经完成。
- 三类窗口都会调用 `activate()`，但是每个窗口有独立的 Pinia store 和独立的事件监听环境。
- `main`、`tooltip` 可能处于隐藏状态，不能默认用户已经打开它们。

### 4.2 推荐的启动顺序

组件需要外部数据时，按以下顺序启动：

```ts
async function start(): Promise<void> {
  // 1. 先等待外壳配置就绪；不要直接读 fallback 配置后就发请求。
  await useFloatingModulesStore().start()

  // 2. 先注册跨窗口监听，避免数据到达早于监听。
  if (!stopListener) {
    stopListener = await listen<MyPayload>('my-module-update', event => {
      applyPayload(event.payload)
    })
  }

  // 3. 订阅本组件配置；组件开关、周期、位置等变化都要在这里重新同步。
  stopConfigWatch ??= watch(
    () => mySettings(useFloatingModulesStore().config),
    () => syncFromConfig(),
    { deep: true, immediate: true },
  )

  // 4. 只有明确的 owner 窗口启动定时器。
  syncFromConfig()
}
```

`start()` 和 `stop()` 必须幂等：重复调用不会创建第二个监听器、第二个定时器或第二条广播链路。

### 4.3 谁负责请求数据

同一组件在三个窗口中都会激活时，必须指定唯一数据 owner。推荐规则：

- 持续运行且始终存在的窗口优先作为 owner；UFO 中通常选择 `floating`。
- 主窗、tooltip 只负责订阅广播和展示，不负责周期性请求。
- owner 必须同时满足「组件已启用」和「当前窗口是 owner」两个条件。
- 组件禁用时，owner 停止定时器；其他窗口不要偷偷接管轮询。
- 窗口打开顺序不确定时，不要用 `setTimeout` 猜测负责窗口；使用窗口 label 或显式的启动协调协议。

推荐代码模式：

```ts
const windowLabel = getCurrentWindow().label
const isOwnerWindow = windowLabel === 'floating'

function syncFromConfig(): void {
  const enabled = isModuleEnabled(modules.config, 'my-module', false)
  if (!enabled) {
    stopPolling()
    return
  }

  if (!isOwnerWindow) {
    // 非 owner 可以做一次只读/缓存补齐，但不能创建周期定时器。
    if (!snapshot.value) void fetchSnapshot(false)
    return
  }

  restartPolling()
  void fetchSnapshot(true)
}
```

如果后端命令可能在多个窗口同时首次调用，Rust 侧应做缓存和 in-flight 去重；不能依赖前端调度顺序保证只打一次网络。

### 4.4 清理清单

`stop()` 必须清理：

- `listen()` 返回的 `UnlistenFn`
- `watch()` 返回的停止函数
- `setInterval` / `setTimeout`
- 正在进行的请求的取消标记或过期标记
- 组件自己的连接、AbortController、轮询状态
- 临时资源和缓存

即使组件被禁用，不清除 `listen` 本身通常可以接受；但必须停止高频业务数据请求和外部网络。

## 5. 配置、密钥与迁移

### 5.1 配置归属

悬浮窗级配置只保留：

- 组件开关 `modules[].enabled`
- 排列顺序 `modules[].order`
- 各形态格子上限 `max_cells_by_layout`

组件自己的字段必须放在：

```json
{
  "module_settings": {
    "my-module": {
      "refresh_interval_min": 15,
      "location_mode": "auto"
    }
  }
}
```

外壳只按 `module_settings[<id>]` 整块透传，不解释字段。组件读配置时：

1. 从 `moduleSettings('my-module')` 取原始对象。
2. 用组件自己的 `settings.ts` 解析。
3. 对每个字段提供默认值。
4. 对数字、枚举、字符串做夹取和归一化。
5. 读取失败只影响当前字段，不要让整块配置失效。

禁止：

- 在外壳文件里写 `my-module` 专有字段名。
- 用 `schema_version` 升级代替 `Option` + 默认值解析。
- 把一个组件字段平铺到 `floating-modules.json` 顶层。
- 把密钥、Token、密码放进 `module_settings`。

### 5.2 TypeScript `settings.ts` 模板

```ts
import type { FloatingModuleConfig } from '@/stores/floatingModules'

export interface MyModuleSettings {
  /** 数据刷新间隔，单位分钟。 */
  refresh_interval_min: number
  /** 用户选择的位置模式。 */
  location_mode: 'auto' | 'manual'
}

export const DEFAULT_MY_MODULE_SETTINGS: MyModuleSettings = {
  refresh_interval_min: 15,
  location_mode: 'auto',
}

/**
 * 将外壳的未知配置解析成组件自己的强类型字段。
 */
export function myModuleSettings(config: FloatingModuleConfig | null): MyModuleSettings {
  const raw = (config?.module_settings?.['my-module'] ?? {}) as Record<string, unknown>
  return {
    refresh_interval_min: clampNumber(raw.refresh_interval_min, 10, 60, 15),
    location_mode: raw.location_mode === 'manual' ? 'manual' : 'auto',
  }
}
```

### 5.3 密钥规范

敏感值包括但不限于 API Key、Bearer Token、密码、Cookie、私钥、云服务凭据。

规则：

1. 敏感值只存 OS 凭据管理器（Windows 凭据管理器 / macOS Keychain / Linux Secret Service）。
2. keyring service 建议使用 `<app-identifier>/<module-id>`，name 使用组件内部唯一字段名。
3. 配置里只允许存 `has_secret: true` 之类的状态，不能存明文或可逆摘要。
4. 写入/删除密钥的 Tauri 命令只授权给 `main` 窗口。
5. 读取密钥优先在 Rust 命令内部按需读取；不要把值返回给前端。
6. 前端输入框默认不回显已存密钥；只显示「已保存，输入新值可替换」。
7. 清除密钥必须走独立命令；不要用空字符串覆盖配置字段来代表删除。
8. 密钥不得进入 tooltip、事件 payload、日志、错误字符串和 code comment。

设置页建议形态：

- 「已保存状态」只读展示。
- 新值输入框留空表示不修改。
- 明确的「保存密钥」和「清除密钥」按钮。
- 测试连接时允许使用当前输入值覆盖已存值，但值不得持久化到配置。

## 6. Rust 侧结构

### 6.1 目录与注册

Rust 组件目录与前端组件一一对应：

```text
src-tauri/src/modules/<module_snake>/
├── mod.rs              # Tauri command、状态、广播入口
├── settings.rs         # 配置解析、默认值、夹取、迁移
├── models.rs           # 前后端协议模型
├── client.rs           # 外部 HTTP/client 封装
├── cache.rs            # 进程内缓存和去重
└── secrets.rs          # 可选：keyring 访问
```

新增 Rust 组件时至少完成：

1. `src-tauri/src/modules/mod.rs` 注册模块目录。
2. `src-tauri/src/lib.rs` 管理组件状态、注册命令、登记配置迁移或变更钩子。
3. 新建 `src-tauri/permissions/<module>.toml` 声明命令 permission。
4. 在前端 capability 文件中授权：读命令给三窗口，写设置/密钥命令只给 `main`。
5. 在 `reference` 里补充该组件的数据源和外部依赖说明（如本文不适用的部分）。

### 6.2 命令设计

读命令命名建议：

- `get_<module>_snapshot`
- `get_<module>_state`
- `get_<module>_secret_status`

写命令命名建议：

- `save_<module>_settings`
- `set_<module>_secret`
- `clear_<module>_secret`
- `test_<module>_connection`

权限边界：

| 命令类型 | 授权窗口 | 原因 |
|---|---|---|
| 读取普通展示数据 | `floating`、`main`、`tooltip` | 三个窗口都可能展示 |
| 修改组件开关/排序/上限 | `main` | 设置只允许主窗操作 |
| 修改组件普通设置 | `main` | 防止隐藏窗口或 tooltip 修改配置 |
| 设置/清除密钥 | `main` | 避免后台窗口静默写敏感凭据 |
| 测试连接 | `main` | 测试属于设置交互，不进入展示链路 |

同一命令必须同时登记在：

1. `#[tauri::command]`
2. `src-tauri/permissions/<module>.toml`
3. `src-tauri/capabilities/*.json`

漏掉任何一处，窗口可能能在代码里调用，但运行时被 ACL 拒绝。

### 6.3 阻塞 I/O 与错误

外部 HTTP、SQLite、文件扫描和凭据管理器都属于阻塞 I/O：

```rust
#[tauri::command]
pub async fn get_my_module_snapshot(
    state: State<'_, MyModuleState>,
) -> Result<MySnapshot, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        fetch_snapshot(&state).map_err(|error| format!("读取失败: {error}"))
    })
    .await
    .map_err(|error| format!("后台任务失败: {error}"))?
}
```

要求：

- 不在主线程做网络、磁盘扫描或 keyring 访问。
- 不用 `unwrap()` / `expect()` / `panic!()` 处理用户数据或外部响应。
- `spawn_blocking` 的 join 失败也要转成普通错误。
- 错误信息不包含密钥、完整 Authorization Header 或原始凭据。
- 长请求要有超时；响应体要有大小上限。
- 一个外部 endpoint 的缓存写入、并发请求和失败退避必须在 Rust 侧收口。

### 6.4 网络与 URL 安全

如果组件会出网：

- 默认只允许 HTTPS；确实需要 HTTP 的公共定位服务要在文档中写明例外原因和隐私风险。
- 不允许用户自定义 Host 直接访问 loopback、私网、链路本地、CGNAT、云元数据和短主机名。
- 重定向限制次数；跟随重定向后重新校验最终地址。
- 每个请求设置连接/整体超时。
- 响应体设置上限，例如 256 KiB；不要无上限 `read_to_string`。
- 用户自定义 Host 要校验 scheme、host、端口和尾部斜杠，不能直接拼进 URL。
- 错误信息中掩去 API Key、query secret 和响应正文中的敏感内容。

### 6.5 配置迁移与运行时同步

新增字段优先使用 `Option` + `#[serde(default)]`，让旧配置可读。

组件自己的迁移钩子只做两件事：

1. 把旧版扁平字段认领进 `module_settings[<id>]`。
2. 把新配置同步到组件运行时。

禁止：

- 因为新增组件字段而提升浮动窗协议版本。
- 在外壳的 `floating/` 目录写组件专有字段。
- 在外壳的 `floating-modules.json` 顶层新增组件字段。
- 用迁移逻辑覆盖用户已经写入的新字段。

## 7. 三形态适配

UFO 的窗口尺寸随贴边状态变化。组件不能把内容盒写死为某个像素尺寸。

### 7.1 形态定义

| 形态 | 触发条件 | 布局特征 | 组件策略 |
|---|---|---|---|
| `square` | 未贴边 | 默认 56×56；多组件时可能等比压缩 | 只保留最强信息，必须有最小形态 |
| `corner` | 贴到四角 | 更大的方形 | 可以增加第二行、位置、状态文本 |
| `horizontal` | 上下贴边 | 宽度放大 | 使用列布局，左侧核心状态，右侧数值/次级信息 |
| `vertical` | 左右贴边 | 高度放大 | 使用行布局，上方核心状态，下方数值/次级信息 |

即使当前只声明支持部分形态，未支持的形态也必须从 `supportedLayouts` 排除，不能靠视觉裁切蒙混过去。

### 7.2 `demand` 与 `allocatedCells`

`demand(layout)` 表示「希望占几格」，`allocatedCells` 表示「宿主实际分到几格」。

必须遵守：

```ts
const moduleApi = inject(floatingModuleKey, null)
const demand = computed(() => myDemand(props.layoutMode))
const budget = computed(() => Math.min(demand.value, moduleApi?.allocatedCells.value ?? demand.value))
```

- 计算窗口尺寸时使用 `demand`。
- 渲染内部网格、文字密度、显隐层级时使用 `budget`。
- `budget` 至少按 1 格处理。
- 当 `budget < demand` 时，必须有降级视图，例如只显示图标、主数值或 `+N`。
- 不能因为 `allocatedCells` 小于声明值就继续按原尺寸绘制。

### 7.3 尺寸布局技术要求

推荐使用：

```css
.my-module {
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  display: flex;
}

.my-module__content {
  min-width: 0;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  grid-template-rows: minmax(0, 1fr);
}
```

禁止：

- 给根节点写固定 `width: 56px` 或 `height: 56px`。
- 用视口宽度字号，例如 `font-size: 2vw`。
- 依赖横向滚动来容纳内容。
- 让长文本把父级撑开；使用 `min-width: 0`、`text-overflow: ellipsis` 或分段显示。
- 在悬浮窗内部放入需要用户滚动才能看全的主内容。
- 只按 `layoutMode` 写死尺寸，不考虑同形态下的实际分配格子。

### 7.4 最小内容预算

每个组件都要定义自己的最小形态：

| 预算 | 建议内容 |
|---|---|
| 1 格 | 一个图标、一个状态点、一个主数值或 `+N` |
| 2 格 | 图标 + 一行核心文字，或两个并排指标 |
| 更多格 | 增加次级字段、趋势、位置、更新时间等 |

文本优先级建议：主状态 > 主数值 > 单位 > 位置/来源 > 更新时间 > 说明。空间不足时从后往前省略，不能把主状态裁掉。

### 7.5 形态验收矩阵

每个组件至少验证以下组合：

| 场景 | square | corner | horizontal | vertical |
|---|---:|---:|---:|---:|
| 组件单独启用 | 必测 | 必测 | 必测 | 必测 |
| 与默认 Agent 状态灯同时启用 | 必测 | 必测 | 必测 | 必测 |
| 三个以上组件同时启用 | 必测 | 必测 | 必测 | 必测 |
| 长文案 / 长城市名 / 大数字 | 必测 | 必测 | 必测 | 必测 |
| 无数据 / 加载中 / 错误 | 必测 | 必测 | 必测 | 必测 |

验收时要把窗口贴到每个角、上下边、左右边，不能只看未贴边截图。

## 8. 视觉与样式规范

### 8.1 Token 与表面

组件必须复用 `src/assets/main.css` 中的变量：

```css
.my-module {
  color: var(--ufo-text);
  background: transparent;
  border-color: var(--ufo-border);
}

.my-module__surface {
  background: var(--ufo-surface-2);
  border: 1px solid var(--ufo-border);
  border-radius: var(--ufo-radius);
}
```

禁止在组件 CSS 里散落：

- `#3D3D3D`、`#2A2A2A`、`#323232`、`#1F1F1F`
- `rgba(255,255,255,0.08)`、`rgba(255,255,255,0.14)`
- 任意灰阶、任意状态色的重复定义
- 用 `border-white/5`、`bg-white/5` 代替语义 token

状态色只用：

- `--ufo-status-green`
- `--ufo-status-yellow`
- `--ufo-status-red`
- `--ufo-accent`
- `--ufo-danger`

### 8.2 组件 CSS 位置

组件自己的样式放在 `src/modules/<id>/<id>.css`，由 `index.ts` import。只有窗口外壳、宿主、公共控件、公共 tooltip 行/分组可留在 `src/assets/main.css`。

独立 CSS 使用 Tailwind `@apply` 时，文件头必须写：

```css
@reference "../../assets/main.css";
```

否则 Tailwind v4 在独立 CSS 中无法解析工具类。

### 8.3 图标与按钮

- 有 lucide 对应图标时优先使用 lucide。
- 图标按钮必须有 `aria-label` 或可见文本。
- 图标只有装饰意义时加 `aria-hidden="true"`。
- 悬浮窗内真实按钮必须阻止冒泡，避免外层把点击解释为“打开组件设置”：

```vue
<button type="button" class="my-module__action" @click.stop="doAction">
  <span aria-hidden="true">...</span>
  <span class="sr-only">执行操作</span>
</button>
```

### 8.4 原生提示

不要在悬浮组件上加 `title` 属性来补信息。`title` 会触发 Windows 原生气泡，和 UFO 自己的 tooltip 重复且不可控。无障碍信息使用 `aria-label` / `aria-describedby` / 可见文本。

## 9. 设置页规范

### 9.1 分区划分

设置页是两级导航：

- 一级：主窗组件列表，或每个有 `settings` 的组件页面。
- 二级：当前组件的 `settings.sections`。

分区按版面划分，不按配置项划分：

- 短表单、开关、下拉：合在一个「常规」分区。
- 需要占满高度或左右主从编辑：独立分区，并标 `fill: true`。
- 只给一个分区时不要渲染二级 tabbar。

### 9.2 组件设置页结构

推荐结构：

```vue
<template>
  <div class="general-cards">
    <section class="ufo-card p-4 space-y-4">
      <header>
        <h3 class="text-sm font-semibold text-[var(--ufo-text)]">服务配置</h3>
        <p class="text-xs text-[var(--ufo-muted)]">只描述字段用途，不写操作教程。</p>
      </header>
      <SettingsField label="字段名" description="字段说明" layout="stacked">
        <BaseInput v-model="value" />
      </SettingsField>
    </section>
  </div>
</template>
```

使用公共控件：

- `BaseButton`
- `BaseInput`
- `BaseSwitch`
- `BaseSelect`
- `BaseSegmentedControl`
- `BaseTabs`
- `SettingsField`

### 9.3 可访问性与反馈

- 所有表单控件必须有可见 label 或 `SettingsField` 包裹。
- 错误提示放在对应字段附近；不要只把错误显示在页面顶部。
- 列表选择项必须是 `<button type="button">` 或等价可键盘控件，不能是只有 `@click` 的 `div`。
- 成功、失败、测试中状态使用 `role="status"` 或 `aria-live="polite"`。
- 禁止 `window.alert` / `window.confirm`；项目安装了 `tauri-plugin-prevent-default`，原生对话框会被拦截。
- 危险操作使用两步确认，不弹原生确认框。
- 滚动容器必须使用 `scrollbar-gutter: stable`，列表、详情面板、日志和编辑器都不能在滚动条出现时挤压内容。

### 9.4 密钥字段

设置页不显示已存密钥原文。推荐：

```text
API Key
[ 输入新值，留空表示保持不变                         ]
[保存密钥] [清除密钥]
状态：已保存 / 未保存
```

测试连接和搜索可以临时使用输入框里的新值；测试结果不得把密钥回显到页面或日志。

## 10. Tooltip 规范

### 10.1 声明

```ts
tooltip: {
  component: MyTooltip,
  size: { width: 300, height: 320 },
  hasContent: target => hasUsableData(target),
}
```

- `component` 只负责内容，不负责窗口定位。
- `size` 是默认尺寸覆盖；改字段数量、字体大小或行数时必须同步检查。
- `hasContent` 必须在数据足够时返回 `true`；如果故意展示“加载中/去设置”提示，可以返回 `true`，但文案必须明确。

### 10.2 内容结构

- 内容组件使用多根节点，不要额外套一层 `div`；`.tooltip-shell` 本身就是行级 flex 容器。
- 单行用 `TooltipRow`。
- 分组用 `TooltipSection`。
- 长文本设置 `title` 只用于 tooltip 内的完整原文；悬浮窗根节点不要用原生 `title`。
- 关键字段用语义色，不要只用颜色表达状态。
- 内容宽度变化时允许省略，但主字段不能被省略掉。
- tooltip 不是独立三形态窗口，不需要适配 floating 的形态；但其内容必须在窗口尺寸内完整显示。

### 10.3 空间预算

新增一行工具提示前，先计算：

```text
标题行 + 主字段行 + 可选分组 + 分隔线 + padding <= tooltip.height
```

如果字段会动态增加：

- 限制最大字段数。
- 超出时合并为“其他 N 项”。
- 不要依赖 tooltip 自身滚动；内容应完整排版在声明尺寸内。

## 11. 样式归属边界

外壳样式可以包含：

- `.main-*`
- `.floating-*`
- `.ufo-*`
- `.module-catalog*`
- `.module-row*`
- `.module-settings*`
- `.shape-preview*`
- `.general-cards`
- `.tooltip-shell|row|section*`

组件样式放在自己目录。验收命令：

```powershell
rg -n "my-module|myModule" src/App.vue src/components/floating src/lib/floating-modules.ts src/lib/floating-module-context.ts
rg -n "my_module_field|my-module-field" src-tauri/src/floating src-tauri/src/mouse.rs src-tauri/src/devtools.rs
```

两条都应无命中。组件自己的目录、组件 spec 和组件 Rust 目录除外。

## 12. 跨窗口事件规范

### 12.1 原则

- 事件名必须带组件 id 作为命名空间，例如 `my-module-update`、`my-module-error`。
- payload 只传展示所需的最小快照，不传 Pinia store、DOM、全量配置或密钥。
- 接收端必须校验 payload 形状；不要把 `any` 直接灌给 store。
- 组件事件只能由组件自己的代码订阅/发布，外壳不得解释 payload 字段。

### 12.2 事件订阅与错过的首帧

窗口打开顺序不确定。需要首帧数据时：

1. 先订阅事件。
2. 再读取缓存或请求快照。
3. 如果窗口可能晚于广播打开，提供一次 `get` 或 request/replay 协议。

不要用「先发一次，再订阅」的顺序，否则主窗或 tooltip 会永久停留在空状态。

### 12.3 广播与窗口控制

- 向所有窗口广播用 `app.emit` / `emit` 的广播能力。
- 向指定窗口发送用 `emitTo(label, event, payload)`。
- ACL 校验的是调用方窗口权限；目标 label 只是参数。
- 调用 `startDragging()`、点击、贴边变化后要 `dismissTooltip()`，避免残留悬停状态。
- 指针离开、`pointercancel`、拖拽开始都要清理 hover key。

## 13. 测试要求

### 13.1 前端单元测试

至少覆盖：

- `settings.ts`：空配置、错类型、边界值、夹取、默认值。
- `demand.ts`：每个支持形态的返回值；不得依赖 `allocatedCells`。
- 视图模型/格式化函数：空值、错误值、长文本、单位转换。
- 事件 payload 校验：缺字段、错类型、旧版本兼容。
- 生命周期：重复 `start()`、重复 `stop()`、禁用后不启动 poll、owner 窗口唯一。

### 13.2 Rust 单元测试

至少覆盖：

- 配置解析、默认值、夹取和旧字段迁移。
- wire model 的反序列化兼容，尤其是 `#[serde(default)]` 字段。
- 缓存命中、过期、并发去重和失败退避。
- API 错误码、空响应、畸形 JSON、超时和响应体上限。
- 密钥缺失时返回明确错误；错误不包含密钥。
- 命令权限边界由 capability 文件锁定。

### 13.3 手工验收

每次改布局或视觉，必须实际验证：

- 未贴边方形：56×56。
- 四个角：放大 square。
- 上边 / 下边：horizontal。
- 左边 / 右边：vertical。
- 单组件、多组件、组件被挤到 1 格、超过上限。
- 加载、空数据、错误、长文本、大数字。
- 悬停 tooltip 定位、换侧、点击穿透、拖拽取消。
- 键盘 Tab、Enter/Space 选择、屏幕阅读器标签。

## 14. 新增组件实施清单

### 14.1 开始前

- [ ] 组件是否真的需要一个悬浮窗组件，而不是主窗功能？
- [ ] 组件的 id、label、description、默认启用态确定了？
- [ ] 组件需要哪些形态，哪些形态可以只显示降级内容？
- [ ] 数据从哪里来，是否涉及网络、密钥、文件或系统权限？
- [ ] 是三个窗口都显示，还是只有一个窗口作为数据 owner？

### 14.2 前端

- [ ] 建好 `src/modules/<id>/` 目录。
- [ ] 视图、样式、settings、demand、store 都放在组件目录。
- [ ] `index.ts` 只声明定义、设置分区、tooltip 和生命周期。
- [ ] `settings.ts` 有字段注释、默认值、解析和夹取。
- [ ] `demand.ts` 不依赖 `allocatedCells`。
- [ ] 视图读取 `allocatedCells` 并实现降级布局。
- [ ] `activate()` 等待外壳配置就绪；监听先于请求；返回清理函数。
- [ ] 组件禁用后不轮询、不发起外部请求。
- [ ] 只允许一个窗口成为轮询 owner。
- [ ] 组件 CSS 使用 `--ufo-*` token，不写死灰色系。
- [ ] 不使用原生 `title` 替代 tooltip。
- [ ] 可交互元素有键盘语义和 `@click.stop`。
- [ ] 设置页滚动容器有 `scrollbar-gutter: stable`。

### 14.3 Rust

- [ ] 建好 `src-tauri/src/modules/<module_snake>/` 目录。
- [ ] 配置字段有默认值、边界和迁移策略。
- [ ] 阻塞 I/O 走 `spawn_blocking`。
- [ ] 网络请求有超时、重定向限制、响应上限和 URL 策略。
- [ ] 有共享缓存和 in-flight 去重，避免多窗口重复请求。
- [ ] 密钥走 OS 凭据管理器，配置只存状态。
- [ ] 命令注册到 `#[tauri::command]`、permission 文件和 capability 文件。
- [ ] 读命令和写命令的窗口授权边界正确。
- [ ] 失败不 panic，错误不含敏感信息。

### 14.4 注册和权限

- [ ] `src/modules/index.ts` 增加组件定义。
- [ ] `src-tauri/src/modules/mod.rs` 增加 Rust 模块。
- [ ] `src-tauri/src/lib.rs` 注册状态、命令、迁移/配置钩子。
- [ ] `src-tauri/permissions/<module>.toml` 增加 permission。
- [ ] `default.json` 只授权三窗口需要的读命令。
- [ ] `main-module-settings.json` 授权主窗写命令和密钥命令。

### 14.5 测试和验收

- [ ] `npx vitest run` 通过。
- [ ] `npm run build` 通过。
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` 通过。
- [ ] `git diff --check` 通过。
- [ ] 三形态矩阵手工验证完成。
- [ ] 多组件压缩状态已验证。
- [ ] 禁用状态确认没有后台请求和定时器。
- [ ] tooltip 在所有边缘位置均不被裁切。
- [ ] 键盘和无障碍检查完成。

## 15. 常见反模式

以下实现一律视为不符合规范：

1. 在 `App.vue` 里 `v-if="windowLabel === 'weather'"` 或出现组件专有词。
2. 在 `floating-modules.json` 顶层写入 `weather_city`、`git_branch` 等组件字段。
3. 在 `FloatingWindow.vue` 里直接 import 某个组件内部 store。
4. 在 `FloatingModuleHost.vue` 里写某个组件的尺寸、图标或文案逻辑。
5. 组件的 `demand()` 读取 `allocatedCells`。
6. 三个窗口都创建 `setInterval` 请求同一外部接口。
7. 组件禁用后仍然持续请求网络。
8. 配置异步加载未完成时直接读取 fallback 配置并发请求，之后也不重试。
9. 把 API Key、Token 或 Cookie 写入 `module_settings`。
10. 把密钥放进 `app.emit` / `emitTo` payload。
11. 使用固定 `width/height` 的根容器，导致贴边后被裁切。
12. 依赖 hover 才能完成核心操作；触摸、键盘用户无法使用。
13. 用 `title` 触发 Windows 原生提示，与 UFO tooltip 重复。
14. 用可控的 `div @click` 当按钮。
15. 让 tooltip 内容组件套一层 `div`，破坏外层 flex 和 `mt-auto`。
16. 在没有 `scrollbar-gutter: stable` 的列表/编辑器里让滚动条挤占内容。
17. 在组件 CSS 中散落灰色硬编码，绕过 Matte token。
18. 为了新增组件字段提升 `SCHEMA_VERSION`，导致旧 hook/旧事件被静默丢弃。

## 16. 参考实现与 mock

- 模块注册与生命周期：`src/lib/floating-modules.ts`
- 形态和尺寸纯函数：`src/lib/floating-layout.ts`
- 宿主与槽位：`src/components/floating/FloatingModuleHost.vue`、`FloatingModuleSlot.vue`
- 模块上下文：`src/lib/floating-module-context.ts`
- 现有垂直切片：`src/modules/agent-status/`
- 浮动组件三套布局 mock：`reference/mock/floating-modules-index.html`
- 设置页导航 mock：`reference/mock/settings-nav-index.html`
- Matte 主窗预览：`reference/mock/main-window-a-matte.html`
- Agent 状态 hook 约定：`reference/doc/agent-status-hooks.md`
