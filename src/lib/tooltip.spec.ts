import { describe, expect, it } from 'vitest'
import type { Component } from 'vue'
import type { DockedEdges } from './snap'
import type { FloatingHoverTarget } from './floating-module-context'
import type { FloatingModuleDefinition } from './floating-modules'
import { clampTooltipSize, placeTooltip, resolveTooltipDefinition, resolveTooltipSize } from './tooltip'

/** 只用来占位的假组件：解析器不关心组件内容，只关心模块有没有声明信息卡。 */
const stubComponent: Component = { template: '<span />' }

/** 造一个模块定义；`withTooltip` 决定它是否自带信息卡。 */
function definition(id: string, withTooltip: boolean): FloatingModuleDefinition {
  return {
    id,
    label: id,
    description: '',
    version: 1,
    defaultEnabled: false,
    supportedLayouts: ['square'],
    component: stubComponent,
    ...(withTooltip ? { tooltip: { component: stubComponent } } : {}),
  }
}

/** 1920×1040 工作区，与 snap.spec.ts 保持一致。 */
const workArea = {
  position: { x: 0, y: 0 },
  size: { width: 1920, height: 1040 },
}

const tooltipSize = { width: 300, height: 132 }

/** 未贴边。 */
const free: DockedEdges = { left: false, top: false, right: false, bottom: false }

/** 只贴某一条边，便于逐个方向断言。 */
function dockedTo(edge: keyof DockedEdges): DockedEdges {
  return { left: false, top: false, right: false, bottom: false, [edge]: true }
}

describe('placeTooltip', () => {
  it('places the tooltip to the right when the floating window is free', () => {
    const result = placeTooltip({ x: 400, y: 400, width: 56, height: 56 }, tooltipSize, workArea, free)
    // x = 400 + 56 + 8；y 与悬浮窗垂直居中：400 + (56 - 132) / 2 = 362
    expect(result).toEqual({ position: { x: 464, y: 362 }, side: 'right' })
  })

  it('keeps the tooltip on the right when docked to the left edge', () => {
    const result = placeTooltip({ x: 0, y: 300, width: 56, height: 400 }, tooltipSize, workArea, dockedTo('left'))
    expect(result.side).toBe('right')
    expect(result.position.x).toBe(64)
  })

  it('flips to the left when docked to the right edge', () => {
    const result = placeTooltip({ x: 1864, y: 300, width: 56, height: 400 }, tooltipSize, workArea, dockedTo('right'))
    // 往左：1864 - 300 - 8 = 1556，仍完整落在工作区内。
    expect(result).toEqual({ position: { x: 1556, y: 434 }, side: 'left' })
  })

  it('drops below when docked to the top edge', () => {
    const result = placeTooltip({ x: 600, y: 0, width: 320, height: 56 }, tooltipSize, workArea, dockedTo('top'))
    // y = 0 + 56 + 8；x 与横条居中：600 + (320 - 300) / 2 = 610
    expect(result).toEqual({ position: { x: 610, y: 64 }, side: 'bottom' })
  })

  it('flips above when docked to the bottom edge', () => {
    const result = placeTooltip({ x: 600, y: 984, width: 320, height: 56 }, tooltipSize, workArea, dockedTo('bottom'))
    // 往上：984 - 132 - 8 = 844
    expect(result).toEqual({ position: { x: 610, y: 844 }, side: 'top' })
  })

  it('prefers the side away from both docked edges in a corner', () => {
    // 左下角：贴 left + bottom → 应优先往右或往上，而不是往左 / 往下。
    const result = placeTooltip({ x: 0, y: 984, width: 112, height: 112 }, tooltipSize, workArea, {
      left: true,
      top: false,
      right: false,
      bottom: true,
    })
    expect(result.side).toBe('right')
    expect(result.position.x).toBe(120)
  })

  it('flips away from the screen edge when the preferred side does not fit', () => {
    // 未贴边但紧贴右侧：往右放不下，应翻到左侧。
    const result = placeTooltip({ x: 1830, y: 400, width: 56, height: 56 }, tooltipSize, workArea, free)
    expect(result.side).toBe('left')
    expect(result.position.x).toBe(1522)
  })

  it('flips below when there is no room above', () => {
    // 未贴边但靠近顶部：首选右侧若放不下才轮到下方；此处右侧可用，
    // 用贴下边的锚点验证「往上放不下 → 往下」。
    const result = placeTooltip({ x: 900, y: 8, width: 56, height: 56 }, tooltipSize, workArea, dockedTo('bottom'))
    // 往上：8 - 132 - 8 = -132 越界 → 应改用下方。
    expect(result.side).toBe('bottom')
    expect(result.position.y).toBe(72)
  })

  it('clamps into the work area when the tooltip is larger than the screen', () => {
    const tiny = { position: { x: 0, y: 0 }, size: { width: 200, height: 150 } }
    const result = placeTooltip({ x: 100, y: 100, width: 56, height: 56 }, { width: 300, height: 200 }, tiny, free)
    // 放不下时固定在工作区左上角留白处，而不是越出到负坐标。
    expect(result.position).toEqual({ x: 4, y: 4 })
  })

  it('handles negative work area coordinates on a secondary monitor', () => {
    const secondary = {
      position: { x: -1920, y: 0 },
      size: { width: 1920, height: 1040 },
    }
    const result = placeTooltip({ x: -1920, y: 400, width: 56, height: 56 }, tooltipSize, secondary, dockedTo('left'))
    // 贴副屏左边缘 → 往右；坐标基于副屏原点，不假定从 0 开始。
    expect(result).toEqual({ position: { x: -1856, y: 362 }, side: 'right' })
  })

  it('rounds fractional positions to integers', () => {
    const result = placeTooltip({ x: 400, y: 401, width: 57, height: 57 }, tooltipSize, workArea, free)
    expect(Number.isInteger(result.position.x)).toBe(true)
    expect(Number.isInteger(result.position.y)).toBe(true)
  })
})

describe('resolveTooltipDefinition', () => {
  const definitions = [definition('agent-status', true), definition('plain', false)]

  it('picks the module that declares a tooltip', () => {
    const target: FloatingHoverTarget = { moduleId: 'agent-status', key: 'session-a' }
    expect(resolveTooltipDefinition(definitions, target)?.id).toBe('agent-status')
  })

  it('returns null when the hovered module has no tooltip', () => {
    // 没声明信息卡的模块（如纯展示型）：悬停不弹提示窗，而不是弹别人的内容。
    expect(resolveTooltipDefinition(definitions, { moduleId: 'plain', key: 'x' })).toBeNull()
  })

  it('returns null for an unknown module id', () => {
    expect(resolveTooltipDefinition(definitions, { moduleId: 'ghost', key: 'x' })).toBeNull()
  })

  it('returns null without a target', () => {
    expect(resolveTooltipDefinition(definitions, null)).toBeNull()
    expect(resolveTooltipDefinition(definitions, undefined)).toBeNull()
  })

  it('dispatches by module id, not by key', () => {
    // 两个模块用同一个 key 也不会串台：分派只看 moduleId。
    const both = [definition('agent-status', true), definition('other', true)]
    expect(resolveTooltipDefinition(both, { moduleId: 'other', key: 'session-a' })?.id).toBe('other')
  })

  it('ignores modules that only declare a tooltip without a component', () => {
    const broken = [{ ...definition('broken', true), tooltip: {} as FloatingModuleDefinition['tooltip'] }]
    expect(resolveTooltipDefinition(broken, { moduleId: 'broken', key: 'x' })).toBeNull()
  })
})

describe('resolveTooltipSize', () => {
  const target: FloatingHoverTarget = { moduleId: 'agent-status', key: 'session-a' }

  it('returns the declared size as-is', () => {
    expect(resolveTooltipSize({ component: stubComponent, size: { width: 300, height: 260 } }, target)).toEqual({
      width: 300,
      height: 260,
    })
  })

  it('computes the size from the hover target when it is a function', () => {
    const tooltip = {
      component: stubComponent,
      // 折叠清单那类内容：行数随目标变化，尺寸得跟着目标现算。
      size: (item: FloatingHoverTarget) => ({ width: 300, height: item.key.length }),
    }
    expect(resolveTooltipSize(tooltip, target)?.height).toBe(target.key.length)
  })

  it('returns null when nothing is declared or there is no target', () => {
    // 没声明 = 沿用窗口默认尺寸；声明成函数但这次没有目标 = 也算不出，同样沿用默认。
    expect(resolveTooltipSize({ component: stubComponent }, target)).toBeNull()
    expect(resolveTooltipSize({ component: stubComponent, size: () => ({ width: 1, height: 1 }) }, null)).toBeNull()
    expect(resolveTooltipSize(undefined, target)).toBeNull()
  })
})

describe('clampTooltipSize', () => {
  it('clamps a taller-than-screen card into the work area', () => {
    // 折叠几十个会话时算出来的高度可能超过屏幕：窗口不能比工作区还高，超出的交给内容自己滚。
    const clamped = clampTooltipSize({ width: 300, height: 4000 }, { width: 1920, height: 1040 })
    expect(clamped).toEqual({ width: 300, height: 1040 - 8 })
  })

  it('keeps a reasonable floor for absurdly small sizes', () => {
    const clamped = clampTooltipSize({ width: 10, height: 10 }, { width: 1920, height: 1040 })
    expect(clamped.width).toBeGreaterThanOrEqual(200)
    expect(clamped.height).toBeGreaterThanOrEqual(88)
  })

  it('rounds fractional sizes to whole pixels', () => {
    const clamped = clampTooltipSize({ width: 300.4, height: 165.6 }, { width: 1920, height: 1040 })
    expect(clamped).toEqual({ width: 300, height: 166 })
  })

  it('falls back to the floor when the work area is tinier than the floor', () => {
    // 工作区比地板还小（极端情况）：取地板，位置由 placeTooltip 的夹取兜底。
    const clamped = clampTooltipSize({ width: 300, height: 300 }, { width: 160, height: 60 })
    expect(clamped).toEqual({ width: 200, height: 88 })
  })
})
