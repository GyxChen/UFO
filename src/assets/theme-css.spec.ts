import { describe, expect, it } from 'vitest'
// ?raw 读入口 CSS：这条护栏要断言的正是「文件里写着什么」，走文本导入最直接。
import css from './main.css?raw'

/**
 * 主题 token 的护栏。
 *
 * 浅色主题是靠 `:root[data-theme='light']` 逐条覆盖 `:root` 的取值实现的 ——
 * **新增一个 token 却忘了给浅色档**，表现是「深色下一切正常、切浅色后某一处仍然发暗」，
 * 肉眼很容易漏。所以这里机械地对齐两边，而不是靠人记。
 */

/** 取一个选择器块的主体（按大括号配平）。 */
function blockBody(source: string, selector: string): string {
  const start = source.indexOf(selector)
  if (start < 0) throw new Error(`找不到选择器 ${selector}`)
  const open = source.indexOf('{', start)
  if (open < 0) throw new Error(`${selector} 后面没有 {`)
  let depth = 0
  for (let i = open; i < source.length; i += 1) {
    if (source[i] === '{') depth += 1
    else if (source[i] === '}') {
      depth -= 1
      if (depth === 0) return source.slice(open + 1, i)
    }
  }
  throw new Error(`${selector} 的大括号没有闭合`)
}

/** 抓出块里声明的自定义属性名（保持出现顺序，便于报错时说清缺了谁）。 */
function declaredTokens(body: string): string[] {
  return [...body.matchAll(/^\s*(--[\w-]+)\s*:/gm)].map(match => match[1])
}

/** 默认（深色）档：注意必须匹配 `:root {` 而不是 `:root[data-theme=...]`。 */
const darkBody = blockBody(css, '\n:root {')
const lightBody = blockBody(css, ":root[data-theme='light']")
const dark = declaredTokens(darkBody)
const light = new Set(declaredTokens(lightBody))

/**
 * 两个主题**取值相同**、因此不需要在浅色档重复声明的 token。
 * 加进这里等于显式声明「这一个是主题无关的」—— 比默默漏掉安全。
 */
const THEME_INDEPENDENT = new Set([
  // 圆角与色彩无关
  '--ufo-radius',
  // 强调色的**填充**档：浅色下强调底仍是那支翠绿（当文字另走 --ufo-accent-text）
  '--ufo-accent',
  // 强调底上的字：始终是深绿（填充没变，它就不能变）
  '--ufo-accent-on',
  // 状态色的**填充**档：灯点 / 色条 / 边框 / 光晕两个主题都要鲜艳（当文字另走 -text）
  '--ufo-status-green',
  '--ufo-status-yellow',
  '--ufo-status-red',
  '--ufo-status-blue',
])

describe('主题 token 护栏', () => {
  it('parses both theme blocks', () => {
    expect(dark.length).toBeGreaterThan(20)
    expect(light.size).toBeGreaterThan(20)
  })

  it('overrides every theme-dependent token in the light theme', () => {
    const missing = dark.filter(token => !light.has(token) && !THEME_INDEPENDENT.has(token))
    expect(missing, `这些 token 只在深色档给了值，切浅色会沿用深色：${missing.join(', ')}`).toEqual([])
  })

  it('keeps the theme-independent allowlist honest', () => {
    // 允许列表里写着「主题无关」的 token 必须真的存在，也不能在浅色档里被重复声明。
    for (const token of THEME_INDEPENDENT) {
      expect(dark, `${token} 已不存在，从 THEME_INDEPENDENT 里删掉`).toContain(token)
      expect(light.has(token), `${token} 已在浅色档声明，从 THEME_INDEPENDENT 里删掉`).toBe(false)
    }
  })

  it('gives the script editor highlight colours a light counterpart', () => {
    // 高亮配色是深色底上调出来的，白底上几乎看不见 —— 必须整套换掉。
    const jse = dark.filter(token => token.startsWith('--jse-'))
    expect(jse.length).toBeGreaterThan(5)
    expect(jse.filter(token => !light.has(token))).toEqual([])
  })

  it('keeps the Tailwind mirrors in sync with the light theme', () => {
    // `--color-ufo-*` 是 @theme 那块的镜像（Tailwind 工具类用），浅色档也要一起改。
    const mirrors = [...light].filter(token => token.startsWith('--color-ufo-'))
    expect(mirrors.length).toBeGreaterThan(10)
    for (const mirror of mirrors) {
      expect(css, `${mirror} 在 @theme 里没有对应声明`).toContain(`${mirror}:`)
    }
  })

  it('drives the monochrome-icon filter from the light theme', () => {
    // 白色图标（close / gear / codex / moon / sun）不反相的话在白底上直接隐形。
    expect(dark).toContain('--ufo-mono-icon-filter')
    expect(css).toMatch(/\.ufo-mono-icon\s*\{\s*filter:\s*var\(--ufo-mono-icon-filter\)/)
  })
})
