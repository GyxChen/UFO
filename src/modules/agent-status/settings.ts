/**
 * agent-status 组件自己的配置：字段名、默认值、取值范围都只在这里解释。
 *
 * 悬浮窗外壳（`@/lib/floating-modules` + `@/stores/floatingModules`）只按 id 存一块 JSON，
 * 不认识里面任何字段；因此给本组件加一个配置项只需要改本文件 + 设置页，
 * 既不用动外壳也不用迁移配置（读不到键就回落这里的默认值）。
 * Rust 侧的对应实现是 `src-tauri/src/modules/agent_status/settings.rs`，两边口径必须一致。
 */
import { invoke } from '@tauri-apps/api/core'
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

/** 代理解析器下拉项：id 存进配置并作 value，label 供 UI 展示。与 Rust `ProxyResolverInfo` 对应。 */
export interface ProxyResolverInfo {
  id: string
  label: string
}

/** 完成灯保留时长：默认 30 秒，可选 1 秒 ..= 24 小时。 */
export const DEFAULT_RETENTION_SECONDS = 30
export const MIN_RETENTION_SECONDS = 1
export const MAX_RETENTION_SECONDS = 86_400

/** Running 无事件超时：默认 60 秒，可选 1 分钟 ..= 60 分钟（慢 agent 可拉长）。 */
export const DEFAULT_RUNNING_TIMEOUT_SECONDS = 60
export const MIN_RUNNING_TIMEOUT_SECONDS = 60
export const MAX_RUNNING_TIMEOUT_SECONDS = 60 * 60

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
}

/** 缺配置时的默认值（与 Rust `AgentStatusSettings::default` 一致）。 */
export const DEFAULT_AGENT_STATUS_SETTINGS: AgentStatusSettings = {
  completed_retention_ms: DEFAULT_RETENTION_SECONDS * 1000,
  running_timeout_ms: DEFAULT_RUNNING_TIMEOUT_SECONDS * 1000,
  token_unit: DEFAULT_TOKEN_UNIT,
  proxy_resolver: NONE_PROXY_RESOLVER,
}

/** 把任意输入夹进 [min, max] 的整数毫秒；非有限值回落 `fallback`。 */
function clampMs(value: unknown, min: number, max: number, fallback: number): number {
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

/** 设置页输入（秒）→ 配置值（毫秒），顺便夹取范围。 */
export function runningTimeoutMsFromSeconds(seconds: number): number {
  return clampMs(
    seconds * 1000,
    MIN_RUNNING_TIMEOUT_SECONDS * 1000,
    MAX_RUNNING_TIMEOUT_SECONDS * 1000,
    DEFAULT_AGENT_STATUS_SETTINGS.running_timeout_ms
  )
}

/** 配置值（毫秒）→ 设置页显示的秒数。 */
export function toSeconds(ms: number): number {
  return Math.round(ms / 1000)
}

/** 向后端拉取可用的代理解析器列表（含 `none`）；由设置页自己按需调用。 */
export function fetchProxyResolvers(): Promise<ProxyResolverInfo[]> {
  return invoke<ProxyResolverInfo[]>('get_proxy_resolvers')
}
