/**
 * Agent 状态灯的「一格内容真的放得下」护栏。
 *
 * 一行灯 = 灯点 + 间隙 + 平台图标，三者都用百分比声明。这里锁住两条容易被改回去的规矩：
 *
 * 1. 三个百分比之和必须小于 100% —— 超出就会被槽位的 `overflow:hidden` 切掉；
 * 2. 竖条（左右贴边）必须按**行宽**取百分比，不能按行高 —— 竖条窗口只有一格宽，
 *    行宽（45 格时 39.5px）比行高（43.2px）更窄；按行高算出的总宽是 42.4px，
 *    会把图标右侧裁掉约 1.4px，观感就是「Agent 状态没显示完整」。
 *    方形 / 四角 / 横条的行宽 ≥ 行高，按行高算本来就不会溢出，不受这条约束。
 *
 * ⚠️ 这里断言的是**声明口径**，不是排版结果：测试环境没有排版引擎。
 * 真实像素用无头 Chrome 量过（45 / 64 / 96 三档竖条都能放下，且灯点大小与方形 / 横条一致）。
 */
import { describe, expect, it } from 'vitest'

/** 模块样式文件：路径 → 原文。 */
const moduleStyles = import.meta.glob('/src/modules/agent-status/*.css', {
  eager: true,
  query: '?raw',
  import: 'default',
}) as Record<string, string>

/** 取样式原文（本模块只有一个样式文件）。 */
function agentStatusCss(): string {
  const entry = Object.entries(moduleStyles).find(([path]) => path.endsWith('agent-status.css'))
  expect(entry, '没找到 agent-status.css').toBeTruthy()
  return entry?.[1] ?? ''
}

/** 按花括号配对取出一段块（用于 @container 这类嵌套块）。 */
function blockAfter(css: string, header: string): string {
  const start = css.indexOf(header)
  if (start < 0) return ''
  const open = css.indexOf('{', start)
  if (open < 0) return ''
  let depth = 0
  for (let index = open; index < css.length; index += 1) {
    if (css[index] === '{') depth += 1
    else if (css[index] === '}') {
      depth -= 1
      if (depth === 0) return css.slice(open + 1, index)
    }
  }
  return ''
}

/** 取某个选择器的规则体；找不到返回空串。 */
function ruleBody(css: string, selector: string): string {
  return blockAfter(css, `${selector} {`)
}

/** 从规则体里读一个声明值（如 `height: 52.5%` → `52.5%`）。 */
function decl(body: string, property: string): string {
  const match = new RegExp(`(?:^|[;{\\s])${property}\\s*:\\s*([^;]+)`).exec(body)
  return match ? match[1].trim() : ''
}

describe('模块样式 · Agent 状态灯一格放得下', () => {
  it('keeps light + gap + icon under the full row width', () => {
    const css = agentStatusCss()
    const light = Number.parseFloat(decl(ruleBody(css, '.agent-status-module__dot'), 'height'))
    const icon = Number.parseFloat(decl(ruleBody(css, '.agent-status-module__identity'), 'height'))
    const gap = Number.parseFloat(decl(ruleBody(css, '.agent-status-module__item'), 'column-gap'))

    expect(Number.isFinite(light) && Number.isFinite(icon) && Number.isFinite(gap)).toBe(true)
    expect(light + gap + icon).toBeLessThan(100)
  })

  it('sizes the vertical light by row width, not row height', () => {
    const css = agentStatusCss()
    for (const selector of [
      '.agent-status-module--vertical .agent-status-module__dot',
      '.agent-status-module--vertical .agent-status-module__identity',
    ]) {
      const body = ruleBody(css, selector)
      expect(body, `${selector} 必须存在：竖条按行宽取百分比`).not.toBe('')
      expect(decl(body, 'width'), `${selector} 要按行宽给百分比`).toMatch(/%$/)
      expect(decl(body, 'height'), `${selector} 的高度要交给 aspect-ratio`).toBe('auto')
    }
  })

  it('keeps the narrow-slot fallback able to override the vertical light size', () => {
    const css = agentStatusCss()
    const narrow = blockAfter(css, '@container floating-slot (max-width: 32px)')
    expect(narrow, '窄槽降级块必须存在（竖条的限制轴是行宽）').not.toBe('')
    // 竖条那条规则选择器更长，降级块里不带同一条选择器就盖不住它，降级会静默失效。
    expect(narrow).toContain('.agent-status-module--vertical .agent-status-module__dot')
    expect(decl(ruleBody(narrow, '.agent-status-module--vertical .agent-status-module__dot'), 'width')).toBe('86%')
    expect(narrow).toContain('display: none')
  })
})