/**
 * 「组件尺寸必须跟着格子大小缩放」的护栏。
 *
 * 用户在「悬浮窗设置」里能把一格调到 32–96px（配置 `cell_px`）。模块里的百分比 / `cqh` 尺寸
 * 本来就是跟着槽位走的，但**写死的设计像素**（字号、图标上限、描边、间隙）不会自己长 ——
 * 一旦冻在设计值上，格子调大后就会出现「灯点长大一倍、+N 还是原来那么小」这种比例散掉的现象。
 * 约定是乘宿主声明的 `var(--floating-cell-scale, 1)`（1 = 设计基准 45px），这个测试拦退化。
 *
 * ⚠️ 不在此列：`@container` 里的绝对 px 断点（如「槽位矮于 33px 就看不清」）—— 那是**可读性阈值**，
 * 与设计缩放无关，本来就不该乘倍率。
 * ⚠️ 也不管信息卡（tooltip）：它是另一个固定尺寸的窗口，不参与格子缩放。
 */
import { describe, expect, it } from 'vitest'

/** 所有模块自带的样式文件：路径 → 原文。 */
const moduleStyles = import.meta.glob('/src/modules/**/*.css', {
  eager: true,
  query: '?raw',
  import: 'default',
}) as Record<string, string>

/**
 * 抽出 CSS 里每个 `clamp(...)` 的参数列表。
 *
 * 按括号配对切片，不能直接用正则 —— 上限常常是 `calc(24px * var(--floating-cell-scale, 1))`，
 * 里面的括号会把「取到第一个右括号」的写法截断。
 */
function clampArgs(css: string): string[][] {
  const results: string[][] = []
  const needle = 'clamp('
  let index = css.indexOf(needle)
  while (index >= 0) {
    let depth = 0
    let end = index + needle.length - 1
    for (; end < css.length; end += 1) {
      if (css[end] === '(') depth += 1
      else if (css[end] === ')') {
        depth -= 1
        if (depth === 0) break
      }
    }
    const body = css.slice(index + needle.length, end)
    // 按顶层逗号切参数（省略 calc(...) 里那些逗号）。
    const args: string[] = []
    let current = ''
    let nested = 0
    for (const char of body) {
      if (char === '(') nested += 1
      else if (char === ')') nested -= 1
      if (char === ',' && nested === 0) {
        args.push(current.trim())
        current = ''
        continue
      }
      current += char
    }
    args.push(current.trim())
    results.push(args)
    index = css.indexOf(needle, end)
  }
  return results
}

describe('模块样式 · 按格子缩放', () => {
  it('finds module stylesheets to inspect', () => {
    const files = Object.keys(moduleStyles)
    expect(files.length).toBeGreaterThan(0)
    // 至少有一个模块在用 clamp 做设计尺寸，否则下面的断言会变成空转。
    expect(Object.values(moduleStyles).some(css => css.includes('clamp('))).toBe(true)
  })

  it('scales every clamp upper bound that is a design pixel value', () => {
    const offenders: string[] = []
    for (const [path, css] of Object.entries(moduleStyles)) {
      for (const args of clampArgs(css)) {
        if (args.length !== 3) continue
        const upper = args[2]
        // 上限本来就是 cq 单位（跟着槽位走）的不要求；写死的 px 设计值必须乘倍率。
        if (!/\d+px/.test(upper)) continue
        if (upper.includes('--floating-cell-scale')) continue
        offenders.push(`${path}: clamp(${args.join(', ')})`)
      }
    }

    expect(offenders, 'clamp 的上限是设计像素时要写成 calc(<设计像素> * var(--floating-cell-scale, 1))').toEqual(
      []
    )
  })

  it('wires the scale variable into every stylesheet that sizes itself against the slot', () => {
    const missing = Object.entries(moduleStyles)
      .filter(([, css]) => css.includes('cqh') && !css.includes('--floating-cell-scale'))
      .map(([path]) => path)

    expect(missing, '用 cqh 按槽位排版的文件都必须引用 --floating-cell-scale 来缩放写死的设计像素').toEqual([])
  })
})