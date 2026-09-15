import type { Component } from 'vue'
import { invoke } from '@tauri-apps/api/core'

export type FloatingLayout = 'square' | 'corner' | 'horizontal' | 'vertical'
export type FloatingModuleId = 'agent-status' | (string & {})

export interface FloatingModuleDefinition {
  id: FloatingModuleId
  label: string
  version: number
  defaultEnabled: boolean
  supportedLayouts: FloatingLayout[]
  component: Component
}

export interface FloatingModuleConfigEntry {
  id: string
  enabled: boolean
  order: number
}

/** 各贴边形态可显示的最大格子数（4..=8）。 */
export interface MaxCellsByLayout {
  square: number
  corner: number
  horizontal: number
  vertical: number
}

/**
 * token 展示单位，与 Rust 侧 `TokenUnit` 的序列化值一一对应。
 * - `tokens`：原始计数，如 `1,261,847`
 * - `m_tokens`：以百万为单位，如 `1.26M`
 */
export type TokenUnit = 'tokens' | 'm_tokens'

/** token 单位默认值：保持原始计数，与加入该设置之前的行为一致。 */
export const DEFAULT_TOKEN_UNIT: TokenUnit = 'tokens'

/** 代理解析器下拉项：id 存进配置并作 value，label 供 UI 展示。与 Rust `ProxyResolverInfo` 对应。 */
export interface ProxyResolverInfo {
  id: string
  label: string
}

/** 不解析（直连）的固定 id，与 Rust `NONE_RESOLVER` 一致。 */
export const NONE_PROXY_RESOLVER = 'none'

export interface FloatingModuleConfig {
  schema_version: number
  revision: number
  modules: FloatingModuleConfigEntry[]
  completed_retention_ms: number
  /** Running 无新 hook 时的超时（毫秒）；超时后移除绿灯。 */
  running_timeout_ms: number
  max_cells_by_layout: MaxCellsByLayout
  /** token 展示单位；主窗口可切换。 */
  token_unit: TokenUnit
  /** 选中的代理解析器 id；`none` = 不解析。 */
  proxy_resolver: string
}

export function defaultMaxCellsByLayout(value: number = 8): MaxCellsByLayout {
  // 与 ABS_MIN_CELLS / ABS_MAX_CELLS 对齐，避免与 agent-status-layout 循环依赖。
  const cells = Math.min(Math.max(Math.floor(value), 4), 8)
  return {
    square: cells,
    corner: cells,
    horizontal: cells,
    vertical: cells,
  }
}

export function createFloatingModuleDefinitions(agentStatusComponent: Component): FloatingModuleDefinition[] {
  return [
    {
      id: 'agent-status',
      label: 'Agent 状态',
      version: 1,
      defaultEnabled: true,
      supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
      component: agentStatusComponent,
    },
  ]
}

export function isModuleEnabled(config: FloatingModuleConfig | null, id: string): boolean {
  return config?.modules.find((module) => module.id === id)?.enabled ?? false
}

/** 向后端拉取可用的代理解析器列表（含 `none`）。 */
export function fetchProxyResolvers(): Promise<ProxyResolverInfo[]> {
  return invoke<ProxyResolverInfo[]>('get_proxy_resolvers')
}

export function enabledModuleDefinitions(
  config: FloatingModuleConfig | null,
  definitions: FloatingModuleDefinition[],
): FloatingModuleDefinition[] {
  return definitions
    .filter((definition) => isModuleEnabled(config, definition.id))
    .sort((left, right) => {
      const leftOrder = config?.modules.find((module) => module.id === left.id)?.order ?? Number.MAX_SAFE_INTEGER
      const rightOrder = config?.modules.find((module) => module.id === right.id)?.order ?? Number.MAX_SAFE_INTEGER
      return leftOrder - rightOrder || left.id.localeCompare(right.id)
    })
}
