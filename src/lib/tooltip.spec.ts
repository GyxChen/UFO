import { describe, expect, it } from 'vitest'
import type { DockedEdges } from './snap'
import { placeTooltip } from './tooltip'

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
    const result = placeTooltip(
      { x: 400, y: 400, width: 56, height: 56 },
      tooltipSize,
      workArea,
      free,
    )
    // x = 400 + 56 + 8；y 与悬浮窗垂直居中：400 + (56 - 132) / 2 = 362
    expect(result).toEqual({ position: { x: 464, y: 362 }, side: 'right' })
  })

  it('keeps the tooltip on the right when docked to the left edge', () => {
    const result = placeTooltip(
      { x: 0, y: 300, width: 56, height: 400 },
      tooltipSize,
      workArea,
      dockedTo('left'),
    )
    expect(result.side).toBe('right')
    expect(result.position.x).toBe(64)
  })

  it('flips to the left when docked to the right edge', () => {
    const result = placeTooltip(
      { x: 1864, y: 300, width: 56, height: 400 },
      tooltipSize,
      workArea,
      dockedTo('right'),
    )
    // 往左：1864 - 300 - 8 = 1556，仍完整落在工作区内。
    expect(result).toEqual({ position: { x: 1556, y: 434 }, side: 'left' })
  })

  it('drops below when docked to the top edge', () => {
    const result = placeTooltip(
      { x: 600, y: 0, width: 320, height: 56 },
      tooltipSize,
      workArea,
      dockedTo('top'),
    )
    // y = 0 + 56 + 8；x 与横条居中：600 + (320 - 300) / 2 = 610
    expect(result).toEqual({ position: { x: 610, y: 64 }, side: 'bottom' })
  })

  it('flips above when docked to the bottom edge', () => {
    const result = placeTooltip(
      { x: 600, y: 984, width: 320, height: 56 },
      tooltipSize,
      workArea,
      dockedTo('bottom'),
    )
    // 往上：984 - 132 - 8 = 844
    expect(result).toEqual({ position: { x: 610, y: 844 }, side: 'top' })
  })

  it('prefers the side away from both docked edges in a corner', () => {
    // 左下角：贴 left + bottom → 应优先往右或往上，而不是往左 / 往下。
    const result = placeTooltip(
      { x: 0, y: 984, width: 112, height: 112 },
      tooltipSize,
      workArea,
      { left: true, top: false, right: false, bottom: true },
    )
    expect(result.side).toBe('right')
    expect(result.position.x).toBe(120)
  })

  it('flips away from the screen edge when the preferred side does not fit', () => {
    // 未贴边但紧贴右侧：往右放不下，应翻到左侧。
    const result = placeTooltip(
      { x: 1830, y: 400, width: 56, height: 56 },
      tooltipSize,
      workArea,
      free,
    )
    expect(result.side).toBe('left')
    expect(result.position.x).toBe(1522)
  })

  it('flips below when there is no room above', () => {
    // 未贴边但靠近顶部：首选右侧若放不下才轮到下方；此处右侧可用，
    // 用贴下边的锚点验证「往上放不下 → 往下」。
    const result = placeTooltip(
      { x: 900, y: 8, width: 56, height: 56 },
      tooltipSize,
      workArea,
      dockedTo('bottom'),
    )
    // 往上：8 - 132 - 8 = -132 越界 → 应改用下方。
    expect(result.side).toBe('bottom')
    expect(result.position.y).toBe(72)
  })

  it('clamps into the work area when the tooltip is larger than the screen', () => {
    const tiny = { position: { x: 0, y: 0 }, size: { width: 200, height: 150 } }
    const result = placeTooltip(
      { x: 100, y: 100, width: 56, height: 56 },
      { width: 300, height: 200 },
      tiny,
      free,
    )
    // 放不下时固定在工作区左上角留白处，而不是越出到负坐标。
    expect(result.position).toEqual({ x: 4, y: 4 })
  })

  it('handles negative work area coordinates on a secondary monitor', () => {
    const secondary = {
      position: { x: -1920, y: 0 },
      size: { width: 1920, height: 1040 },
    }
    const result = placeTooltip(
      { x: -1920, y: 400, width: 56, height: 56 },
      tooltipSize,
      secondary,
      dockedTo('left'),
    )
    // 贴副屏左边缘 → 往右；坐标基于副屏原点，不假定从 0 开始。
    expect(result).toEqual({ position: { x: -1856, y: 362 }, side: 'right' })
  })

  it('rounds fractional positions to integers', () => {
    const result = placeTooltip(
      { x: 400, y: 401, width: 57, height: 57 },
      tooltipSize,
      workArea,
      free,
    )
    expect(Number.isInteger(result.position.x)).toBe(true)
    expect(Number.isInteger(result.position.y)).toBe(true)
  })
})
