// @vitest-environment happy-dom

/**
 * 三形态预览（方案 C）的回归测试。
 *
 * 锁四件事：① 当前分组进主图、其余两态进缩略排；② 像素读数用的是**配置里那一格**
 * （不是写死的 45）；③ 图例逐模块给需求格数、颜色取自模块定义的 `tint`（外壳只透传不解释）；
 * ④ 缩放后不超出画布 / 缩略盒的上限。
 */
import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import css from '../../assets/main.css?raw'
import FloatingShapePreview from './FloatingShapePreview.vue'
import type { FloatingModuleDefinition, FloatingRuntimeSnapshot } from '@/lib/floating-modules'

/** 主图画布的兜底上限（环境没有布局时组件用这套，见组件里的 FALLBACK_CANVAS）。 */
const CANVAS_MAX = { width: 196, height: 150 }
/** 缩略盒的上限（见组件里的 THUMB_BOX）。 */
const THUMB_MAX = { width: 62, height: 44 }

/** 两个假模块：各自声明预览身份色。 */
const definitions: FloatingModuleDefinition[] = [
  {
    id: 'stub',
    label: '占位模块',
    description: '占位模块',
    tint: 'var(--ufo-status-green)',
    version: 1,
    defaultEnabled: true,
    supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
    component: {} as never,
  },
  {
    id: 'other',
    label: '另一个模块',
    description: '另一个模块',
    tint: 'var(--ufo-status-blue)',
    version: 1,
    defaultEnabled: true,
    supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
    component: {} as never,
  },
]

/** 构造运行时快照：逐形态给出「模块 → 需求格数」，单模块时占 `cells` 格。 */
function snapshotOf(options: {
  layout: FloatingRuntimeSnapshot['layout']
  demands?: Record<string, number>
  cells?: number
}): FloatingRuntimeSnapshot {
  const demands = options.demands ?? { stub: options.cells ?? 4 }
  const perLayout = { demands, windowCells: Object.values(demands).reduce((sum, value) => sum + value, 0) }
  return {
    layout: options.layout,
    byLayout: { square: perLayout, corner: perLayout, horizontal: perLayout, vertical: perLayout },
  }
}

/** 挂载预览并返回三类查询：主图窗口、缩略排、文本读数。 */
function mounted(options: Parameters<typeof snapshotOf>[0], cellPx: number) {
  const wrapper = mount(FloatingShapePreview, {
    props: { snapshot: snapshotOf(options), definitions, cellPx },
  })
  return {
    wrapper,
    heroWindow: wrapper.get('.shape-preview__canvas .shape-preview__window'),
    thumbs: wrapper.findAll('.shape-preview__thumb'),
    readout: wrapper.get('.shape-preview__readout').text(),
    legendRows: wrapper.findAll('.shape-preview__row'),
  }
}

/** 读元素内联样式里的像素尺寸。 */
function drawnSize(element: { element: HTMLElement }) {
  return {
    width: Number.parseFloat(element.element.style.width),
    height: Number.parseFloat(element.element.style.height),
  }
}

/** 取入口样式表里某条规则的正文；只查声明文本，不做布局。 */
function ruleBody(selector: string) {
  const start = css.indexOf(`${selector} {`)
  expect(start, `main.css 里找不到 ${selector}`).toBeGreaterThan(-1)
  return css.slice(start, css.indexOf('}', start))
}

describe('FloatingShapePreview · 方案 C', () => {
  it('把当前形态画进主图，其余两态降成缩略', () => {
    const { wrapper, thumbs } = mounted({ layout: 'horizontal' }, 45)
    expect(wrapper.get('.shape-preview__canvas .shape-preview__window').classes()).toContain(
      'shape-preview__window--horizontal'
    )
    expect(wrapper.get('.shape-preview__title').text()).toContain('横条')
    // 三组里除去当前那一组，缩略排正好两个；预览不再列方形。
    expect(thumbs).toHaveLength(2)
    expect(thumbs.map(thumb => thumb.get('.shape-preview__thumb-name').text())).toEqual(['四角', '竖条'])
    expect(thumbs.some(thumb => thumb.text().includes('方形'))).toBe(false)
  })

  it('reads pixel sizes off the configured cell instead of a hardcoded 45', () => {
    // 单模块 4 格 → 四角是它自己的 2×2 阶格：64 的一格就是 128×128。
    const { readout } = mounted({ layout: 'corner', cells: 4 }, 64)
    expect(readout).toContain('2×2')
    expect(readout).toContain('128×128px')
  })

  it('draws the undocked window as one cell inside the corner group', () => {
    // 未贴边折进「四角」那一组，但整窗仍只有一格：需求再多也只画 1×1，且不再叫「方形」。
    const { readout, wrapper } = mounted({ layout: 'square', cells: 4 }, 45)
    expect(wrapper.get('.shape-preview__title').text()).toContain('四角 · 未贴边')
    expect(readout).toContain('1×1')
    expect(readout).toContain('45×45px')
    expect(wrapper.findAll('.shape-preview__thumb').map(thumb => thumb.get('.shape-preview__thumb-name').text())).toEqual(['横条', '竖条'])
  })

  it('lists every module with its demand and passes its tint through', () => {
    const { legendRows, wrapper } = mounted({ layout: 'corner', demands: { stub: 2, other: 1 } }, 45)
    expect(
      legendRows.map(row => [row.get('.shape-preview__row-name').text(), row.get('.shape-preview__cells').text()])
    ).toEqual([
      ['占位模块', '2 格'],
      ['另一个模块', '1 格'],
    ])
    // 色块与图例点的颜色都来自模块定义，外壳不解释取值。
    const block = wrapper.get('.shape-preview__canvas .shape-preview__block')
    expect(block.element.style.getPropertyValue('--block-tint')).toBe('var(--ufo-status-green)')
  })

  it('fits the hero and the thumbs into their boxes at the largest cell size', () => {
    const { heroWindow, thumbs } = mounted({ layout: 'corner', cells: 8 }, 96)
    // 8 格单模块 → 四角 3×3 格 = 288×288，必须缩进画布。
    const hero = drawnSize(heroWindow)
    expect(hero.width).toBeLessThanOrEqual(CANVAS_MAX.width)
    expect(hero.height).toBeLessThanOrEqual(CANVAS_MAX.height)
    for (const thumb of thumbs) {
      const box = drawnSize(thumb.get('.shape-preview__window'))
      expect(box.width).toBeLessThanOrEqual(THUMB_MAX.width)
      expect(box.height).toBeLessThanOrEqual(THUMB_MAX.height)
    }
  })

  it('shows an empty hint until the runtime snapshot arrives', () => {
    const wrapper = mount(FloatingShapePreview, { props: { snapshot: null, definitions, cellPx: 45 } })
    expect(wrapper.get('.shape-preview__empty').text()).toContain('正在读取')
    expect(wrapper.find('.shape-preview__hero').exists()).toBe(false)
  })

  it('lets the name wrap instead of claiming width, with the line budget off the block height', () => {
    // 竖条 2:1 分槽：整窗缩到画布里是 50×150，两块的宽度都只有约 50px。
    const { wrapper } = mounted({ layout: 'vertical', demands: { stub: 2, other: 1 } }, 45)
    const blocks = wrapper.findAll('.shape-preview__canvas .shape-preview__block')
    expect(blocks).toHaveLength(2)
    // 行预算 = 块高能装几行（8 行的块 vs 4 行的块），而不是永远一行。
    const lines = blocks.map(block => Number(block.element.style.getPropertyValue('--label-lines')))
    expect(lines[0]).toBeGreaterThan(1)
    expect(lines[1]).toBeLessThan(lines[0])
    // 名字仍然渲染（能折行就放得下），且颜色与权重照旧走内联样式。
    expect(blocks[0].get('.shape-preview__block-label').text()).toBe('占位模块')
    expect(blocks[0].element.style.flexGrow).toBe('2')
  })

  it('pins the block sizes to the demand weights in CSS', () => {
    // 护栏：块的大小只由内联 flex-grow（= 需求格数）决定。basis 一旦回到 auto，
    // 长名字的块就会被标签的固有宽度顶宽，预览的「真实比例」立刻是假的。
    expect(ruleBody('.shape-preview__block')).toContain('flex-basis: 0')
    const label = ruleBody('.shape-preview__block-label')
    expect(label).not.toContain('whitespace-nowrap')
    expect(label).toContain('-webkit-line-clamp')
    expect(label).toContain('overflow-wrap: anywhere')
  })
})
