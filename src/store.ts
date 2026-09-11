import { acceptHMRUpdate, defineStore } from 'pinia'

const versionString =
  import.meta.env.MODE === 'development' ? `${import.meta.env.VITE_APP_VERSION}-dev` : import.meta.env.VITE_APP_VERSION

export const useStore = defineStore('main', {
  state: () => ({
    debug: import.meta.env.MODE === 'development',
    version: versionString,
    isInitialized: false,
  }),

  actions: {
    /** 标记应用已完成前端初始化。 */
    initApp() {
      this.isInitialized = true
    },
  },

  getters: {
    /** 是否仍未完成初始化（历史命名保留）。 */
    isReady: (state) => {
      return !state.isInitialized
    },
  },
})

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useStore, import.meta.hot))
}
