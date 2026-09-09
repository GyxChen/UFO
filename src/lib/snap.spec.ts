import { describe, expect, it } from 'vitest'
import { snapToWorkArea } from './snap'

const workArea = {
  position: { x: 0, y: 0 },
  size: { width: 1920, height: 1040 },
}
const windowSize = { width: 80, height: 80 }
const baseSize = { width: 80, height: 80 }

describe('snapToWorkArea', () => {
  it('snaps to the left edge and expands along the edge', () => {
    expect(snapToWorkArea({ x: 12, y: 200 }, windowSize, baseSize, workArea)).toEqual({
      position: { x: 0, y: 200 },
      size: { width: 80, height: 320 },
      docked: { left: true, top: false, right: false, bottom: false },
    })
  })

  it('snaps to the right edge and expands along the edge', () => {
    expect(snapToWorkArea({ x: 1828, y: 200 }, windowSize, baseSize, workArea)).toEqual({
      position: { x: 1840, y: 200 },
      size: { width: 80, height: 320 },
      docked: { left: false, top: false, right: true, bottom: false },
    })
  })

  it('snaps to the top edge and expands along the edge', () => {
    expect(snapToWorkArea({ x: 200, y: 12 }, windowSize, baseSize, workArea)).toEqual({
      position: { x: 200, y: 0 },
      size: { width: 320, height: 80 },
      docked: { left: false, top: true, right: false, bottom: false },
    })
  })

  it('snaps to the bottom edge and expands along the edge', () => {
    expect(snapToWorkArea({ x: 200, y: 972 }, windowSize, baseSize, workArea)).toEqual({
      position: { x: 200, y: 960 },
      size: { width: 320, height: 80 },
      docked: { left: false, top: false, right: false, bottom: true },
    })
  })

  it('snaps corners independently and becomes a square', () => {
    // 放大为正方形后仍需紧贴两条边:左下角 y = 1040 - 160 = 880。
    expect(snapToWorkArea({ x: 10, y: 970 }, windowSize, baseSize, workArea)).toEqual({
      position: { x: 0, y: 880 },
      size: { width: 160, height: 160 },
      docked: { left: true, top: false, right: false, bottom: true },
    })

    // 右上角 x = 1920 - 160 = 1760。
    expect(snapToWorkArea({ x: 1828, y: 10 }, windowSize, baseSize, workArea)).toEqual({
      position: { x: 1760, y: 0 },
      size: { width: 160, height: 160 },
      docked: { left: false, top: true, right: true, bottom: false },
    })
  })

  it('does not snap when outside the threshold', () => {
    expect(snapToWorkArea({ x: 25, y: 200 }, windowSize, baseSize, workArea)).toEqual({
      position: { x: 25, y: 200 },
      size: { width: 80, height: 80 },
      docked: { left: false, top: false, right: false, bottom: false },
    })
  })

  it('restores the base size when dragged away from every edge', () => {
    // 模拟一个已贴边放大的 160×160 窗口被拖回屏幕中间。
    expect(
      snapToWorkArea({ x: 400, y: 400 }, { width: 160, height: 160 }, baseSize, workArea),
    ).toEqual({
      position: { x: 400, y: 400 },
      size: { width: 80, height: 80 },
      docked: { left: false, top: false, right: false, bottom: false },
    })
  })

  it('supports work areas with negative coordinates', () => {
    expect(
      snapToWorkArea(
        { x: -1908, y: -10 },
        windowSize,
        baseSize,
        { position: { x: -1920, y: -1080 }, size: { width: 1920, height: 1040 } },
      ),
    ).toEqual({
      position: { x: -1920, y: -200 },
      size: { width: 160, height: 160 },
      docked: { left: true, top: false, right: false, bottom: true },
    })
  })

  it('clamps oversized or out-of-bounds windows', () => {
    expect(
      snapToWorkArea(
        { x: 500, y: 500 },
        { width: 2200, height: 1200 },
        baseSize,
        workArea,
      ),
    ).toEqual({
      position: { x: 0, y: 0 },
      size: { width: 160, height: 160 },
      docked: { left: true, top: true, right: false, bottom: false },
    })
  })
})
