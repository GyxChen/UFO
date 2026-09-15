import { defineStore } from 'pinia'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  SESSION_RECORDS_EVENT,
  clearSessionRecords,
  deleteSessionRecord,
  emptyRecordList,
  fetchSessionRecords,
  isSessionRecordList,
  type SessionRecordList,
} from '@/lib/session-records'

let stopListener: UnlistenFn | undefined

/**
 * 会话存档 store（主窗口「会话记录」页的数据源）。
 *
 * 与 `agentStatus`（内存里的存活会话）分开：这里的行来自 SQLite，跨重启仍在；
 * 存活态灯色由前端按 key join `agentStatus` 得到，不写进存档。
 * 与 `endpointProfiles` 同构：变更源有两处 —— monitor 线程刷盘 + 用户删除/清空。
 */
export const useSessionRecordsStore = defineStore('session-records', {
  state: () => ({
    list: emptyRecordList() as SessionRecordList,
    started: false,
    /** 首次拉取是否完成；区分「真的没有记录」与「还没读到」。 */
    loaded: false,
    connection: 'starting' as 'starting' | 'connected' | 'degraded',
  }),

  getters: {
    records: (state) => state.list.records,
    revision: (state) => state.list.revision,
  },

  actions: {
    /** 覆盖本地状态；丢弃 revision 回退的事件（后端并发写时可能乱序）。 */
    applyList(list: SessionRecordList) {
      if (list.revision < this.list.revision) return
      this.list = list
      this.loaded = true
      this.connection = 'connected'
    },

    async start() {
      if (this.started) return
      this.started = true
      try {
        stopListener = await listen<SessionRecordList>(SESSION_RECORDS_EVENT, (event) => {
          if (isSessionRecordList(event.payload)) this.applyList(event.payload)
        })
        this.applyList(await fetchSessionRecords())
      } catch (error) {
        // 读不到就不可能再等到数据：标记 loaded，让 UI 走「降级」而不是永远停在「正在读取」。
        this.connection = 'degraded'
        this.loaded = true
        console.warn('session record channel unavailable', error)
      }
    },

    /** 删除一条存档；后端拒绝时抛错，由调用方展示原因。 */
    async remove(key: string) {
      this.applyList(await deleteSessionRecord(key))
    },

    /** 清空全部存档（只删归档）。 */
    async clear() {
      this.applyList(await clearSessionRecords())
    },

    stop() {
      stopListener?.()
      stopListener = undefined
      this.started = false
    },
  },
})
