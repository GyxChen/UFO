import { invoke } from '@tauri-apps/api/core'
import { emitTo, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { defineStore } from 'pinia'
import { ABS_MAX_CELLS, ABS_MIN_CELLS } from '@/lib/floating-layout'
import {
  DEFAULT_TOKEN_UNIT,
  NONE_PROXY_RESOLVER,
  applyModuleOrder,
  defaultMaxCellsByLayout,
  fetchProxyResolvers,
  isModuleEnabled,
  type FloatingLayout,
  type FloatingModuleConfig,
  type FloatingRuntimeSnapshot,
  type MaxCellsByLayout,
  type ProxyResolverInfo,
  type TokenUnit,
} from '@/lib/floating-modules'
import {
  FLOATING_RUNTIME_EVENT,
  FLOATING_RUNTIME_REQUEST_EVENT,
  type FloatingRuntimePayload,
} from '@/lib/floating-bridge'

const MODULE_CONFIG_EVENT = 'floating-module-config'
let stopListener: UnlistenFn | undefined
let stopRuntimeListener: UnlistenFn | undefined

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
  token_unit: DEFAULT_TOKEN_UNIT,
  proxy_resolver: NONE_PROXY_RESOLVER,
}

function clampLayoutMax(value: number): number {
  return Math.min(Math.max(Math.floor(value), ABS_MIN_CELLS), ABS_MAX_CELLS)
}

export const useFloatingModulesStore = defineStore('floating-modules', {
  state: () => ({
    config: fallbackConfig as FloatingModuleConfig | null,
    connection: 'starting' as 'starting' | 'connected' | 'degraded',
    started: false,
    /** 后端注册的代理解析器选项（含 `none`）；主窗口下拉据此渲染。 */
    resolvers: [] as ProxyResolverInfo[],
    /** 悬浮窗广播来的运行时快照（各形态的格子需求与窗口大小）；主窗口预览据此渲染。 */
    runtime: null as FloatingRuntimeSnapshot | null,
  }),

  getters: {
    /**
     * 查询某个模块是否启用：配置里没有该模块时按定义里的默认值兜底，
     * 因此新增组件不必先迁移配置文件。做成传参函数便于按注册表批量渲染列表。
     */
    moduleEnabled:
      state =>
      (id: string, defaultEnabled = false): boolean =>
        isModuleEnabled(state.config, id, defaultEnabled),
    retentionSeconds: state => Math.round((state.config?.completed_retention_ms ?? 30_000) / 1000),
    /** 当前选中的代理解析器 id，默认 `none`（不解析）。 */
    proxyResolver: (state): string => state.config?.proxy_resolver ?? NONE_PROXY_RESOLVER,
    /** Running 无新 hook 超时（秒），默认 60。 */
    runningTimeoutSeconds: state =>
      Math.round((state.config?.running_timeout_ms ?? DEFAULT_RUNNING_TIMEOUT_SECONDS * 1000) / 1000),
    maxCellsByLayout: (state): MaxCellsByLayout => state.config?.max_cells_by_layout ?? defaultMaxCellsByLayout(),
    /** token 展示单位，默认原始计数。 */
    tokenUnit: (state): TokenUnit => state.config?.token_unit ?? DEFAULT_TOKEN_UNIT,

    /** 配置里的模块顺序（order 升序，order 相同按 id）；组件列表的排序与拖拽都用它。 */
    orderedModuleIds: (state): string[] =>
      [...(state.config?.modules ?? [])]
        .sort((left, right) => left.order - right.order || left.id.localeCompare(right.id))
        .map(module => module.id),
  },

  actions: {
    maxCellsFor(layout: FloatingLayout): number {
      return this.maxCellsByLayout[layout] ?? ABS_MAX_CELLS
    },

    /** 记录悬浮窗广播的运行时快照（主窗口预览 / 徽标用）。 */
    applyRuntime(snapshot: FloatingRuntimeSnapshot) {
      this.runtime = snapshot
    },

    /**
     * 主窗口专用：订阅悬浮窗广播的运行时快照，并主动要一次 ——
     * 悬浮窗是常驻的，主窗口挂载时可能已经错过它启动时那次广播。
     */
    async startRuntimeBridge() {
      try {
        stopRuntimeListener = await listen<FloatingRuntimePayload>(FLOATING_RUNTIME_EVENT, event => {
          this.applyRuntime(event.payload)
        })
        await emitTo('floating', FLOATING_RUNTIME_REQUEST_EVENT)
      } catch (error) {
        console.warn('floating runtime bridge unavailable', error)
      }
    },

    /** 停止运行时快照订阅（主窗口卸载时调用）。 */
    stopRuntimeBridge() {
      stopRuntimeListener?.()
      stopRuntimeListener = undefined
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
        stopListener = await listen<FloatingModuleConfig>(MODULE_CONFIG_EVENT, event => {
          this.applyConfig(event.payload)
        })
        this.applyConfig(await invoke<FloatingModuleConfig>('get_floating_module_config'))
        this.resolvers = await fetchProxyResolvers()
      } catch (error) {
        this.config = fallbackConfig
        this.connection = 'degraded'
        console.warn('floating module configuration unavailable', error)
      }
    },

    /**
     * 启用 / 停用某个悬浮窗模块。
     * 配置里还没有这个模块时（新模块首次出现）就地补一条，顺序排在最后。
     */
    async updateModuleEnabled(id: string, enabled: boolean) {
      const current = this.config ?? fallbackConfig
      const modules = current.modules.some(module => module.id === id)
        ? current.modules.map(module => (module.id === id ? { ...module, enabled } : module))
        : [...current.modules, { id, enabled, order: current.modules.length }]
      try {
        this.applyConfig(
          await invoke<FloatingModuleConfig>('set_floating_module_config', {
            config: { ...current, modules },
          })
        )
      } catch (error) {
        console.warn(`failed to update floating module ${id}`, error)
        throw error
      }
    },

    /**
     * 按给定 id 顺序重排模块（组件列表拖拽 / 键盘排序后调用）。
     * 只改 order 字段，启用状态与其它配置原样保留。
     */
    async updateModuleOrder(orderedIds: readonly string[]) {
      const current = this.config ?? fallbackConfig
      try {
        this.applyConfig(await invoke<FloatingModuleConfig>('set_floating_module_config', {
          config: { ...current, modules: applyModuleOrder(current.modules, orderedIds) },
        }))
      } catch (error) {
        console.warn('failed to update floating module order', error)
        throw error
      }
    },

    async updateRetentionSeconds(seconds: number) {
      const current = this.config ?? fallbackConfig
      const bounded = Math.min(86_400, Math.max(1, Math.round(seconds)))
      try {
        this.applyConfig(
          await invoke<FloatingModuleConfig>('set_floating_module_config', {
            config: { ...current, completed_retention_ms: bounded * 1000 },
          })
        )
      } catch (error) {
        console.warn('failed to update completed retention', error)
        throw error
      }
    },

    /** 更新 Running 无新 hook 超时（秒）；慢 agent 可拉长避免绿灯被误清。 */
    async updateRunningTimeoutSeconds(seconds: number) {
      const current = this.config ?? fallbackConfig
      const bounded = Math.min(MAX_RUNNING_TIMEOUT_SECONDS, Math.max(MIN_RUNNING_TIMEOUT_SECONDS, Math.round(seconds)))
      try {
        this.applyConfig(
          await invoke<FloatingModuleConfig>('set_floating_module_config', {
            config: { ...current, running_timeout_ms: bounded * 1000 },
          })
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
        ...Object.fromEntries(Object.entries(partial).map(([key, value]) => [key, clampLayoutMax(value as number)])),
      } as MaxCellsByLayout
      try {
        this.applyConfig(
          await invoke<FloatingModuleConfig>('set_floating_module_config', {
            config: { ...current, max_cells_by_layout: next },
          })
        )
      } catch (error) {
        console.warn('failed to update max cells by layout', error)
        throw error
      }
    },

    /** 更新 token 展示单位（tokens / M tokens）。 */
    async updateTokenUnit(unit: TokenUnit) {
      const current = this.config ?? fallbackConfig
      try {
        this.applyConfig(
          await invoke<FloatingModuleConfig>('set_floating_module_config', {
            config: { ...current, token_unit: unit },
          })
        )
      } catch (error) {
        console.warn('failed to update token unit', error)
        throw error
      }
    },

    /** 更新选中的代理解析器（下拉的 value=id）；后端会即时同步给状态监测。 */
    async updateProxyResolver(id: string) {
      const current = this.config ?? fallbackConfig
      try {
        this.applyConfig(
          await invoke<FloatingModuleConfig>('set_floating_module_config', {
            config: { ...current, proxy_resolver: id },
          })
        )
      } catch (error) {
        console.warn('failed to update proxy resolver', error)
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
