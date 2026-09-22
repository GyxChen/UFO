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
import { OVERFLOW_TOOLTIP_METRICS } from './lib/tooltip-size'

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
    // ⚠️ 徽章是圆的、图标是方的：图标占满（100%）时四个角会伸出圆外，徽章必须自己裁。
    expect(decl(badge, 'overflow')).toBe('hidden')

    // 图标占满徽章的内容盒；宽高都不写死，交给 object-contain 按各自的比例铺满。
    const icon = ruleBody(css, '.agent-status-module__icon')
    expect(decl(icon, 'width')).toBe('100%')
    expect(decl(icon, 'height')).toBe('100%')
    // `object-contain` 是 @apply 进去的工具类，读原文而不是读声明。
    expect(icon).toContain('object-contain')
  })

  it('amplifies the completion flash and keeps a reduced-motion fallback', () => {
    const css = agentStatusCss()
    const rule = ruleBody(css, '.agent-status-module__item.is-flashing .agent-status-module__dot')

    // 667ms = 原来 1s 的 1.5 倍频（1 Hz → 1.5 Hz）。
    expect(rule, '闪烁规则必须存在').not.toBe('')
    expect(decl(rule, 'animation')).toContain('667ms')
    expect(decl(rule, 'animation')).toContain('infinite')

    const frames = blockAfter(css, '@keyframes agent-status-flash {')
    expect(frames, '闪烁的 keyframes 必须存在').not.toBe('')
    // 峰值**涨出去**（不是往小缩）+ 谷底压得很暗：人对「变大 / 变亮」远比「变小 / 变暗」敏感，
    // 旧版只往 0.78 缩、谷底才 0.35，所以看着像呼吸不像事件。
    expect(frames).toContain('scale(1.1)')
    expect(frames).toContain('scale(0.82)')
    expect(frames).toContain('opacity: 0.12')
    // ⚠️ 峰值只有 1.10 的余量：灯点本身就是限制轴的 80%，再叠 1px 描边与 2px 光晕就贴到格子边了，
    // 放大被槽位的 overflow: hidden 切成直边是这套动画最容易踩的坑。
    expect(frames, '峰值不能再往上加（会被槽位切边）').not.toContain('scale(1.12)')

    // 全站其它动画都有 prefers-reduced-motion 降级，这条以前漏了。
    const reduced = blockAfter(css, '@media (prefers-reduced-motion: reduce)')
    expect(reduced, '闪烁必须有「减少动态效果」降级').not.toBe('')
    expect(reduced).toContain('.agent-status-module__item.is-flashing')
    expect(reduced).toContain('animation: none')
    // 降级不是「什么都不显」：停动画后留一圈更粗的静态同色描边，完成态仍然看得见。
    expect(reduced).toContain('box-shadow')
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

  it('keeps the overflow list height contract in sync with lib/tooltip-size.ts', () => {
    const css = agentStatusCss()
    // 行高钉死：信息卡的窗口高度就是按它算出来的（OVERFLOW_TOOLTIP_METRICS.row）。
    expect(Number.parseFloat(decl(ruleBody(css, '.tooltip-overflow-row'), 'height'))).toBe(
      OVERFLOW_TOOLTIP_METRICS.row
    )
    // 头部与兜底行同理 —— 不钉死就会随行高变化，把算好的窗口高度顶偏。
    expect(Number.parseFloat(decl(ruleBody(css, '.tooltip-overflow-head'), 'height'))).toBe(
      OVERFLOW_TOOLTIP_METRICS.head
    )
    expect(Number.parseFloat(decl(ruleBody(css, '.tooltip-overflow-note'), 'line-height'))).toBe(
      OVERFLOW_TOOLTIP_METRICS.note
    )
    // 折叠几个就列几行（不截断），行数超过屏幕时在这里滚，而不是把窗口撑出屏幕。
    expect(decl(ruleBody(css, '.tooltip-overflow-list'), 'overflow-y')).toBe('auto')
    // 被 flex 压扁会让行高失效（高度账全错），必须 shrink-0。
    expect(ruleBody(css, '.tooltip-overflow-row')).toContain('shrink-0')
    // 高度账里剩下的两项来自外壳（`src/assets/main.css` 的 `.tooltip-shell`：`p-2.5` = 上下 10px、
    // `gap-1.5` = 相邻 6px）。⚠️ 测试环境只放行模块 CSS 的 `?raw`（见 vitest.config.ts 的 `css.include`），
    // 入口那份读不到原文，所以它们只能靠这里的常量名与注释对齐 —— 改 main.css 时看这条注释。
    expect(OVERFLOW_TOOLTIP_METRICS.shellPaddingY).toBe(10)
    expect(OVERFLOW_TOOLTIP_METRICS.gap).toBe(6)
  })

  it('centres the idle platform ghosts across the whole grid', () => {
    const css = agentStatusCss()
    const idle = ruleBody(css, '.agent-status-module__idle')
    expect(idle, '空态必须有单独的规则').not.toBe('')

    // ⚠️ 必须跨满整个网格：模块根节点是 grid（列行按形态内联设置），不写这条鬼影会被塞进某一格，
    // 看上去是「缩在角上」而不是居中。⚠️ 四条线要写全 —— `1 / -1` 只是「行起 1 / 列起 -1」，
    // 行末与列末默认 auto（各占一格），看着像全跨其实只占最后一列那一格。
    expect(decl(idle, 'grid-area')).toBe('1 / 1 / -1 / -1')
    // 尺寸取两条轴里更紧的那条，且上限是设计像素 → 必须跟格子缩放走。
    const size = decl(idle, '--agent-idle-icon-h')
    expect(size).toContain('cqh')
    expect(size).toContain('cqw')
    expect(size).toContain('--floating-cell-scale')
    // 间隙按图标高算，比例才锁得死（写死 px 会在格子调大后变成两个贴在一起的标）。
    expect(decl(idle, 'gap')).toContain('--agent-idle-icon-h')

    const icon = ruleBody(css, '.agent-status-module__idle-icon')
    expect(decl(icon, 'height')).toBe('var(--agent-idle-icon-h)')
    // 只给高、宽交给资源比例：两个 logo 的 viewBox 不是同一个宽高比，写死宽会压扁其中一个。
    expect(decl(icon, 'width')).toBe('auto')
    expect(decl(icon, 'opacity')).toBe('0.38')
    expect(decl(icon, 'filter')).toBe('grayscale(1)')
  })

  it('draws the overflow ring as a hollow band centred in the cell', () => {
    const css = agentStatusCss()
    const band = blockAfter(css, '.agent-status-module__ring,')
    expect(band, '环与两圈光晕共用一条规则').not.toBe('')

    // 按宽度取百分比 + aspect-ratio：格子不是正方形，同时写 width / height 会变成椭圆。
    expect(decl(band, 'width')).toMatch(/%$/)
    expect(decl(band, 'aspect-ratio')).toBe('1')
    // 居中不依赖 flex 的「绝对定位子元素静态位置」，用 left/top 50% + translate 兜底。
    expect(decl(band, 'left')).toBe('50%')
    expect(decl(band, 'top')).toBe('50%')
    for (const selector of [
      '.agent-status-module__ring',
      '.agent-status-module__halo--near',
      '.agent-status-module__halo--far',
    ]) {
      expect(decl(ruleBody(css, selector), 'transform'), `${selector} 必须自己带居中的 translate`).toContain(
        'translate(-50%, -50%)'
      )
    }

    // 挖空中心成环带：closest-side 的半径 = 内切圆，两个百分比就是「盘的直径占比」，内 < 外。
    const mask = decl(band, 'mask')
    const stops = [...mask.matchAll(/(\d+)%/g)].map(match => Number(match[1]))
    expect(mask).toContain('radial-gradient(closest-side')
    expect(stops.length).toBe(2)
    expect(stops[0]).toBeLessThan(stops[1])

    // 核心要落在环的孔里（留出暗缝），否则彩边会糊在数字上。
    const ringWidth = Number.parseFloat(decl(band, 'width'))
    const coreWidth = Number.parseFloat(decl(ruleBody(css, '.agent-status-module__overflow-core'), 'width'))
    expect(ringWidth * (stops[0] / 100)).toBeGreaterThan(coreWidth)
  })

  it('keeps the halo inside the cell and the count readable at every cell size', () => {
    const css = agentStatusCss()
    const ringWidth = Number.parseFloat(decl(blockAfter(css, '.agent-status-module__ring,'), 'width'))
    const coreWidth = Number.parseFloat(decl(ruleBody(css, '.agent-status-module__overflow-core'), 'width'))

    // 最外的光晕放大后仍要留在格子里（真实槽位是 overflow: hidden，越界会被切出直边）。
    const far = ruleBody(css, '.agent-status-module__halo--far')
    const farScale = Number.parseFloat(/(?:^|\s)scale\(([\d.]+)\)/.exec(decl(far, 'transform'))?.[1] ?? '')
    expect(Number.isFinite(farScale)).toBe(true)
    expect(ringWidth * farScale).toBeLessThanOrEqual(100)
    // 越远的越淡。
    const nearOpacity = Number.parseFloat(decl(ruleBody(css, '.agent-status-module__halo--near'), 'opacity'))
    expect(nearOpacity).toBeGreaterThan(Number.parseFloat(decl(far, 'opacity')))

    // 核心比一格小一圈（不是吃满），且数字字号跟着格子缩放：格子调大后 +N 不能停在设计值上。
    expect(coreWidth).toBeLessThan(70)
    const count = ruleBody(css, '.agent-status-module__overflow')
    expect(decl(count, 'font-size')).toContain('--floating-cell-scale')
    // 三位数（+10 起）另收一档，否则会顶到核心圆边。
    expect(decl(ruleBody(css, '.agent-status-module__overflow--wide'), 'font-size')).toContain('--floating-cell-scale')
  })

  it('degrades the overflow ring the same way as the light in a narrow slot', () => {
    const css = agentStatusCss()
    for (const header of [
      '@container floating-slot (max-height: 32px)',
      '@container floating-slot (max-width: 32px)',
    ]) {
      const block = blockAfter(css, header)
      expect(block, `${header} 降级块必须存在`).not.toBe('')
      // 远环退场、环与核心放大一档：槽位小到看不清细节时，先保住「颜色 + 数字」。
      expect(block).toContain('.agent-status-module__halo--far')
      const ring = Number.parseFloat(decl(ruleBody(block, '.agent-status-module__ring'), 'width'))
      const core = Number.parseFloat(decl(ruleBody(block, '.agent-status-module__overflow-core'), 'width'))
      expect(ring).toBeGreaterThan(Number.parseFloat(decl(blockAfter(css, '.agent-status-module__ring,'), 'width')))
      expect(core).toBeGreaterThan(
        Number.parseFloat(decl(ruleBody(css, '.agent-status-module__overflow-core'), 'width'))
      )
      // 放大后仍要留在格子里。
      expect(ring).toBeLessThanOrEqual(100)
    }
  })
})
