import { describe, expect, it } from 'vitest'
import { colorForSession, type SessionSnapshot } from './agent-status'
import {
  ABS_MAX_CELLS,
  indicatorCapacity,
  indicatorGrid,
  layoutFromDocked,
  partitionIndicatorSessions,
  squareSideForCells,
  windowCellsForSessions,
} from './agent-status-layout'

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
  tokens: null,
  last_sequence: null,
  termination_confirmed: phase === 'completed',
})

describe('agent status presentation', () => {
  it('maps each session phase to its independent lamp color', () => {
    expect(colorForSession(session('running'))).toBe('green')
    expect(colorForSession(session('completed'))).toBe('green')
    expect(colorForSession(session('waiting'))).toBe('yellow')
    expect(colorForSession(session('unknown'))).toBe('yellow')
    expect(colorForSession(session('error'))).toBe('red')
  })

  it('derives each floating layout from docked edges', () => {
    expect(layoutFromDocked(null)).toBe('square')
    expect(layoutFromDocked({ left: false, top: true, right: false, bottom: false })).toBe('horizontal')
    expect(layoutFromDocked({ left: true, top: false, right: false, bottom: false })).toBe('vertical')
    expect(layoutFromDocked({ left: true, top: true, right: false, bottom: false })).toBe('corner')
  })

  it('uses full square grid slots so 3×3 overflow can fill every cell', () => {
    expect(indicatorCapacity('square', 4, 4)).toBe(4)
    expect(indicatorCapacity('corner', 8, 8)).toBe(9)
    expect(indicatorCapacity('corner', 8, 5)).toBe(9)
    expect(indicatorCapacity('horizontal', 6)).toBe(6)
    expect(indicatorCapacity('vertical', 99)).toBe(ABS_MAX_CELLS)
  })

  it('maps each layout to its indicator grid for a cell count', () => {
    expect(indicatorGrid('square', 4)).toEqual({ cols: 2, rows: 2 })
    expect(indicatorGrid('corner', 4)).toEqual({ cols: 2, rows: 2 })
    expect(indicatorGrid('corner', 5)).toEqual({ cols: 3, rows: 3 })
    expect(indicatorGrid('horizontal', 6)).toEqual({ cols: 6, rows: 1 })
    expect(indicatorGrid('vertical', 6)).toEqual({ cols: 1, rows: 6 })
  })

  it('computes square side as ceil(sqrt(cells))', () => {
    expect(squareSideForCells(1)).toBe(1)
    expect(squareSideForCells(4)).toBe(2)
    expect(squareSideForCells(5)).toBe(3)
    expect(squareSideForCells(8)).toBe(3)
  })

  it('floors window cells at the default and caps at layout max', () => {
    expect(windowCellsForSessions(0, 8)).toBe(4)
    expect(windowCellsForSessions(3, 8)).toBe(4)
    expect(windowCellsForSessions(4, 8)).toBe(4)
    expect(windowCellsForSessions(5, 8)).toBe(5)
    expect(windowCellsForSessions(10, 8)).toBe(8)
    expect(windowCellsForSessions(10, 3)).toBe(4)
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
    // 角上限 4、仍是 2×2：5 个会话 → 3 灯 +N
    expect(partitionIndicatorSessions(items.slice(0, 5), 'corner', 4, 4)).toEqual({
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
    // 3×3：未超上限时全显（可暂空）；超上限后按 9 格占满，8 灯 +N
    expect(partitionIndicatorSessions(items.slice(0, 8), 'corner', 8, 8)).toEqual({
      visible: [1, 2, 3, 4, 5, 6, 7, 8],
      overflow: 0,
    })
    expect(partitionIndicatorSessions(items.slice(0, 9), 'corner', 8, 8)).toEqual({
      visible: [1, 2, 3, 4, 5, 6, 7, 8],
      overflow: 1,
    })
    expect(partitionIndicatorSessions(items, 'corner', 8, 8)).toEqual({
      visible: [1, 2, 3, 4, 5, 6, 7, 8],
      overflow: 2,
    })
  })
})
