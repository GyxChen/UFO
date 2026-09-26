/**
 * 消息推送的面板 store（主窗口「消息推送」分区的数据源）。
 *
 * 两条通道分工明确：
 * - **配置**写回外壳（`updateModuleSettings` → `set_floating_module_config`），后端配置钩子
 *   随即把新配置推给运行时（改完即时生效，无需重启）；
 * - **快照**由后端在每次投递后主动广播（`push-status`），面板只读。
 *
 * 每次写配置后都重新 `invoke` 一次快照：后端才是权威（它做过夹取与截断），前端自己算容易和它不一致。
 *
 * ⚠️ 不用 `window.alert` / `confirm`（装了 `tauri-plugin-prevent-default` 会被拦）：
 * 反馈一律走行内 `role="status"`，破坏性操作由面板做两步确认。
 */
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import {
  AGENT_STATUS_MODULE_ID,
} from '../settings'
import {
  isPushSnapshot,
  newChannelId,
  pushSettings,
  pushPatch,
  PUSH_STATUS_EVENT,
  type ChannelEntry,
  type PushConfig,
  type PushSnapshot,
  type PushTestOutcome,
} from '../lib/push'

/** 后端还没回话时的空快照（面板据此渲染「正在读取」而不是崩）。 */
function emptySnapshot(): PushSnapshot {
  return {
    config: pushSettings(null),
    providers: [],
    channels: [],
  }
}

export const usePushStore = defineStore('agent-push', {
  state: () => ({
    snapshot: emptySnapshot() as PushSnapshot,
    started: false,
    /** 首次拉取是否完成；用于区分「一个主题都没有」与「还没读到」。 */
    loaded: false,
    connection: 'starting' as 'starting' | 'connected' | 'degraded',
    /** 正在等后端回话的服务 id（统一测试期间按钮转圈并禁用）。 */
    pendingProvider: null as string | null,
  }),

  getters: {
    /** 权威配置：后端快照里的值优先（它做过夹取与清洗）。 */
    config: state => state.snapshot.config,
    providers: state => state.snapshot.providers,
    channels: state => state.snapshot.channels,
    /** 该服务是否有一次测试在飞。 */
    isBusy: state => (providerId: string) => state.pendingProvider === providerId,
  },

  actions: {
    /** 覆盖本地快照（每次广播都是全量，直接替换即可）。 */
    applySnapshot(snapshot: PushSnapshot) {
      this.snapshot = snapshot
      this.loaded = true
      this.connection = 'connected'
    },

    /**
     * 起订阅 + 拉首帧。幂等：反复调也只挂一个监听。
     *
     * 不退订是刻意的：本 store 只活在主窗口，面板的 `activate` 生命周期比订阅长，
     * 留着 unlisten 句柄反而没人调它。
     */
    async start() {
      if (this.started) return
      this.started = true
      try {
        await listen<PushSnapshot>(PUSH_STATUS_EVENT, event => {
          if (isPushSnapshot(event.payload)) this.applySnapshot(event.payload)
        })
        this.applySnapshot(await invoke<PushSnapshot>('get_push_status'))
      } catch (error) {
        // 读不到就等不到数据：标记 loaded，让界面走「降级」而不是永远转圈。
        this.connection = 'degraded'
        this.loaded = true
        console.warn('push channel unavailable', error)
      }
    },

    /** 重新拉一份快照（写完配置后用，保证与后端一致）。 */
    async refresh() {
      this.applySnapshot(await invoke<PushSnapshot>('get_push_status'))
    },

    /**
     * 把一份配置写回外壳，并刷新快照。
     *
     * 外壳的 `updateModuleSettings` 是**浅合并**，所以整块 `push` 一次给全，
     * 不要指望局部深合并。
     */
    async writeConfig(next: PushConfig) {
      const modules = useFloatingModulesStore()
      await modules.updateModuleSettings(AGENT_STATUS_MODULE_ID, pushPatch(next))
      await this.refresh()
    },

    /** 总开关。 */
    async setEnabled(enabled: boolean) {
      await this.writeConfig({ ...this.snapshot.config, enabled })
    },

    /** 勾/去勾一个事件（数组是「存在即权威」，全去掉就是空数组）。 */
    async toggleEvent(event: string, on: boolean) {
      const events = new Set(this.snapshot.config.events as string[])
      if (on) events.add(event)
      else events.delete(event)
      await this.writeConfig({
        ...this.snapshot.config,
        events: [...events] as PushConfig['events'],
      })
    },

    /** 勾 / 去勾一项提醒内容（agent 状态固定发，这里只管「还要带什么」）。 */
    async toggleContent(item: string, on: boolean) {
      const content = new Set(this.snapshot.config.content as string[])
      if (on) content.add(item)
      else content.delete(item)
      await this.writeConfig({
        ...this.snapshot.config,
        content: [...content] as PushConfig['content'],
      })
    },

    /** 服务级开关。 */
    async setProviderEnabled(providerId: string, enabled: boolean) {
      await this.writeConfig({
        ...this.snapshot.config,
        providers: this.providersWith(providerId, { enabled }),
      })
    },

    /** 服务器地址（该服务下所有主题共用）。 */
    async setProviderServer(providerId: string, server: string) {
      await this.writeConfig({
        ...this.snapshot.config,
        providers: this.providersWith(providerId, { server }),
      })
    },

    /** 改某个服务的配置项，其它服务原样保留。 */
    providersWith(providerId: string, patch: Partial<PushConfig['providers'][string]>) {
      const providers = { ...this.snapshot.config.providers }
      providers[providerId] = {
        ...(providers[providerId] ?? { enabled: false, server: '' }),
        ...patch,
      }
      return providers
    },

    /** 新建一行主题（名字留空由后端回落成服务名，主题串等用户填）。 */
    async addTopic(providerId: string) {
      const channel: ChannelEntry = {
        id: newChannelId(),
        provider: providerId,
        name: '',
        topic: '',
      }
      await this.writeConfig({
        ...this.snapshot.config,
        channels: [...this.snapshot.config.channels, channel],
      })
      return channel.id
    },

    /** 改一行主题的名字或主题串。 */
    async updateTopic(channelId: string, patch: Partial<Pick<ChannelEntry, 'name' | 'topic'>>) {
      const channels = this.snapshot.config.channels.map(channel =>
        channel.id === channelId ? { ...channel, ...patch } : channel
      )
      await this.writeConfig({ ...this.snapshot.config, channels })
    },

    /** 删一行主题（配置里删掉即可，已经没有凭据要级联清理）。 */
    async removeTopic(channelId: string) {
      const channels = this.snapshot.config.channels.filter(channel => channel.id !== channelId)
      await this.writeConfig({ ...this.snapshot.config, channels })
    },

    /**
     * 给某个服务下的每个主题各发一条测试，逐主题回成败。
     *
     * 命令本身是 `async` + 后端 `spawn_blocking`：几条请求串行，最坏 10s × N，
     * 不放到阻塞线程会冻结主窗事件循环。
     */
    async testProvider(providerId: string): Promise<PushTestOutcome[]> {
      this.pendingProvider = providerId
      try {
        return await invoke<PushTestOutcome[]>('test_push_provider', { providerId })
      } finally {
        this.pendingProvider = null
      }
    },
  },
})
