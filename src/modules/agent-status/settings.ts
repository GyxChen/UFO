/**
 * agent-status 组件自己的配置：字段名、默认值、取值范围都只在这里解释。
 *
 * 悬浮窗外壳（`@/lib/floating-modules` + `@/stores/floatingModules`）只按 id 存一块 JSON，
 * 不认识里面任何字段；因此给本组件加一个配置项只需要改本文件 + 设置页，
 * 既不用动外壳也不用迁移配置（读不到键就回落这里的默认值）。
 * Rust 侧的对应实现是 `src-tauri/src/modules/agent_status/settings.rs`，两边口径必须一致。
 */
import { invoke } from '@tauri-apps/api/core'
import { ABS_MIN_CELLS } from '@/lib/floating-layout'
import type { FloatingModuleConfig } from '@/lib/floating-modules'

/** 本组件在外壳配置里的键（`module_settings` 的键，也是注册表里的模块 id）。 */
export const AGENT_STATUS_MODULE_ID = 'agent-status'

/**
 * token 展示单位，与 Rust 侧序列化值一一对应。
 * - `tokens`：原始计数，如 `1,261,847`
 * - `m_tokens`：以百万为单位，如 `1.26M`
 */
export type TokenUnit = 'tokens' | 'm_tokens'

/** token 单位默认值：保持原始计数，与加入该设置之前的行为一致。 */
export const DEFAULT_TOKEN_UNIT: TokenUnit = 'tokens'

/** 不解析（直连）的固定 id，与 Rust `NONE_RESOLVER` 一致。 */
export const NONE_PROXY_RESOLVER = 'none'

/** 代理解析器下拉项：id 存进配置并作 value，label 供 UI 展示，description 是菜单里的一句说明。与 Rust `ProxyResolverInfo` 对应（旧后端可能不带说明，故可选）。 */
export interface ProxyResolverInfo {
  id: string
  label: string
  description?: string
}

/** 完成灯保留时长：默认 30 秒，可选 1 秒 ..= 24 小时。 */
export const DEFAULT_RETENTION_SECONDS = 30
export const MIN_RETENTION_SECONDS = 1
export const MAX_RETENTION_SECONDS = 86_400

/** Running 无事件超时（界面标题「无响应超时」）：默认 60 秒，可选 1 分钟 ..= 60 分钟（慢 agent 可拉长）。 */
export const DEFAULT_RUNNING_TIMEOUT_SECONDS = 60
export const MIN_RUNNING_TIMEOUT_SECONDS = 60
export const MAX_RUNNING_TIMEOUT_SECONDS = 60 * 60
/** 同一区间的分钟口径（设置页输入用）：1 ..= 60 分钟。 */
export const MIN_RUNNING_TIMEOUT_MINUTES = MIN_RUNNING_TIMEOUT_SECONDS / 60
export const MAX_RUNNING_TIMEOUT_MINUTES = MAX_RUNNING_TIMEOUT_SECONDS / 60

/**
 * 本模块可选的格子上限档位（主窗口「窗口格子」滑块用）：单模块最多 3×3 = 9。
 * ⚠️ 整窗还能因同级组件叠加而长到更大（见 `@/lib/floating-layout` 的 `CELL_STEPS`，到 4×4 = 16），
 * 但那些更大档位是给「一个模块吃满 3×3 再叠加别的组件」的整窗预留的，不暴露给本模块的滑块。
 */
export const AGENT_MAX_CELL_STEPS: readonly number[] = [1, 2, 3, 4, 6, 9]
/** 本模块格子上限的最大值：3×3 = 9。 */
export const MAX_AGENT_CELLS = 9

/**
 * 转灯日志的档位，与 Rust `LogLevel` 的 serde 取值一一对应（`log/config.rs`）。
 * 档位是「包含」关系：verbose ⊇ normal ⊇ minimal；`off` = 一行都不写。
 *
 * ⚠️ 界面上**没有**这四档的选择器：设置页只有一个 switch（见 `LOG_ON` / `LOG_OFF`），
 * 这四档留着是因为 Rust 的配置块与旧配置仍按 `level` 解释。
 */
export type LogLevel = 'off' | 'minimal' | 'normal' | 'verbose'

/** 日志配置块，字段与 Rust `LogConfig` 同名同义（时间单位毫秒、字节单位字节）。 */
export interface LogSettings {
  /** 记录到多细；`off` = 完全不记。 */
  level: LogLevel
  /** hook 侧（每次平台调用都要开一次进程）是否也写自己那一行。 */
  hook_enabled: boolean
  /** 日志文件留多少天（1–90）。 */
  retention_days: number
  /** 单个文件写到多大就换（64 KiB–64 MiB）。 */
  max_file_bytes: number
  /** 整个日志目录的体积上限（1–512 MiB）。 */
  max_total_bytes: number
  /** 每轮增量读活动文件最多读多少字节（64 KiB–4 MiB）。 */
  live_tail_bytes: number
}

const KIB = 1024
const MIB = 1024 * KIB

/**
 * 日志保留天数的夹取边界，与 Rust `log/config.rs` 的 `MIN/MAX_RETENTION_DAYS` 同值。
 * 它是界面上**唯一**的回收旋钮（下拉只有 `LOG_RETENTION_DAYS_CHOICES` 那两档）：体积上限
 * （`max_total_bytes`）刻意不给入口 —— 两个旋钮会让用户分不清「留多久」与「占多少」谁说了算；
 * 容量那条线只在量特别大时兜底，并靠设置页的占用读数把它是否动过手写出来（见 `logUsageLabel`）。
 *
 * ⚠️ 上限 14 是产品决定：这套账是排障工具，两周之外的裁决已经没有价值，
 * 再长只是让盘上多躺着没人翻的提问摘录。`MIN` 仍留 1 是给手改配置的人 —— 那种意图是「少留」，
 * 夹到 1 天比拒绝或回默认更贴近它。
 */
export const MIN_LOG_RETENTION_DAYS = 1
export const MAX_LOG_RETENTION_DAYS = 14

/** 下拉里可选的保留天数（界面上只有这两档）。 */
export const LOG_RETENTION_DAYS_CHOICES = [7, 14] as const

/**
 * 缺 `log` 块时的默认值：**与 Rust `LogConfig::default()` 必须逐字段一致**。
 *
 * 默认关（`level: 'off'`）：设置页那个 switch 的语义是「关 = 完全不记，开 = 明细级 + hook 同记」，
 * 默认值若是 normal，switch 读数与真实档位就对不上；且明细级会落提问摘录，
 * 不能让「没人碰过开关」的机器上就出现用户文本。
 */
export const DEFAULT_LOG_SETTINGS: LogSettings = {
  level: 'off',
  hook_enabled: false,
  // 默认留 7 天：这套账是主动排障工具，两周的量在明细级下没必要常驻磁盘。
  // ⚠️ 与 Rust `DEFAULT_RETENTION_DAYS` 必须同值，`settings.spec.ts` 用 ?raw 跨语言锁住它。
  retention_days: 7,
  max_file_bytes: 8 * MIB,
  max_total_bytes: 96 * MIB,
  live_tail_bytes: 256 * KIB,
}

/**
 * switch「打开」写入的那一对值：明细级 + hook 侧同记。
 *
 * ⚠️ 必须两个字段一起写：只写 level 会留下 `hook_enabled: false`，hook 那一侧（含
 * 「适配器静默忽略」那些行）就是瞎的 —— 而那正是三家平台最容易掉灯的地方。
 */
export const LOG_SWITCH_ON: Pick<LogSettings, 'level' | 'hook_enabled'> = {
  level: 'verbose',
  hook_enabled: true,
}

/** switch「关闭」写入的那一对值。 */
export const LOG_SWITCH_OFF: Pick<LogSettings, 'level' | 'hook_enabled'> = {
  level: 'off',
  hook_enabled: false,
}

/** 档位认得出来才用 given，否则回落 Rust 的 `FALLBACK_LEVEL`（裁决级，不是 off）。 */
const LOG_LEVELS: readonly LogLevel[] = ['off', 'minimal', 'normal', 'verbose']

/** 把任意输入读成一个档位；写错的值按「有人想记但写错了」处理，落到裁决级。 */
function readLevel(value: unknown, blockPresent: boolean): LogLevel {
  if (typeof value === 'string' && (LOG_LEVELS as readonly string[]).includes(value.trim())) {
    return value.trim() as LogLevel
  }
  // 没有 log 块 = 没人打开过开关 → 默认档；有块但值不对 → 回落档（见 config.rs 同处注释）。
  return blockPresent ? 'normal' : DEFAULT_LOG_SETTINGS.level
}

/** 整数逐字段取值 + 夹取，范围与 Rust 的 MIN/MAX 常量对齐（坏一个字段只影响它自己）。 */
function readClampedInt(value: unknown, fallback: number, min: number, max: number): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return fallback
  return Math.min(max, Math.max(min, Math.round(value)))
}

/**
 * 从本组件的配置块里解析日志设置。
 *
 * ⚠️ 入参是**整块** `module_settings['agent-status']`（日志字段在它的 `log` 子块里）：
 * 少包一层会把整块读成默认值，两边口径就悄悄分家了。
 */
export function logSettings(raw: Record<string, unknown> | undefined): LogSettings {
  const block = raw?.log
  const present = !!block && typeof block === 'object'
  const fields = (block ?? {}) as Record<string, unknown>
  return {
    level: readLevel(fields.level, present),
    hook_enabled: typeof fields.hook_enabled === 'boolean' ? fields.hook_enabled : DEFAULT_LOG_SETTINGS.hook_enabled,
    retention_days: readClampedInt(
      fields.retention_days,
      DEFAULT_LOG_SETTINGS.retention_days,
      MIN_LOG_RETENTION_DAYS,
      MAX_LOG_RETENTION_DAYS
    ),
    max_file_bytes: readClampedInt(fields.max_file_bytes, DEFAULT_LOG_SETTINGS.max_file_bytes, 64 * KIB, 64 * MIB),
    max_total_bytes: readClampedInt(fields.max_total_bytes, DEFAULT_LOG_SETTINGS.max_total_bytes, MIB, 512 * MIB),
    live_tail_bytes: readClampedInt(fields.live_tail_bytes, DEFAULT_LOG_SETTINGS.live_tail_bytes, 64 * KIB, 4 * MIB),
  }
}

/** switch 的读数：只要档位不是 `off` 就算「打开」（历史脏值也一样，确实有东西在记）。 */
export function logSwitchOn(settings: LogSettings): boolean {
  return settings.level !== 'off'
}

/**
 * 按 switch 的新状态产出**整块**日志设置（未暴露的字段原样保留）。
 *
 * ⚠️ 落盘走 `updateModuleSettings`，它是模块层浅合并：只传 `{ log: { level } }` 会把
 * `retention_days` 等字段整块丢掉，所以这里必须返回完整一块。
 */
export function logBlockWithSwitch(current: LogSettings, on: boolean): LogSettings {
  return { ...current, ...(on ? LOG_SWITCH_ON : LOG_SWITCH_OFF) }
}

/** 本组件自己解释的配置（时间单位与后端一致，都是毫秒）。 */
export interface AgentStatusSettings {
  /** 一轮执行完成后绿灯保留多久（毫秒）。 */
  completed_retention_ms: number
  /** Running 阶段无新事件时的超时（毫秒）；超时后移除绿灯。 */
  running_timeout_ms: number
  /** token 展示单位；主窗口可切换，悬浮窗提示行按此格式化。 */
  token_unit: TokenUnit
  /** 选中的代理解析器 id；`none` = 不解析。 */
  proxy_resolver: string
  /**
   * 本模块最多长到几格（主窗口「窗口格子」滑块选的档位 {1,2,3,4,6,9}，封顶 3×3 = 9）。
   * ⚠️ 只限制**本模块**的需求上限，不影响其它组件 —— 窗口仍按各组件需求之和（可超过 9）自动伸缩。
   */
  max_cells: number
  /**
   * 状态音效开关：红灯 / 黄灯 / 完成绿闪时各播一段提示音。
   * 音效由悬浮窗前端播放（`lib/sound-player.ts`），Rust 侧不解释本字段。
   */
  sound_enabled: boolean
  /**
   * 转灯日志的配置块（`module_settings['agent-status'].log`）。
   *
   * ⚠️ 界面上只有**一个 switch** 管它（关 = 完全不记；开 = 明细级 + hook 侧同记），
   * 其余字段是给 Rust 运行态用的容量/保留期上限，不暴露给用户 —— 但写回时必须整块带上，
   * 否则浅合并会把它们抹掉（见 `logBlockWithSwitch`）。
   */
  log: LogSettings
}

/** 缺配置时的默认值（与 Rust `AgentStatusSettings::default` 一致）。 */
export const DEFAULT_AGENT_STATUS_SETTINGS: AgentStatusSettings = {
  completed_retention_ms: DEFAULT_RETENTION_SECONDS * 1000,
  running_timeout_ms: DEFAULT_RUNNING_TIMEOUT_SECONDS * 1000,
  token_unit: DEFAULT_TOKEN_UNIT,
  proxy_resolver: NONE_PROXY_RESOLVER,
  max_cells: MAX_AGENT_CELLS,
  // 默认开：音效是「不看屏幕也能知道出事了」的那一半价值，默认关会没人发现它存在。
  sound_enabled: true,
  // 默认关：转灯日志是主动排障工具，不是常开黑匣子（明细级会落提问摘录）。
  log: DEFAULT_LOG_SETTINGS,
}

/** 把任意输入夹进 [min, max] 的整数毫秒；非有限值回落 `fallback`。 */
function clampMs(value: unknown, min: number, max: number, fallback: number): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return fallback
  return Math.min(max, Math.max(min, Math.round(value)))
}

/** 把任意输入夹进 [min, max] 的整数；非有限值回落 `fallback`（格子数这类通用整数用）。 */
function clampInt(value: unknown, min: number, max: number, fallback: number): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return fallback
  return Math.min(max, Math.max(min, Math.round(value)))
}

/**
 * 从外壳配置里解析本组件的配置块。
 *
 * 逐字段宽松取值、并在这里夹取范围（与 Rust 侧 `AgentStatusSettings::from_block` 同一口径）：
 * 某一个字段写错只影响它自己，不会让整块配置或整份配置失效。
 */
export function agentStatusSettings(config: FloatingModuleConfig | null): AgentStatusSettings {
  const raw = (config?.module_settings?.[AGENT_STATUS_MODULE_ID] ?? {}) as Record<string, unknown>
  const defaults = DEFAULT_AGENT_STATUS_SETTINGS
  const proxyResolver = typeof raw.proxy_resolver === 'string' ? raw.proxy_resolver.trim() : ''
  return {
    completed_retention_ms: clampMs(
      raw.completed_retention_ms,
      MIN_RETENTION_SECONDS * 1000,
      MAX_RETENTION_SECONDS * 1000,
      defaults.completed_retention_ms
    ),
    running_timeout_ms: clampMs(
      raw.running_timeout_ms,
      MIN_RUNNING_TIMEOUT_SECONDS * 1000,
      MAX_RUNNING_TIMEOUT_SECONDS * 1000,
      defaults.running_timeout_ms
    ),
    token_unit: raw.token_unit === 'm_tokens' ? 'm_tokens' : DEFAULT_TOKEN_UNIT,
    proxy_resolver: proxyResolver || NONE_PROXY_RESOLVER,
    max_cells: clampInt(raw.max_cells, ABS_MIN_CELLS, MAX_AGENT_CELLS, defaults.max_cells),
    // 布尔字段只认真正的布尔值：写错类型（如字符串 "false"）回落默认，不做隐式转换。
    sound_enabled: typeof raw.sound_enabled === 'boolean' ? raw.sound_enabled : defaults.sound_enabled,
    // 日志整块交给 `logSettings` 解析：它有「有没有 log 块」这个判断，默认档因此能分清
    // 「没人打开过开关」(off) 与「写了但值不对」(normal)，不能并进这里的通用夹取。
    log: logSettings(raw),
  }
}

/** 设置页输入（秒）→ 配置值（毫秒），顺便夹取范围。 */
export function retentionMsFromSeconds(seconds: number): number {
  return clampMs(
    seconds * 1000,
    MIN_RETENTION_SECONDS * 1000,
    MAX_RETENTION_SECONDS * 1000,
    DEFAULT_AGENT_STATUS_SETTINGS.completed_retention_ms
  )
}

/** 设置页输入（分钟）→ 配置值（毫秒），顺便夹取范围。 */
export function runningTimeoutMsFromMinutes(minutes: number): number {
  return clampMs(
    minutes * 60_000,
    MIN_RUNNING_TIMEOUT_SECONDS * 1000,
    MAX_RUNNING_TIMEOUT_SECONDS * 1000,
    DEFAULT_AGENT_STATUS_SETTINGS.running_timeout_ms
  )
}

/**
 * 任意值 → 下拉里真能选中的那一档（先夹进 `MIN–MAX`，再取最近的选择）。
 *
 * 为什么必须就近取而不是原样显示：旧版是数字输入框，盘上可能留着 1–90 之间的任何数，
 * 而下拉只有两档 —— 界面显示一个选不出来的数，就是「面板说 14 天、后端按 3 天删」。
 * 取完最近一档后调用方要把它写回去（见设置卡的回写）。
 */
export function nearestRetentionChoice(value: unknown, fallback: number): number {
  const days = readClampedInt(value, fallback, MIN_LOG_RETENTION_DAYS, MAX_LOG_RETENTION_DAYS)
  return LOG_RETENTION_DAYS_CHOICES.reduce(
    (best, choice) => (Math.abs(choice - days) < Math.abs(best - days) ? choice : best),
    LOG_RETENTION_DAYS_CHOICES[0]
  )
}

/** 配置值（毫秒）→ 设置页显示的秒数。 */
export function toSeconds(ms: number): number {
  return Math.round(ms / 1000)
}

/** 配置值（毫秒）→ 「无响应超时」输入框显示的分钟数。 */
export function toMinutes(ms: number): number {
  return Math.round(ms / 60_000)
}

/** 向后端拉取可用的代理解析器列表（含 `none`）；由设置页自己按需调用。 */
export function fetchProxyResolvers(): Promise<ProxyResolverInfo[]> {
  return invoke<ProxyResolverInfo[]>('get_proxy_resolvers')
}
