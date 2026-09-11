import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { defineStore } from 'pinia'
import { ABS_MAX_CELLS, ABS_MIN_CELLS } from '@/lib/agent-status-layout'
import {
  defaultMaxCellsByLayout,
  isModuleEnabled,
  type FloatingLayout,
  type FloatingModuleConfig,
  type MaxCellsByLayout,
} from '@/lib/floating-modules'

const MODULE_CONFIG_EVENT = 'floating-module-config'
let stopListener: UnlistenFn | undefined

/** Running 无事件超时默认 60 秒。 */
const DEFAULT_RUNNING_TIMEOUT_SECONDS = 60
/** Running 超时可选范围：1 分钟 ..= 60 分钟。 */
const MIN_RUNNING_TIMEOUT_SECONDS = 60
const MAX_RUNNING_TIMEOUT_SECONDS = 60 * 60

const fallbackConfig: FloatingModuleConfig = {
  schema_version: 1,
  revision: 0,
  modules: [{ id: 'agent-status', enabled: true, order: 0 }],
  completed_retention_ms: 30_000,
  running_timeout_ms: DEFAULT_RUNNING_TIMEOUT_SECONDS * 1000,
  max_cells_by_layout: defaultMaxCellsByLayout(),
}

function clampLayoutMax(value: number): number {
  return Math.min(Math.max(Math.floor(value), ABS_MIN_CELLS), ABS_MAX_CELLS)
}

export const useFloatingModulesStore = defineStore('floating-modules', {
  state: () => ({
    config: fallbackConfig as FloatingModuleConfig | null,
    connection: 'starting' as 'starting' | 'connected' | 'degraded',
    started: false,
  }),

  getters: {
    agentStatusEnabled: (state) => isModuleEnabled(state.config, 'agent-status'),
    retentionSeconds: (state) => Math.round((state.config?.completed_retention_ms ?? 30_000) / 1000),
    /** Running 无新 hook 超时（秒），默认 60。 */
    runningTimeoutSeconds: (state) =>
      Math.round(
        (state.config?.running_timeout_ms ?? DEFAULT_RUNNING_TIMEOUT_SECONDS * 1000) / 1000,
      ),
    maxCellsByLayout: (state): MaxCellsByLayout =>
      state.config?.max_cells_by_layout ?? defaultMaxCellsByLayout(),
  },

  actions: {
    maxCellsFor(layout: FloatingLayout): number {
      return this.maxCellsByLayout[layout] ?? ABS_MAX_CELLS
    },

    applyConfig(config: FloatingModuleConfig) {
      if (this.config && config.revision < this.config.revision) return
      this.config = config
      this.connection = 'connected'
    },

    async start() {
      if (this.started) return
      this.started = true
      try {
        stopListener = await listen<FloatingModuleConfig>(MODULE_CONFIG_EVENT, (event) => {
          this.applyConfig(event.payload)
        })
        this.applyConfig(await invoke<FloatingModuleConfig>('get_floating_module_config'))
      } catch (error) {
        this.config = fallbackConfig
        this.connection = 'degraded'
        console.warn('floating module configuration unavailable', error)
      }
    },

    async updateAgentStatus(enabled: boolean) {
      const current = this.config ?? fallbackConfig
      const modules = current.modules.some((module) => module.id === 'agent-status')
        ? current.modules.map((module) => (module.id === 'agent-status' ? { ...module, enabled } : module))
        : [...current.modules, { id: 'agent-status', enabled, order: current.modules.length }]
      try {
        this.applyConfig(await invoke<FloatingModuleConfig>('set_floating_module_config', {
          config: { ...current, modules },
        }))
      } catch (error) {
        console.warn('failed to update agent status module', error)
        throw error
      }
    },

    async updateRetentionSeconds(seconds: number) {
      const current = this.config ?? fallbackConfig
      const bounded = Math.min(86_400, Math.max(1, Math.round(seconds)))
      try {
        this.applyConfig(await invoke<FloatingModuleConfig>('set_floating_module_config', {
          config: { ...current, completed_retention_ms: bounded * 1000 },
        }))
      } catch (error) {
        console.warn('failed to update completed retention', error)
        throw error
      }
    },

    /** 更新 Running 无新 hook 超时（秒）；慢 agent 可拉长避免绿灯被误清。 */
    async updateRunningTimeoutSeconds(seconds: number) {
      const current = this.config ?? fallbackConfig
      const bounded = Math.min(
        MAX_RUNNING_TIMEOUT_SECONDS,
        Math.max(MIN_RUNNING_TIMEOUT_SECONDS, Math.round(seconds)),
      )
      try {
        this.applyConfig(
          await invoke<FloatingModuleConfig>('set_floating_module_config', {
            config: { ...current, running_timeout_ms: bounded * 1000 },
          }),
        )
      } catch (error) {
        console.warn('failed to update running timeout', error)
        throw error
      }
    },

    async updateMaxCellsByLayout(partial: Partial<MaxCellsByLayout>) {
      const current = this.config ?? fallbackConfig
      const next: MaxCellsByLayout = {
        ...current.max_cells_by_layout,
        ...Object.fromEntries(
          Object.entries(partial).map(([key, value]) => [key, clampLayoutMax(value as number)]),
        ),
      } as MaxCellsByLayout
      try {
        this.applyConfig(await invoke<FloatingModuleConfig>('set_floating_module_config', {
          config: { ...current, max_cells_by_layout: next },
        }))
      } catch (error) {
        console.warn('failed to update max cells by layout', error)
        throw error
      }
    },

    stop() {
      stopListener?.()
      stopListener = undefined
      this.started = false
    },
  },
})
