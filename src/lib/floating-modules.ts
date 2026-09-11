import type { Component } from 'vue'

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

export interface FloatingModuleConfig {
  schema_version: number
  revision: number
  modules: FloatingModuleConfigEntry[]
  completed_retention_ms: number
  /** Running 无新 hook 时的超时（毫秒）；超时后移除绿灯。 */
  running_timeout_ms: number
  max_cells_by_layout: MaxCellsByLayout
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
