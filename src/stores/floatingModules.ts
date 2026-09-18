import { invoke } from '@tauri-apps/api/core'
import { emitTo, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { defineStore } from 'pinia'
import { ABS_MAX_CELLS, ABS_MIN_CELLS, clampCellPx } from '@/lib/floating-layout'
import {
  applyModuleOrder,
  configCellPx,
  defaultMaxCellsByLayout,
  isModuleEnabled,
  type FloatingLayout,
  type FloatingModuleConfig,
  type FloatingRuntimeSnapshot,
  type MaxCellsByLayout,
} from '@/lib/floating-modules'
import {
  FLOATING_RUNTIME_EVENT,
  FLOATING_RUNTIME_REQUEST_EVENT,
  type FloatingRuntimePayload,
} from '@/lib/floating-bridge'

const MODULE_CONFIG_EVENT = 'floating-module-config'
let stopListener: UnlistenFn | undefined
let stopRuntimeListener: UnlistenFn | undefined

const fallbackConfig: FloatingModuleConfig = {
  schema_version: 1,
  revision: 0,
  // 兜底配置不列任何模块：启用状态一律回落到定义里的 defaultEnabled，
  // 外壳不点名具体模块（后端配置到达后立刻被 applyConfig 覆盖）。
  modules: [],
  max_cells_by_layout: defaultMaxCellsByLayout(),
  // 也不预置任何组件配置块：组件读不到自己的键就回落自己的默认值。
  module_settings: {},
}

function clampLayoutMax(value: number): number {
  return Math.min(Math.max(Math.floor(value), ABS_MIN_CELLS), ABS_MAX_CELLS)
}

export const useFloatingModulesStore = defineStore('floating-modules', {
  state: () => ({
    config: fallbackConfig as FloatingModuleConfig | null,
    connection: 'starting' as 'starting' | 'connected' | 'degraded',
    started: false,

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
    maxCellsByLayout: (state): MaxCellsByLayout => state.config?.max_cells_by_layout ?? defaultMaxCellsByLayout(),
    /** 配置里的一格边长（物理像素）；没配过回落默认 45（见 `configCellPx`）。 */
    cellPx: (state): number => configCellPx(state.config),
    /**
     * 取某个组件自己的配置块（原样返回，不解释内容）；没配置过时返回空对象。
     * 组件自己的 `settings.ts` 负责把它解析成强类型值 —— 外壳不知道里面有什么。
     */
    moduleSettings:
      state =>
      (id: string): Record<string, unknown> =>
        state.config?.module_settings?.[id] ?? {},

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

    /**
     * 更新某个组件自己的配置块（浅合并；组件负责传它认识的字段与合法取值）。
     * 外壳只做「贴到 module_settings[id] 上再落盘」，不解释里面任何内容 ——
     * 因此新增组件的配置项不需要改这里。
     */
    async updateModuleSettings(id: string, patch: Record<string, unknown>) {
      const current = this.config ?? fallbackConfig
      const next = {
        ...current.module_settings,
        [id]: { ...((current.module_settings?.[id] as Record<string, unknown> | undefined) ?? {}), ...patch },
      }
      try {
        this.applyConfig(
          await invoke<FloatingModuleConfig>('set_floating_module_config', {
            config: { ...current, module_settings: next },
          })
        )
      } catch (error) {
        console.warn(`failed to update module settings for ${id}`, error)
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

    /**
     * 改一格边长（物理像素）。
     *
     * 夹取范围后整份落盘：这是**窗口级**参数，悬浮窗收到配置事件后会按新尺寸重新吸附，
     * 因此这里不需要通知任何具体组件。
     */
    async updateCellPx(px: number) {
      const current = this.config ?? fallbackConfig
      try {
        this.applyConfig(
          await invoke<FloatingModuleConfig>('set_floating_module_config', {
            config: { ...current, cell_px: clampCellPx(px) },
          })
        )
      } catch (error) {
        console.warn('failed to update floating cell size', error)
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
