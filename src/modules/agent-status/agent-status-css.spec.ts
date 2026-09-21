/**
 * Agent 状态灯的「一格内容真的放得下」护栏。
 *
 * 布局已从「灯点 + 平台图标并排」改为「灯点吃满格子 + 图标缩成右下角徽章（badge）」。
 * 这里锁住几条容易被改回去的规矩：
 *
 * 1. 状态灯要**占满格子**（用户明确要求「占满网格但留点 margin」）：默认按限制轴取的百分比
 *    必须够大（≥ 70%），且明显大于角标徽章 —— 否则又退化成「小灯点 + 大图标」的老样子。
 * 2. 徽章必须**绝对定位叠在灯点上**（`position: absolute` + 有实色背景），而不是排回灯点旁边；
 *    且徽章不溢出格子（`right + width ≤ 100%`）。
 * 3. 竖条（左右贴边）必须按**行宽**取百分比，不能按行高 —— 竖条窗口只有一格宽，
 *    行宽（45 格时 39.5px）比行高（43.2px）更窄；按行高算会把灯点右侧裁掉。
 * 4. 窄槽降级：槽位矮于 / 窄于 32px 时藏掉徽章、把灯点放大到吃满格子；
 *    竖条那条规则选择器更长，宽度降级块里要带同一条选择器才盖得住。
 *
 * ⚠️ 这里断言的是**声明口径**，不是排版结果：测试环境没有排版引擎。
 * 真实像素用无头 Chrome 量过（45 / 64 / 96 三档竖条都能放下）。
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
  it('fills the cell with the status light, bigger than the badge', () => {
    const css = agentStatusCss()
    const dot = Number.parseFloat(decl(ruleBody(css, '.agent-status-module__dot'), 'height'))
    const badge = Number.parseFloat(decl(ruleBody(css, '.agent-status-module__identity'), 'width'))

    expect(Number.isFinite(dot) && Number.isFinite(badge)).toBe(true)
    // 状态灯要占满格子（四周留一点 margin），至少 70%，不能又缩回小圆点。
    expect(dot).toBeGreaterThanOrEqual(70)
    // 灯点是主体，明显大于角标徽章。
    expect(dot).toBeGreaterThan(badge)
  })

  it('overlays the icon as an absolutely-positioned badge, not beside the light', () => {
    const css = agentStatusCss()
    const badge = ruleBody(css, '.agent-status-module__identity')

    // 徽章是绝对定位叠在灯点上，不再排回灯点旁边。
    expect(decl(badge, 'position')).toBe('absolute')
    // 有实色背景（token），才能从彩色灯点上"抠"出来。
    expect(decl(badge, 'background')).toMatch(/var\(--ufo-/)
    // 徽章不溢出格子：右边距 + 宽度 ≤ 100%。
    const right = Number.parseFloat(decl(badge, 'right'))
    const width = Number.parseFloat(decl(badge, 'width'))
    expect(right + width).toBeLessThanOrEqual(100)
  })

  it('sizes the vertical light by row width, not row height', () => {
    const css = agentStatusCss()
    const body = ruleBody(css, '.agent-status-module--vertical .agent-status-module__dot')
    expect(body, '竖条必须按行宽取百分比').not.toBe('')
    expect(decl(body, 'width'), '竖条要按行宽给百分比').toMatch(/%$/)
    expect(decl(body, 'height'), '竖条的高度要交给 aspect-ratio').toBe('auto')
  })

  it('keeps the narrow-slot fallback able to override the vertical light size', () => {
    const css = agentStatusCss()
    const narrow = blockAfter(css, '@container floating-slot (max-width: 32px)')
    expect(narrow, '窄槽降级块必须存在（竖条的限制轴是行宽）').not.toBe('')
    // 竖条那条规则选择器更长，降级块里不带同一条选择器就盖不住它，降级会静默失效。
    expect(narrow).toContain('.agent-status-module--vertical .agent-status-module__dot')
    expect(decl(ruleBody(narrow, '.agent-status-module--vertical .agent-status-module__dot'), 'width')).toBe('86%')
    // 矮槽 / 窄槽都要藏掉徽章，把面积让给灯色。
    expect(narrow).toContain('display: none')
    const short = blockAfter(css, '@container floating-slot (max-height: 32px)')
    expect(short, '矮槽降级块必须存在（横条 / 方形的限制轴是行高）').not.toBe('')
    expect(short).toContain('display: none')
  })
})