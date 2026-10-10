import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { defineStore } from 'pinia'
import { PLUGINS_CHANGED_EVENT } from '@/lib/plugin-events'
import { retryOnStartupRace } from '@/lib/startup-retry'
import { declaredSurfaceIds, type CapabilityFactJson, type LedgerBroadcastJson, type PluginRecordJson } from '@/lib/plugin-types'

let stopListener: UnlistenFn | undefined

/**
 * 读一次安装记录。
 *
 * 首帧 IPC 可能早于后端 `setup` 里的 `.manage()`（与悬浮窗配置同一个竞态），
 * 一次失败就退化成「一个包都没有」的话，悬浮窗会白闪一下，所以走统一的重试助手。
 */
async function fetchLedger(): Promise<LedgerBroadcastJson> {
  return retryOnStartupRace(() => invoke<LedgerBroadcastJson>('get_plugins'))
}

/** 读一次能力注册表（徽标与披露的语义来源）。 */
async function fetchRegistry(): Promise<CapabilityFactJson[]> {
  return retryOnStartupRace(() => invoke<CapabilityFactJson[]>('get_plugin_registry'))
}

/**
 * 插件安装记录的 store：三个窗口都读它，只有主窗（和它派出的面板窗）写它。
 *
 * ⚠️ 这里**不认识任何具体插件**：一切从安装记录与注册表来。
 * 写命令失败一律变成 `actionError`（界面走行内 `role="status"`，`window.alert` 被插件拦掉了）。
 */
export const usePluginsStore = defineStore('plugins', {
  state: () => ({
    /** 已应用安装记录的版本号（比它小的广播直接丢）。 */
    revision: 0,
    /** 全量记录，含校验失败的包。 */
    records: [] as PluginRecordJson[],
    /** 能力与挂载点的注册表。 */
    registry: [] as CapabilityFactJson[],
    connection: 'starting' as 'starting' | 'connected' | 'degraded',
    started: false,
    /** 管理区右栏当前选中的包。 */
    selectedId: '',
    /** 最近一次写命令的失败原因（行内提示用）。 */
    actionError: '',
  }),

  getters: {
    /** 此刻该挂载进窗口的记录：已启用且清单校验通过。 */
    mountedRecords: state => state.records.filter(record => record.manifest != null),
    /** 按 id 取记录；没装上就是 null。 */
    recordOf: state => (id: string): PluginRecordJson | null =>
      state.records.find(record => record.id === id) ?? null,
    /** 正在调试（热重载开着）的包数 —— 卡头那枚角标的读数。 */
    debugCount: state => state.records.filter(record => record.devReload).length,
    /** 按 id 查一条能力事实（挂载点名 / 权限名 → 徽标语义）。 */
    factOf: state => (id: string): CapabilityFactJson | undefined =>
      state.registry.find(fact => fact.id === id),
    /**
     * 某个包该显示的徽标：挂载点在前、能力在后（与 Rust 表序一致），查不到的名字跳过。
     * 查不到不是错误 —— 安装记录里的包可能来自更新的宿主，界面不能因此整块空白。
     */
    badgesOf: state => (record: PluginRecordJson | null): CapabilityFactJson[] => {
      if (!record?.manifest) return []
      const ids = [
        ...declaredSurfaceIds(record.manifest),
        ...record.manifest.permissions,
      ]
      return ids
        .map(id => state.registry.find(fact => fact.id === id))
        .filter((fact): fact is CapabilityFactJson => fact != null)
    },
    /** 所有已装包的体积之和（管理区标题行的常驻读数）。 */
    totalBytes: state => state.records.reduce((sum, record) => sum + record.bytes, 0),
  },

  actions: {
    /** 应用一次广播：版本比当前小就丢掉（迟到的那一份不该盖回新状态）。 */
    applyBroadcast(broadcast: LedgerBroadcastJson) {
      if (broadcast.revision < this.revision) return
      this.revision = broadcast.revision
      this.records = broadcast.records
      this.connection = 'connected'
      // 选中的包被卸载掉时，右栏回到第一条，别停在一个已经不存在的 id 上。
      if (this.selectedId && !broadcast.records.some(record => record.id === this.selectedId)) {
        this.selectedId = broadcast.records[0]?.id ?? ''
      }
    },

    /** 选中某个包（右栏详情跟着换）。 */
    select(id: string) {
      this.selectedId = id
    },

    /** 拉安装记录 + 注册表并挂上广播。 */
    async start() {
      if (this.started) return
      this.started = true
      try {
        // 顺序要紧：先订阅再首拉，否则首拉与订阅之间发生的变更会永久丢。
        stopListener = await listen<LedgerBroadcastJson>(PLUGINS_CHANGED_EVENT, event => {
          this.applyBroadcast(event.payload)
        })
        const registry = await fetchRegistry()
        this.registry = registry
        this.applyBroadcast(await fetchLedger())
        if (!this.selectedId) this.selectedId = this.records[0]?.id ?? ''
      } catch (error) {
        this.connection = 'degraded'
        console.warn('plugin ledger unavailable', error)
      }
    },

    /** 丢弃某个包的记录（IPC 不可用时的退化形状：整块管理区空着而不是报错）。 */
    clear() {
      this.records = []
      this.selectedId = ''
    },

    /** 校验一个目录能不能装（不写盘）。失败原因落到 actionError 并抛给调用方。 */
    async preview(dir: string): Promise<PluginRecordJson | null> {
      try {
        this.actionError = ''
        return await invoke<PluginRecordJson>('preview_plugin', { dir })
      } catch (error) {
        this.actionError = asMessage(error)
        throw error
      }
    },

    /** 确认过后正式安装。 */
    async install(dir: string) {
      await this.write('install_plugin', { dir })
    },

    /** 卸载（界面上要先走两步确认）。 */
    async uninstall(id: string) {
      await this.write('uninstall_plugin', { id })
    },

    /** 逐包热重载开关。 */
    async setDevReload(id: string, devReload: boolean) {
      await this.write('set_plugin_dev_reload', { id, devReload })
    },

    /** 手动重扫盘上的包。 */
    async reload() {
      await this.write('reload_plugins', {})
    },

    /** 上报本次装载耗时（读数，不阻塞、不弹错）。 */
    reportLoad(id: string, loadMs: number) {
      void invoke('report_plugin_load_ms', { id, loadMs: Math.max(0, Math.round(loadMs)) }).catch(error => {
        console.warn('failed to report plugin load time', error)
      })
    },

    /** 写命令的公共尾巴：成功后应用返回的安装记录，失败就把中文原因留在行内提示里。 */
    async write(command: string, args: Record<string, unknown>) {
      this.actionError = ''
      try {
        this.applyBroadcast(await invoke<LedgerBroadcastJson>(command, args))
      } catch (error) {
        this.actionError = asMessage(error)
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

/** 把 invoke 的失败值变成一行可读中文（Rust 侧的 `Err(String)` 到这里通常已是字符串）。 */
function asMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  try {
    return JSON.stringify(error) ?? String(error)
  } catch {
    return String(error)
  }
}
