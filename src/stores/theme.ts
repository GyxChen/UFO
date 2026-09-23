import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { defineStore } from 'pinia'
import {
  applyThemeToDocument,
  browserThemeStorage,
  DEFAULT_THEME,
  nextTheme,
  readStoredTheme,
  THEME_CHANGE_EVENT,
  themeFromEventPayload,
  writeStoredTheme,
  type ThemeName,
} from '@/lib/theme'

/** 跨窗口事件订阅的清理函数；三个窗口各自一份，故放模块级。 */
let stopListener: UnlistenFn | undefined

/**
 * 界面主题（深色 / 浅色）。
 *
 * 主题是**窗口级（外壳）数据**：主窗、悬浮窗、提示窗三份前端都要用同一份取值，
 * 所以由外壳在 `App.vue` 里 `start()` 拉起，任何具体组件都不参与。
 *
 * 三个窗口在启动时都已创建（main / tooltip 只是 `visible: false`），
 * 因此「主窗切主题 → 另两窗跟着变」这条广播一定有人接。
 *
 * ⚠️ `hydrate()` 必须在首帧之前调用（见 `main.ts`）：`start()` 里的 `listen` 是异步的，
 * 等它返回才应用主题的话，浅色用户每次启动都会先看到一帧深色。
 */
export const useThemeStore = defineStore('theme', {
  state: () => ({
    /** 当前主题。 */
    theme: DEFAULT_THEME as ThemeName,
    /** 是否已经从存储里同步读过一次（`hydrate` 标记）。 */
    hydrated: false,
    /** 是否已经挂上跨窗口监听（`start` 标记）。 */
    started: false,
  }),

  getters: {
    /** 是否深色主题（图标与文案按它分派）。 */
    isDark: (state): boolean => state.theme === 'dark',
  },

  actions: {
    /**
     * 应用主题：写进 state、落到文档根、并同步持久化。
     *
     * 收到别的窗口的广播时也走这里，于是**每个窗口各存一份**：
     * 即便某个平台上跨窗口共享 localStorage 不可靠，各窗口自己的下次启动仍是正确的。
     */
    apply(theme: ThemeName) {
      this.theme = theme
      applyThemeToDocument(document.documentElement, theme)
      writeStoredTheme(browserThemeStorage(), theme)
    },

    /**
     * 同步读一次已存主题并落地；重复调用不会读第二遍。
     * 首帧之前调用（`main.ts`）才能保证浅色主题不带闪烁。
     */
    hydrate() {
      if (this.hydrated) return
      this.hydrated = true
      this.apply(readStoredTheme(browserThemeStorage()))
    },

    /** 切换主题：本地先生效，再广播给另两个窗口（自己收到也幂等）。 */
    setTheme(theme: ThemeName) {
      if (theme === this.theme) return
      this.apply(theme)
      void emit(THEME_CHANGE_EVENT, theme).catch(error => {
        console.warn('failed to broadcast theme change', error)
      })
    },

    /** 在深色 / 浅色之间切换（主题按钮点击）。 */
    toggle() {
      this.setTheme(nextTheme(this.theme))
    },

    /**
     * 开始跟随跨窗口广播（其余窗口切主题时跟着变）。
     * 订阅失败也不影响本次生效 —— 只是收不到另两个窗口的切换而已。
     */
    async start() {
      this.hydrate()
      if (this.started) return
      this.started = true
      try {
        stopListener = await listen<unknown>(THEME_CHANGE_EVENT, event => {
          const theme = themeFromEventPayload(event.payload)
          if (theme) this.apply(theme)
        })
      } catch (error) {
        console.warn('theme channel unavailable', error)
      }
    },

    /** 停止订阅（外壳卸载时调用）。 */
    stop() {
      stopListener?.()
      stopListener = undefined
      this.started = false
    },
  },
})
