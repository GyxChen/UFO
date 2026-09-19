import { defineStore } from 'pinia'
import { retryOnStartupRace } from '@/lib/startup-retry'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  emptyFlowSnapshot,
  fetchFlowSnapshot,
  isFlowSnapshot,
  FLOW_RESULTS_EVENT,
  type FlowField,
  type FlowSessionResult,
  type FlowSnapshot,
} from '../lib/flow'

let stopListener: UnlistenFn | undefined

/**
 * 流程脚本结果 store（tooltip 信息卡的数据源）。
 *
 * 与 `endpointProfiles`（配置）/ `floatingModules`（悬浮窗配置）分开：这里的变更源是后端 worker
 * 跑完脚本的广播，频率由 endpoint 的轮询间隔决定，语义是「某会话当前该显示哪些拓展字段」。
 *
 * ⚠️ 必须在 `App.vue` 里 `start()` —— tooltip 是独立窗口要用它，和 `tokenUnit` 那个坑同理，
 * 不启动就永远拿不到数据。
 */
export const useFlowResultsStore = defineStore('flow-results', {
  state: () => ({
    snapshot: emptyFlowSnapshot() as FlowSnapshot,
    started: false,
  }),

  getters: {
    /** 会话 key → 该会话 endpoint 的拓展结果（字段 + 新鲜度）；tooltip 按 `session.key` 直接查。 */
    bySession: (state) => state.snapshot.sessions,
  },

  actions: {
    /** 覆盖本地状态；丢弃 revision 回退的事件（worker 并发写时可能乱序）。 */
    applySnapshot(snapshot: FlowSnapshot) {
      if (snapshot.revision < this.snapshot.revision) return
      this.snapshot = snapshot
    },

    /** 取某会话完整的拓展结果（无则 null，tooltip 用它显示刷新时间标注）。 */
    resultFor(sessionKey: string | null | undefined): FlowSessionResult | null {
      if (!sessionKey) return null
      return this.snapshot.sessions[sessionKey] ?? null
    },

    /** 取某会话应显示的字段（无则空数组，调用方据此不渲染该行）。 */
    fieldsFor(sessionKey: string | null | undefined): FlowField[] {
      return this.resultFor(sessionKey)?.fields ?? []
    },

    async start() {
      if (this.started) return
      this.started = true
      try {
        stopListener = await listen<FlowSnapshot>(FLOW_RESULTS_EVENT, (event) => {
          if (isFlowSnapshot(event.payload)) this.applySnapshot(event.payload)
        })
        // 首帧 IPC 可能早于后端 .manage()：重试几次，别让一次竞态退化成「没有拓展字段」。
        const initial = await retryOnStartupRace(fetchFlowSnapshot)
        if (isFlowSnapshot(initial)) this.applySnapshot(initial)
      } catch (error) {
        // 读不到就走空快照：tooltip 不显拓展字段，不影响其它行。
        console.warn('flow results channel unavailable', error)
      }
    },

    stop() {
      stopListener?.()
      stopListener = undefined
      this.started = false
    },
  },
})
