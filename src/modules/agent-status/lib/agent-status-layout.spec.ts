import { describe, expect, it } from 'vitest'
import { colorForSession, type SessionSnapshot } from './agent-status'
import { ABS_MAX_CELLS, DEFAULT_WINDOW_CELLS, type FloatingLayout } from '@/lib/floating-layout'
import { indicatorCapacity, partitionIndicatorSessions, windowCellsForSessions } from './agent-status-layout'

const session = (phase: SessionSnapshot['phase']): SessionSnapshot => ({
  key: `claude:${phase}`,
  provider: 'claude',
  session_id: phase,
  phase,
  source: 'hook',
  confidence: 100,
  started_at_ms: 0,
  last_seen_at_ms: 0,
  completed_at_ms: phase === 'completed' ? 0 : null,
  remove_at_ms: phase === 'completed' ? 30_000 : null,
  is_flashing: phase === 'completed',
  pid: null,
  error_code: null,
  base_url: null,
  upstream_url: null,
  prompt: null,
  waiting_reason: null,
  tokens: null,
  last_sequence: null,
  termination_confirmed: phase === 'completed',
})

/** 各形态下「格子数 → 能放几盏灯」的期望值，供分区用例复用。 */
const capacity: Record<string, number> = {
  square4: 4,
  corner8: 9,
  horizontal6: 6,
  vertical99: ABS_MAX_CELLS,
}

describe('agent status presentation', () => {
  it('maps each session phase to its independent lamp color', () => {
    expect(colorForSession(session('running'))).toBe('green')
    expect(colorForSession(session('completed'))).toBe('green')
    expect(colorForSession(session('waiting'))).toBe('yellow')
    expect(colorForSession(session('unknown'))).toBe('yellow')
    expect(colorForSession(session('error'))).toBe('red')
  })

  it('floors the cell demand at the default and caps it at the layout max', () => {
    expect(windowCellsForSessions(0, 8)).toBe(DEFAULT_WINDOW_CELLS)
    expect(windowCellsForSessions(3, 8)).toBe(DEFAULT_WINDOW_CELLS)
    expect(windowCellsForSessions(4, 8)).toBe(DEFAULT_WINDOW_CELLS)
    expect(windowCellsForSessions(5, 8)).toBe(5)
    expect(windowCellsForSessions(10, 8)).toBe(8)
    // 上限低于默认（用户配错）时仍不低于默认容量。
    expect(windowCellsForSessions(10, 3)).toBe(DEFAULT_WINDOW_CELLS)
  })

  it('uses full square grid slots so overflow can fill every cell', () => {
    expect(indicatorCapacity('square', 4)).toBe(capacity.square4)
    expect(indicatorCapacity('corner', 8)).toBe(capacity.corner8)
    // 上限 5 也是 3×3 网格，容量按网格算。
    expect(indicatorCapacity('corner', 5)).toBe(9)
    expect(indicatorCapacity('horizontal', 6)).toBe(capacity.horizontal6)
    expect(indicatorCapacity('vertical', 99)).toBe(capacity.vertical99)
  })

  it('partitions sessions so overflow replaces the last cell', () => {
    const items = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]

    expect(partitionIndicatorSessions([], 'square', 4)).toEqual({ visible: [], overflow: 0 })
    expect(partitionIndicatorSessions(items.slice(0, 1), 'square', 4)).toEqual({
      visible: [1],
      overflow: 0,
    })
    expect(partitionIndicatorSessions(items.slice(0, 4), 'horizontal', 4)).toEqual({
      visible: [1, 2, 3, 4],
      overflow: 0,
    })
    // 2×2 网格放 5 个会话 → 3 灯 +N
    expect(partitionIndicatorSessions(items.slice(0, 5), 'corner', 4)).toEqual({
      visible: [1, 2, 3],
      overflow: 2,
    })
    expect(partitionIndicatorSessions(items.slice(0, 7), 'vertical', 4)).toEqual({
      visible: [1, 2, 3],
      overflow: 4,
    })
    expect(partitionIndicatorSessions(items.slice(0, 8), 'horizontal', 6)).toEqual({
      visible: [1, 2, 3, 4, 5],
      overflow: 3,
    })
    // 3×3 网格（8 格上限也是 3×3）能放 9 盏，超过才出现 +N。
    expect(partitionIndicatorSessions(items.slice(0, 9), 'corner', 8)).toEqual({
      visible: [1, 2, 3, 4, 5, 6, 7, 8, 9],
      overflow: 0,
    })
    expect(partitionIndicatorSessions(items, 'corner', 8)).toEqual({
      visible: [1, 2, 3, 4, 5, 6, 7, 8],
      overflow: 2,
    })
  })

  it('keeps every shape renderable for a squeezed budget', () => {
    // 窗口被同级模块挤到 1 格时不能算出空集。
    const shapes: FloatingLayout[] = ['square', 'corner', 'horizontal', 'vertical']
    for (const layout of shapes) {
      expect(indicatorCapacity(layout, 1)).toBeGreaterThanOrEqual(1)
      expect(partitionIndicatorSessions([1, 2, 3], layout, 1).overflow).toBeGreaterThan(0)
    }
  })
})
