import { describe, expect, it } from 'vitest'
import { colorForSession, type SessionSnapshot } from './agent-status'
import type { FloatingLayout } from '@/lib/floating-layout'
import {
  agentStatusCapacity,
  effectiveLayoutMax,
  indicatorCapacity,
  partitionIndicatorSessions,
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
  waiting_reason: null,
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

  it('maps session counts to the step ladder per layout', () => {
    // 边条按个数精确取长度，封顶 9，最低 1 格。
    expect(agentStatusCapacity('horizontal', 0, 9)).toBe(1)
    expect(agentStatusCapacity('horizontal', 1, 9)).toBe(1)
    expect(agentStatusCapacity('horizontal', 5, 9)).toBe(5)
    expect(agentStatusCapacity('horizontal', 9, 9)).toBe(9)
    expect(agentStatusCapacity('horizontal', 12, 9)).toBe(9)
    expect(agentStatusCapacity('vertical', 7, 9)).toBe(7)
    // 贴角逐档，最少 1×2（2 格）：1–2 → 2、3 → 3、4 → 4、5–6 → 6、7–9 → 9。
    expect(agentStatusCapacity('corner', 1, 9)).toBe(2)
    expect(agentStatusCapacity('corner', 2, 9)).toBe(2)
    expect(agentStatusCapacity('corner', 3, 9)).toBe(3)
    expect(agentStatusCapacity('corner', 4, 9)).toBe(4)
    expect(agentStatusCapacity('corner', 5, 9)).toBe(6)
    expect(agentStatusCapacity('corner', 6, 9)).toBe(6)
    expect(agentStatusCapacity('corner', 9, 9)).toBe(9)
    expect(agentStatusCapacity('corner', 12, 9)).toBe(9)
    // 方形可到 1×1。
    expect(agentStatusCapacity('square', 1, 9)).toBe(1)
    expect(agentStatusCapacity('square', 5, 9)).toBe(6)
  })

  it('caps the ladder at the configured per-layout max (snapped down to a step)', () => {
    // 配置上限向下夹到最近档位：8 → 6、9 → 9、5 → 4。
    expect(effectiveLayoutMax('horizontal', 8)).toBe(6)
    expect(effectiveLayoutMax('horizontal', 9)).toBe(9)
    expect(effectiveLayoutMax('corner', 5)).toBe(4)
    // 贴角地板 1×2：配置再低也不低于 2。
    expect(effectiveLayoutMax('corner', 1)).toBe(2)
    // 容量受配置上限约束（不会因归档而越过上限）。
    expect(agentStatusCapacity('horizontal', 9, 6)).toBe(6)
    expect(agentStatusCapacity('corner', 9, 6)).toBe(6)
  })

  it('computes capacity as the grid area', () => {
    expect(indicatorCapacity('horizontal', 5)).toBe(5)
    expect(indicatorCapacity('vertical', 7)).toBe(7)
    expect(indicatorCapacity('corner', 6)).toBe(6) // 2×3
    expect(indicatorCapacity('corner', 4)).toBe(4) // 2×2
    expect(indicatorCapacity('square', 1)).toBe(1)
  })

  it('partitions sessions so overflow replaces the last cell', () => {
    const items = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]

    expect(partitionIndicatorSessions([], 'corner', 2)).toEqual({ visible: [], overflow: 0 })
    expect(partitionIndicatorSessions(items.slice(0, 1), 'corner', 2)).toEqual({ visible: [1], overflow: 0 })
    // 贴角 2×3（容量 6）放 5 个会话 → 全显。
    expect(partitionIndicatorSessions(items.slice(0, 5), 'corner', 6)).toEqual({
      visible: [1, 2, 3, 4, 5],
      overflow: 0,
    })
    // 贴角 2×3（容量 6）放 8 个 → 5 灯 +N3。
    expect(partitionIndicatorSessions(items.slice(0, 8), 'corner', 6)).toEqual({
      visible: [1, 2, 3, 4, 5],
      overflow: 3,
    })
    // 边条容量 9 放 10 → 8 灯 +N2。
    expect(partitionIndicatorSessions(items, 'horizontal', 9)).toEqual({
      visible: [1, 2, 3, 4, 5, 6, 7, 8],
      overflow: 2,
    })
    // 竖条 4 格放 7 → 3 灯 +N4。
    expect(partitionIndicatorSessions(items.slice(0, 7), 'vertical', 4)).toEqual({
      visible: [1, 2, 3],
      overflow: 4,
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
