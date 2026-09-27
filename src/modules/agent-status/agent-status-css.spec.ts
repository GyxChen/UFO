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

/**
 * 定位某个位置所属规则的选择器（用于「按声明反查选择器」的护栏）。
 * 往回找到最近的 `{`，再往前取到上一个 `}` / `{`（`@media` 块头）之间的文本，注释剥掉。
 */
function selectorAt(css: string, index: number): string {
  const open = css.lastIndexOf('{', index)
  if (open < 0) return ''
  const start = Math.max(css.lastIndexOf('}', open), css.lastIndexOf('{', open - 1), -1)
  return css
    .slice(start + 1, open)
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .trim()
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
    // 有实色背景（主题 token），才能从彩色灯点上"抠"出来。
    expect(decl(badge, 'background')).toBe('var(--agent-status-badge-bg)')
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

  it('switches lamp chrome tokens between dark and light themes', () => {
    const css = agentStatusCss()
    const base = ruleBody(css, '.agent-status-module')
    const light = ruleBody(css, ":root[data-theme='light'] .agent-status-module")

    // Default dark theme keeps the current glass-on-dark edge treatment.
    expect(base, 'must declare status-lamp theme variables').not.toBe('')
    expect(decl(base, '--agent-status-badge-bg')).toBe('var(--ufo-bg-main)')
    expect(decl(base, '--agent-status-dot-ring')).toBe('rgba(255, 255, 255, 0.4)')
    expect(decl(base, '--agent-status-ring-shadow')).toBe('rgba(0, 0, 0, 0.45)')

    // Light theme only swaps chrome tokens; sizes and layout stay untouched.
    expect(light, 'light theme must override agent status chrome').not.toBe('')
    expect(decl(light, '--agent-status-badge-bg')).toBe('var(--ufo-surface)')
    expect(decl(light, '--agent-status-badge-border')).toBe('var(--ufo-border-strong)')
    expect(decl(light, '--agent-status-dot-ring')).toBe('var(--ufo-border-strong)')
    expect(decl(light, '--agent-status-ring-shadow')).toContain('var(--ufo-text)')
    expect(decl(light, '--agent-status-overflow-core-bg')).toBe('var(--ufo-surface)')
    expect(decl(light, '--agent-status-overflow-core-inset')).toContain('var(--ufo-text)')
    expect(decl(light, '--agent-status-idle-icon-opacity')).toBe('0.56')
    expect(light).not.toContain('width:')
    expect(light).not.toContain('height:')

    // Concrete elements consume the theme variables instead of hard-coded dark-theme assumptions.
    const badge = ruleBody(css, '.agent-status-module__identity')
    expect(decl(badge, 'background')).toBe('var(--agent-status-badge-bg)')
    expect(decl(badge, 'border')).toContain('var(--agent-status-badge-border)')
    expect(decl(ruleBody(css, '.agent-status-module__dot'), 'border-color')).toBe('var(--agent-status-dot-ring)')
    expect(decl(ruleBody(css, '.agent-status-module__ring'), 'filter')).toContain('var(--agent-status-ring-shadow)')

    const core = ruleBody(css, '.agent-status-module__overflow-core')
    expect(decl(core, 'background')).toBe('var(--agent-status-overflow-core-bg)')
    expect(decl(core, 'box-shadow')).toContain('var(--agent-status-overflow-core-inset)')
    expect(decl(ruleBody(css, '.agent-status-module__idle-icon'), 'opacity')).toBe(
      'var(--agent-status-idle-icon-opacity)'
    )
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
    expect(decl(icon, 'opacity')).toBe('var(--agent-status-idle-icon-opacity)')
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

/**
 * 设置页「卡片撑满列宽」护栏。
 *
 * 卡片基类 `.ufo-card`（`src/assets/main.css`）自带 `max-w-md`，而模块 CSS 打包时排在入口 CSS
 * **之前**（见构建产物里 `.ufo-card{…max-width:var(--container-md)…}` 的位置）。
 * 于是单类覆盖（`.push-master`）与它同权重、由加载顺序决定胜负 —— 实测「消息推送」上半区
 * 就这样缩回 448px，而下半区两栏正常（它们本来就是 `.push-split > .ufo-card`）。
 * 用户明确要求过「上面的板块占满全部宽度」，所以把权重口径钉在这里，不让它靠顺序碰运气。
 */
describe('模块样式 · 设置页卡片必须真的撑满列宽', () => {
  it('gives every max-w-none override at least two class tokens', () => {
    // 注释里也写着选择器与类名，先剥掉再扫，否则会把注释文字当成选择器。
    const css = agentStatusCss().replace(/\/\*[\s\S]*?\*\//g, '')
    const weak: string[] = []
    let overrides = 0

    for (const match of css.matchAll(/max-w-none/g)) {
      overrides += 1
      const selector = selectorAt(css, match.index)
      const list = selector
        .split(',')
        .map(item => item.trim())
        .filter(Boolean)
      // 「两类权重」= 选择器里至少两个类令牌（`.ufo-card.xxx` 或「容器 > .ufo-card」）。
      if (!list.length || list.some(item => (item.match(/\./g) ?? []).length < 2)) {
        weak.push(selector || '<找不到选择器>')
      }
    }

    // 用例本身别退化成「一条都没扫到」的空断言。
    expect(overrides, '模块 CSS 里应有放开卡片限宽的覆盖').toBeGreaterThanOrEqual(4)
    expect(weak, `这些 max-w-none 撑不过 .ufo-card 的限宽：${weak.join(' / ')}`).toEqual([])
  })

  it('keeps the push master card full width and the two-zone row split', () => {
    const css = agentStatusCss()
    // 上半区（总开关那块）必须是「容器 > 卡片」形式才撑得满（见上一条权重用例）。
    const override = blockAfter(css, '.push-panel > .push-master,')
    expect(override, '推送上半区必须有 .push-panel > .push-master 的限宽覆盖').toContain('max-w-none')

    // 上半区按内容高：它只剩两组勾选，写死 40% 会在中间留一大片空白。
    expect(decl(ruleBody(css, '.push-panel'), 'grid-template-rows')).toBe('auto minmax(0, 1fr)')
  })

  it('lays the topic rows out as name / topic / remove', () => {
    const css = agentStatusCss()
    // 三列：名字 | 主题串 | 删除按钮。用户要求「左边名字、右边主题值」，顺序反了就错。
    // 三格等高：行不能写 `align-items: center`，否则右侧删除按钮按自身内容高缩成一小块、占不满整行。
    const topic = ruleBody(css, '.push-topic')
    expect(decl(topic, 'grid-template-columns')).toBe('minmax(0, 1fr) minmax(0, 1.4fr) auto')
    expect(decl(topic, 'align-items'), '删除按钮要靠 grid 默认的 stretch 占满行高').not.toBe('center')
    // 主题串等宽：要能逐字核对（也便于发现粘进去的空格）。
    // ⚠️ 选择器必须带 `.push-topic input` 前缀才压得住那条元素选择器（(0,1,1) > 单类 (0,1,0)），
    // 写成单类就是「声明看着对、渲染没生效」—— mock 里踩过。
    expect(decl(ruleBody(css, '.push-topic input.push-topic__value'), 'font-family')).toBe(
      'var(--font-mono)'
    )
  })

  it('pins the push cards to the top instead of stretching their rows', () => {
    const css = agentStatusCss()
    // 卡片基类 `.ufo-card` 是 grid，而 `main.css` 排在模块 CSS **之后**：卡里写的 `flex` 争不过它。
    // grid 的 `align-content` 默认是 normal（= stretch），两行内容会被拉出一大片空白 ——
    // 所以三张卡都必须显式钉 `content-start`。
    for (const header of ['.push-master {', '.push-list-card,']) {
      const body = blockAfter(css, header)
      expect(body, `${header} 规则必须存在`).not.toBe('')
      expect(body, `${header} 必须钉住 content-start（否则 grid 会把行拉开）`).toContain('content-start')
    }
  })

  /**
   * 转灯日志的版面护栏（用户在 mock 上连着反馈过两次的那两条）。
   *
   * ① 列表类的 grid 必须钉 `align-content: start`：容器吃满高度时 grid 默认 stretch，
   *    会把富余摊给每一行 —— 行与行之间拉出大片空白（mock 上实测行被撑到 105px）。
   * ② 会话卡片定高 + 提问只给一行：这是「行高一致、滚动位置可预测」的约定。
   * ③ 浮层的高度链：bar 固定 → body `min-h-0` + `flex-1` → 两栏各自滚，少一环就退化成整页滚。
   */
  it('keeps the transition-log lists pinned to the top and the modal chain unbroken', () => {
    const css = agentStatusCss()
    // `.sr-list` 用的是普通声明（align-content: start），`.lg-rows` 走 @apply content-start：两种写法等价。
    for (const header of ['.sr-list {', '.lg-rows {']) {
      const body = blockAfter(css, header)
      expect(body, `${header} 规则必须存在`).not.toBe('')
      expect(
        body.includes('content-start') || body.includes('align-content: start'),
        `${header} 必须钉住行对齐，否则 grid 会把行拉开`
      ).toBe(true)
    }

    const row = blockAfter(css, '.sr-list__row {')
    expect(row, '会话卡片要定高（行高一致才谈得上滚动位置可预测）').toContain('min-height: 52px')
    // ⚠️ truncate 要生效，收缩必须从**网格项**这一层放行：`.sr-list` 是 grid，`.sr-list__item`
    // 的 min-width 默认 auto 会把列轨钉在提问那句 nowrap 文本的固有宽度上（无头 Chrome 实测
    // 224px 的列被顶到 1012px），结果是列表出横向滚动条、省略号永远不出现。
    const item = blockAfter(css, '.sr-list__item {')
    expect(item, '列表项必须 min-w-0，否则提问不会省略号而是把列表撑出横向滚动').toContain('min-w-0')
    const prompt = blockAfter(css, '.sr-list__prompt {')
    expect(prompt, '提问摘要只给一行').toContain('truncate')
    expect(prompt, '不许再把提问改回两行夹行高').not.toContain('line-clamp-2')

    // 满宽设置卡：突破 .ufo-card 限宽必须有两条类权重（同 `.push-panel > .push-master`）。
    expect(css, '设置卡要占满页宽').toContain('.sr-page > .lg-settings')

    const modalBody = blockAfter(css, '.lg-modal__body {')
    expect(modalBody, '浮层主体要吃满剩余高度且允许收缩').toContain('min-h-0')
    expect(modalBody).toContain('flex-1')
    // ⚠️ 浮层必须 `absolute`（宿主 = 主窗内容区 `#main-content`）：`fixed` 铺的是视口，
    // 实测既盖住顶部两级标签页，又把方角画到 `.main-shell` 圆角之外（1000×750 的窗，
    // 浮量到 1249×1221）。
    const modal = blockAfter(css, '.lg-modal {')
    expect(modal, '整窗浮层要贴着内容区铺，不许按视口').toContain('absolute')
    expect(modal, '不许改回 fixed：那会盖住标签页并伸出窗口圆角').not.toContain('fixed')
    const split = blockAfter(css, '.lg-split {')
    expect(split, '浮层两栏都要能独立滚').toContain('min-h-0')
  })

  /**
   * 「记录详情」卡的三块必须按**卡片实际高度**分配（用户反馈：三块都固定高，卡底空一片）。
   *
   * 口径：详情卡是 flex 列；只有「最近提问」拉伸（`flex-1` + `min-h-0`，长文在块内滚），
   * 字段表 / 日志条目等一律按内容高。⚠️ 提问正文不许退回视口高的固定上限（原先那个
   * `max-h` 夹的是视口的 22%）—— 那正是「卡很高、提问区却很矮、剩下全空白」的成因。
   */
  it('grows the record-detail prompt with the card instead of capping it to the viewport', () => {
    const css = agentStatusCss()
    // ⚠️ 两类权重：`.ufo-card` 的 `@apply grid` 未分层，单类 `.sr-detail { @apply flex }` 压不过它
    // （见 `src/assets/main.css` 卡片基类注释）。写成单类时卡其实还是 grid，`flex-1` 全失效，
    // 而且 `align-content: normal`（= stretch）会把富余高度平摊给每一行 —— 多一行 / 少一行
    // 所有元素高度一起重算，就是「切会话时详情卡的块跳高」（无头 Chrome 实测四行 45.6/116.9/122.1/137.4）。
    expect(/\.ufo-card\.sr-detail \{[^}]*@apply[^;]*\bflex flex-col\b/.test(css), '.sr-detail 必须两类权重才打得过 grid').toBe(
      true
    )
    const fixed = blockAfter(css, '.sr-detail > :not(.sr-detail__prompt) {')
    expect(fixed, '提问块以外的详情子项都不许被拉伸').toContain('shrink-0')

    const prompt = blockAfter(css, '.sr-detail__prompt {')
    expect(prompt).toContain('flex-1')
    expect(prompt, '矮窗口要给提问留可视下限，否则会塌成一条线').toContain('min-h-[96px]')
    expect(prompt, '正文那行才是一fr，标签行按内容高').toContain('auto minmax(0, 1fr)')

    const text = blockAfter(css, '.sr-detail__prompt-text {')
    expect(text).toContain('min-h-0')
    expect(text, '滚动落在正文自己').toContain('overflow-y-auto')
    expect(text, '不许再用 vh 把提问夹成视口的固定比例').not.toMatch(/vh/)
  })

  /**
   * 存活提醒只能待在标题行，且只能占一行（用户要求：提醒移到标题旁，提问块高度按已归档那条定死）。
   *
   * 它原先是正文里的一个兄弟节点：flex 列里多一行 = 提问块少一行，选中「存活」与「已归档」
   * 两类会话之间整卡跳高。搬进标题行后仍有一条新坑 —— 标题行是 flex，
   * 提醒与右侧 `provider · session id` 都会跟着被挤，挤到换行就等于把跳高搬回卡头（实测 45.6 → 60.3px）。
   * 所以：提醒自己 `min-w-0 truncate`，标题与右侧读数 `shrink-0 whitespace-nowrap` 锁死。
   */
  it('keeps the live-session reminder on the title line and one line tall', () => {
    const css = agentStatusCss()
    // 列表卡同一条 grid 权重规矩（它也是 `.ufo-card`）。
    expect(/\.ufo-card\.sr-list-card \{[^}]*@apply[^;]*\bflex flex-col\b/.test(css), '.sr-list-card 必须两类权重').toBe(
      true
    )

    const live = blockAfter(css, '.sr-detail__live {')
    expect(live, '存活提醒规则必须存在').not.toBe('')
    expect(live, '提醒要独占标题行的富余，且放得下不换行').toContain('mr-auto')
    expect(live).toContain('min-w-0')
    expect(live, '提醒只能截断，不能换行加高卡头').toContain('truncate')

    const locked = blockAfter(css, '.sr-detail > .ufo-card__header > .ufo-card__title,')
    expect(locked, '标题行的另外两块要锁死').toContain('shrink-0')
    expect(locked).toContain('whitespace-nowrap')
  })

  /**
   * 「最近提问」是 `flex-1`，高 = 卡片高 − 其余各块 ⇒ 要它固定，另外几块必须不随行数变。
   *
   * 用户反馈：切换选中会话时详情卡的元素高度会跳。两处成因（都按真实数据量过）：
   * ① 字段表的「Token 累计」原来是 `v-if`（有无统计值差一行），且值按字符断行（长 endpoint 两行）；
   * ② 转灯日志折叠块原来是内容高：日志关闭 / 只有 1 条 / 正常两条摘要 / 导出后带反馈，
   *   四种状态实测 85.6 / 104 / 124.8 / 193.1px，把提问块在 158.7–242.1 之间顶来顶去。
   * 口径：字段表一行一条（`truncate`，全文进 `title`），折叠块钉成定高槽、装不下在块内滚。
   */
  it('pins the blocks around the prompt so its height cannot move', () => {
    const css = agentStatusCss()
    const dd = blockAfter(css, '.sr-detail__fields dd {')
    expect(dd, '字段表的值只占一行，否则提问块会被顶矮').toContain('truncate')
    expect(dd, '不许再让长值断行加高字段表').not.toContain('overflow-wrap')

    const inline = blockAfter(css, '.lg-inline {')
    expect(inline, '折叠块要钉死高度').toMatch(/h-\[\d+px\]/)
    expect(inline, '钉死的高度要允许块内滚，否则内容会被裁掉').toContain('overflow-y-auto')
    expect(inline).toContain('shrink-0')
    expect(blockAfter(css, '.lg-inline__head {'), '折叠块卡头不许换行（会把动作钮挤出定高槽）').toContain(
      'flex-nowrap'
    )

    // ⚠️ 钉高只是前半段：nowrap 文本仍会把 grid 轨道顶宽（实测长 endpoint 把详情卡顶出
    // 横向滚动条，滚动条再吃掉 10px 纵向高 → 提问块 202.8 ↔ 192.8）。收缩必须逐层放行。
    expect(blockAfter(css, '.sr-detail__fields {'), '字段表自己要不许被内容顶宽').toContain('min-w-0')
    expect(blockAfter(css, '.sr-detail__fields > div {'), '字段表的每一行要不许被内容顶宽').toContain('min-w-0')
    const shrinkable = blockAfter(css, '.lg-inline > *,')
    expect(shrinkable, '折叠块 / 提问块里的 grid 项都要 min-w-0').toContain('min-w-0')
    // 选择器写在规则头而不是规则体里，所以要对着原文查这三条都在同一条规则上。
    expect(css, '折叠块内的每一行摘要也要放行收缩').toMatch(
      /\.lg-inline > \*,\s*\.lg-inline__rows > li,\s*\.sr-detail__prompt > \*/
    )
  })
})
