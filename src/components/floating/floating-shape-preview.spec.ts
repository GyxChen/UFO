// @vitest-environment happy-dom

/**
 * 三形态预览的回归测试。
 *
 * 预览的画布是固定尺寸的卡片（宽 250 / 高 132），所以任何形态都必须**等比缩进来**，
 * 一旦按配置把一格调大（用户可在「悬浮窗设置」里改 `cell_px`）就会出现超大窗口 ——
 * 这里锁「缩放后不超出画布」与「尺寸说明用的是配置里那一格」，避免只按默认 45 写死。
 */
import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import FloatingShapePreview from './FloatingShapePreview.vue'
import type { FloatingModuleDefinition, FloatingRuntimeSnapshot } from '@/lib/floating-modules'

/** 画布上限，与组件里的常量一致（卡片放不下时要缩，不是溢出）。 */
const MAX_WIDTH = 250
const MAX_HEIGHT = 132

const definitions: FloatingModuleDefinition[] = [
  {
    id: 'stub',
    label: '占位模块',
    description: '占位模块',
    version: 1,
    defaultEnabled: true,
    supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
    component: {} as never,
  },
]

/** 构造运行时快照：每个形态都是 `cells` 格、单模块。 */
function snapshotOf(cells: number): FloatingRuntimeSnapshot {
  const perLayout = { demands: { stub: cells }, windowCells: cells }
  return {
    layout: 'square',
    byLayout: { square: perLayout, corner: perLayout, horizontal: perLayout, vertical: perLayout },
  }
}

/** 挂载预览并取每个分组的「实际绘制尺寸」。 */
function drawnSizes(cellPx: number, cells = 4) {
  const wrapper = mount(FloatingShapePreview, {
    props: { snapshot: snapshotOf(cells), definitions, cellPx },
  })
  return wrapper.findAll<HTMLElement>('.shape-preview__window').map(element => ({
    width: Number.parseFloat(element.element.style.width),
    height: Number.parseFloat(element.element.style.height),
    note: element.element.parentElement?.parentElement?.querySelector('.shape-preview__size')?.textContent ?? '',
  }))
}

describe('FloatingShapePreview', () => {
  it('notes the configured cell size instead of a hardcoded one', () => {
    const [corner] = drawnSizes(64)
    // 4 格 → 四角边长 2 格：64 的窗口就是 128×128。
    expect(corner.note).toContain('未贴边 64×64')
    expect(corner.note).toContain('贴角 128×128px')
  })

  it('keeps every shape inside the preview canvas at the largest cell size', () => {
    for (const drawn of drawnSizes(96, 8)) {
      // 8 格 → 四角 3×3 格 = 288×288 的真实尺寸，必须缩到画布内。
      expect(drawn.width).toBeLessThanOrEqual(MAX_WIDTH)
      expect(drawn.height).toBeLessThanOrEqual(MAX_HEIGHT)
    }
  })

  it('draws small windows at their real size', () => {
    // 4 格 → 四角 2×2 格 = 64×64，放得下就不缩放（画布只在小窗口时保持 1:1）。
    const [corner, horizontal, vertical] = drawnSizes(32)
    expect([corner.width, corner.height]).toEqual([64, 64])
    expect([horizontal.width, horizontal.height]).toEqual([128, 32])
    expect([vertical.width, vertical.height]).toEqual([32, 128])
  })
})