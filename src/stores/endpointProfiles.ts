import { defineStore } from 'pinia'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  addEndpointProfile,
  deleteEndpointProfile,
  emptyProfileList,
  fetchEndpointProfiles,
  isEndpointProfileList,
  ENDPOINT_PROFILES_EVENT,
  type EndpointProfileList,
} from '@/lib/endpoint-profiles'

let stopListener: UnlistenFn | undefined

/**
 * endpoint 档案 store（主窗口「信息卡拓展」的数据源）。
 *
 * 与 `floatingModules` 分开：配置改动走 `floating-module-config` 事件，
 * 档案的变更源是 monitor 线程的采集（会话里发现新 endpoint），两者频率与语义都不同。
 */
export const useEndpointProfilesStore = defineStore('endpoint-profiles', {
  state: () => ({
    list: emptyProfileList() as EndpointProfileList,
    started: false,
    /** 首次拉取是否已完成；用于区分「真的没有 endpoint」与「还没读到」。 */
    loaded: false,
    connection: 'starting' as 'starting' | 'connected' | 'degraded',
  }),

  getters: {
    profiles: (state) => state.list.profiles,
    revision: (state) => state.list.revision,
  },

  actions: {
    /** 覆盖本地状态；丢弃 revision 回退的事件（后端并发写时可能乱序）。 */
    applyList(list: EndpointProfileList) {
      if (list.revision < this.list.revision) return
      this.list = list
      this.loaded = true
      this.connection = 'connected'
    },

    async start() {
      if (this.started) return
      this.started = true
      try {
        stopListener = await listen<EndpointProfileList>(ENDPOINT_PROFILES_EVENT, (event) => {
          if (isEndpointProfileList(event.payload)) this.applyList(event.payload)
        })
        this.applyList(await fetchEndpointProfiles())
      } catch (error) {
        // 读不到就不可能再等到数据：标记 loaded，让 UI 走「降级」而不是永远停在「正在读取」。
        this.connection = 'degraded'
        this.loaded = true
        console.warn('endpoint profile channel unavailable', error)
      }
    },

    /** 手动登记一条 endpoint；后端拒绝时抛错，由调用方展示原因。 */
    async add(url: string) {
      this.applyList(await addEndpointProfile(url))
    },

    /** 删除一条档案。 */
    async remove(id: string) {
      this.applyList(await deleteEndpointProfile(id))
    },

    stop() {
      stopListener?.()
      stopListener = undefined
      this.started = false
    },
  },
})
