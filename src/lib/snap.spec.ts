import { describe, expect, it } from 'vitest'
import { snapToWorkArea } from './snap'

const workArea = {
  position: { x: 0, y: 0 },
  size: { width: 1920, height: 1040 },
}
const windowSize = { width: 80, height: 80 }

describe('snapToWorkArea', () => {
  it('snaps each nearby edge', () => {
    expect(snapToWorkArea({ x: 12, y: 200 }, windowSize, workArea)).toEqual({ x: 0, y: 200 })
    expect(snapToWorkArea({ x: 1828, y: 200 }, windowSize, workArea)).toEqual({ x: 1840, y: 200 })
    expect(snapToWorkArea({ x: 200, y: 12 }, windowSize, workArea)).toEqual({ x: 200, y: 0 })
    expect(snapToWorkArea({ x: 200, y: 972 }, windowSize, workArea)).toEqual({ x: 200, y: 960 })
  })

  it('snaps corners independently', () => {
    expect(snapToWorkArea({ x: 10, y: 970 }, windowSize, workArea)).toEqual({ x: 0, y: 960 })
  })

  it('does not snap when outside the threshold', () => {
    expect(snapToWorkArea({ x: 25, y: 200 }, windowSize, workArea)).toEqual({ x: 25, y: 200 })
  })

  it('supports work areas with negative coordinates', () => {
    expect(
      snapToWorkArea(
        { x: -1908, y: -10 },
        windowSize,
        { position: { x: -1920, y: -1080 }, size: { width: 1920, height: 1040 } },
      ),
    ).toEqual({ x: -1920, y: -120 })
  })

  it('clamps oversized or out-of-bounds windows', () => {
    expect(
      snapToWorkArea(
        { x: 500, y: 500 },
        { width: 2200, height: 1200 },
        workArea,
      ),
    ).toEqual({ x: 0, y: 0 })
  })
})
