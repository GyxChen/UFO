import { describe, expect, it, vi } from 'vitest'
import {
  applyThemeToDocument,
  DEFAULT_THEME,
  nextTheme,
  normalizeTheme,
  readStoredTheme,
  themeFromEventPayload,
  THEME_STORAGE_KEY,
  writeStoredTheme,
  type ThemeStorage,
} from './theme'

/** 内存版 Storage：只实现本模块用到的那两个方法。 */
function fakeStorage(initial: Record<string, string> = {}): ThemeStorage & { data: Record<string, string> } {
  const data = { ...initial }
  return {
    data,
    getItem: (key: string) => (key in data ? data[key] : null),
    setItem: (key: string, value: string) => {
      data[key] = value
    },
  }
}

describe('theme primitives', () => {
  it('falls back to the default theme for unknown values', () => {
    expect(normalizeTheme('light')).toBe('light')
    expect(normalizeTheme('dark')).toBe('dark')
    // 脏值（手改的、旧版本写的）一律回落默认，而不是让整窗没有主题。
    expect(normalizeTheme('solarized')).toBe(DEFAULT_THEME)
    expect(normalizeTheme(undefined)).toBe(DEFAULT_THEME)
    expect(normalizeTheme(null)).toBe(DEFAULT_THEME)
    expect(normalizeTheme(0)).toBe(DEFAULT_THEME)
  })

  it('toggles between the two themes', () => {
    expect(nextTheme('dark')).toBe('light')
    expect(nextTheme('light')).toBe('dark')
    expect(nextTheme(nextTheme('dark'))).toBe('dark')
  })

  it('reads a stored theme and falls back when missing or dirty', () => {
    expect(readStoredTheme(fakeStorage({ [THEME_STORAGE_KEY]: 'light' }))).toBe('light')
    expect(readStoredTheme(fakeStorage())).toBe(DEFAULT_THEME)
    expect(readStoredTheme(fakeStorage({ [THEME_STORAGE_KEY]: '  light  ' }))).toBe(DEFAULT_THEME)
  })

  it('survives a storage that throws (private mode / disabled storage)', () => {
    const hostile: ThemeStorage = {
      getItem: () => {
        throw new Error('storage disabled')
      },
      setItem: () => {
        throw new Error('storage disabled')
      },
    }
    expect(readStoredTheme(hostile)).toBe(DEFAULT_THEME)
    expect(() => writeStoredTheme(hostile, 'light')).not.toThrow()
    expect(readStoredTheme(null)).toBe(DEFAULT_THEME)
    expect(() => writeStoredTheme(undefined, 'light')).not.toThrow()
  })

  it('writes the theme under the shared key', () => {
    const storage = fakeStorage()
    writeStoredTheme(storage, 'light')
    expect(storage.data[THEME_STORAGE_KEY]).toBe('light')
  })

  it('applies the theme as a data attribute on the document root', () => {
    // CSS 靠 `:root[data-theme='light']` 覆盖 token；属性名写错整套浅色就静默失效。
    const root = { dataset: {} } as unknown as HTMLElement
    applyThemeToDocument(root, 'light')
    expect(root.dataset.theme).toBe('light')
    applyThemeToDocument(root, 'dark')
    expect(root.dataset.theme).toBe('dark')
  })

  it('ignores event payloads that are not a theme name', () => {
    expect(themeFromEventPayload('light')).toBe('light')
    expect(themeFromEventPayload('dark')).toBe('dark')
    expect(themeFromEventPayload({ theme: 'light' })).toBeNull()
    expect(themeFromEventPayload(undefined)).toBeNull()
  })

  it('does not touch storage when reading from an unavailable store', () => {
    const getItem = vi.fn(() => null)
    readStoredTheme({ getItem, setItem: vi.fn() })
    expect(getItem).toHaveBeenCalledWith(THEME_STORAGE_KEY)
  })
})
