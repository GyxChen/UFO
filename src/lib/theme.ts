/**
 * 主题（深色 / 浅色）的纯逻辑：取值规整、落盘读写、应用到文档。
 *
 * 主题是**纯表现层偏好** —— 后端完全不需要知道它（不像 `cell_px` 会影响窗口几何），
 * 所以存在 localStorage 里。好处是读取是同步的：首帧就能带上正确主题，不会先闪一下深色。
 * 跨窗口同步走 Tauri 事件（见 `stores/theme.ts`），因为三个窗口各自跑一份前端。
 */

/** 主题名。CSS 侧靠 `:root[data-theme='<name>']` 覆盖 token。 */
export type ThemeName = 'dark' | 'light'

/** 默认主题：沿用项目原有的 Matte 深色外观。 */
export const DEFAULT_THEME: ThemeName = 'dark'

/** localStorage 键名。 */
export const THEME_STORAGE_KEY = 'ufo-theme'

/** 主题变更的跨窗口事件名；带 `ufo-` 前缀以免与 Tauri 内置事件撞名。 */
export const THEME_CHANGE_EVENT = 'ufo-theme-change'

/**
 * 把任意输入规整成合法主题名。
 * 存储里的脏值（手改的、旧版本写的）一律回落默认主题，而不是让整窗没有主题。
 */
export function normalizeTheme(value: unknown): ThemeName {
  return value === 'light' || value === 'dark' ? value : DEFAULT_THEME
}

/** 另一个主题（切换按钮用）。 */
export function nextTheme(current: ThemeName): ThemeName {
  return current === 'dark' ? 'light' : 'dark'
}

/** 能读能写的存储（`Storage` 的子集，便于测试时传假对象）。 */
export interface ThemeStorage {
  getItem(key: string): string | null
  setItem(key: string, value: string): void
}

/**
 * 从存储里读主题。
 * 存储不可用（隐私模式 / 宿主禁用）或取到脏值都回落默认主题 —— 主题读不到不该让页面挂掉。
 */
export function readStoredTheme(storage: ThemeStorage | null | undefined): ThemeName {
  try {
    return normalizeTheme(storage?.getItem(THEME_STORAGE_KEY))
  } catch {
    return DEFAULT_THEME
  }
}

/** 写主题到存储。写失败静默忽略：配额满 / 存储被禁都不该让「切主题」这个动作失败。 */
export function writeStoredTheme(storage: ThemeStorage | null | undefined, theme: ThemeName): void {
  try {
    storage?.setItem(THEME_STORAGE_KEY, theme)
  } catch {
    // 忽略：持久化失败只影响下次启动的默认值，不影响本次切换。
  }
}

/**
 * 把主题落到文档根上：`data-theme` 属性供 CSS 覆盖 token。
 *
 * ⚠️ 必须在首帧之前调用（`main.ts` 挂载前），否则会先渲染默认深色再跳浅色。
 * `color-scheme` 不在这里写：它由 CSS 的 `:root[data-theme]` 规则给出，避免 JS 与 CSS 两处真相。
 */
export function applyThemeToDocument(root: HTMLElement, theme: ThemeName): void {
  root.dataset.theme = theme
}

/** 从事件载荷里取主题；认不出就返回 null（表示「这条事件不用理会」）。 */
export function themeFromEventPayload(payload: unknown): ThemeName | null {
  return payload === 'light' || payload === 'dark' ? payload : null
}

/**
 * 取浏览器的 localStorage。
 * 连取属性都可能抛（宿主禁用存储时），所以这里兜一层；不可用就返回 null，
 * 由 `readStoredTheme` / `writeStoredTheme` 各自回落到默认主题 / 静默忽略。
 */
export function browserThemeStorage(): ThemeStorage | null {
  try {
    return window.localStorage
  } catch {
    return null
  }
}
