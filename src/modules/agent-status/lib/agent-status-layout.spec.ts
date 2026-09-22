import { describe, expect, it } from 'vitest'
import { colorForSession, colorLabel, phaseLabel, type SessionPhase, type SessionSnapshot } from './agent-status'
import type { FloatingLayout } from '@/lib/floating-layout'
import {
  agentStatusCapacity,
  effectiveLayoutMax,
  indicatorCapacity,
  overflowHoverKey,
  overflowListFor,
  overflowRingGradient,
  overflowSegments,
  parseOverflowHoverKey,
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

    expect(partitionIndicatorSessions([], 'corner', 2)).toEqual({ visible: [], hidden: [], overflow: 0 })
    expect(partitionIndicatorSessions(items.slice(0, 1), 'corner', 2)).toEqual({
      visible: [1],
      hidden: [],
      overflow: 0,
    })
    // 贴角 2×3（容量 6）放 5 个会话 → 全显。
    expect(partitionIndicatorSessions(items.slice(0, 5), 'corner', 6)).toEqual({
      visible: [1, 2, 3, 4, 5],
      hidden: [],
      overflow: 0,
    })
    // 贴角 2×3（容量 6）放 8 个 → 5 灯 +N3（被折叠的 3 个也一并返回，占比环与清单靠它）。
    expect(partitionIndicatorSessions(items.slice(0, 8), 'corner', 6)).toEqual({
      visible: [1, 2, 3, 4, 5],
      hidden: [6, 7, 8],
      overflow: 3,
    })
    // 边条容量 9 放 10 → 8 灯 +N2。
    expect(partitionIndicatorSessions(items, 'horizontal', 9)).toEqual({
      visible: [1, 2, 3, 4, 5, 6, 7, 8],
      hidden: [9, 10],
      overflow: 2,
    })
    // 竖条 4 格放 7 → 3 灯 +N4。
    expect(partitionIndicatorSessions(items.slice(0, 7), 'vertical', 4)).toEqual({
      visible: [1, 2, 3],
      hidden: [4, 5, 6, 7],
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

  it('labels phases and lamp colours for the folded-session list', () => {
    expect(phaseLabel('running')).toBe('执行中')
    expect(phaseLabel('waiting')).toBe('等待操作')
    expect(phaseLabel('error')).toBe('出错')
    expect(phaseLabel('completed')).toBe('已完成')
    // 未列出的阶段（旧 hook 事件反序列化出的 unknown / 将来新增的枚举）不能返回空标签。
    for (const phase of ['unknown', 'brand-new-phase' as SessionPhase]) {
      expect(phaseLabel(phase)).toBe('状态未知')
    }
    expect(colorLabel('red')).toBe('出错')
    expect(colorLabel('yellow')).toBe('等待操作')
    expect(colorLabel('green')).toBe('执行中')
  })
})

/**
 * 溢出格（方案 B「分片环」）的三块纯逻辑：合成 key、占比分片 / 渐变、折叠清单。
 * 悬浮窗与提示窗是两个窗口，全靠这一份口径把「屏上的环」与「悬停清单」对齐。
 */
describe('overflow cell (ring)', () => {
  /** 造一批颜色可控的会话：红 / 黄 / 绿各自数量由入参决定。 */
  const colored = (...phases: SessionSnapshot['phase'][]): SessionSnapshot[] =>
    phases.map((phase, index) => ({ ...session(phase), key: `claude:${index}`, session_id: `s${index}` }))

  it('round-trips the overflow hover key', () => {
    expect(overflowHoverKey('corner', 2)).toBe('@overflow:corner:2')
    expect(parseOverflowHoverKey('@overflow:corner:2')).toEqual({ layout: 'corner', cells: 2 })
    // 造 key 时夹取（0 / 越界都会把窗口算塌），解析时同样宽进严出。
    expect(overflowHoverKey('square', 0)).toBe('@overflow:square:1')
    expect(overflowHoverKey('square', 99)).toBe('@overflow:square:16')
    // 真实会话 key 是 `<provider>:<session-id>`，绝不能被当成溢出格。
    expect(parseOverflowHoverKey('claude:abc')).toBeNull()
    expect(parseOverflowHoverKey('@overflow:corner')).toBeNull()
    expect(parseOverflowHoverKey('@overflow:diagonal:2')).toBeNull()
    expect(parseOverflowHoverKey('@overflow:corner:1.5')).toBeNull()
    expect(parseOverflowHoverKey('@overflow:corner:0')).toBeNull()
    expect(parseOverflowHoverKey('@overflow:corner:17')).toBeNull()
  })

  it('groups folded sessions by colour in attention order', () => {
    // 全绿：只出一个分片（不占角度、不留零宽 stop）。
    expect(overflowSegments(colored('running', 'completed'))).toEqual([{ color: 'green', count: 2 }])
    // 红 → 黄 → 绿，与哪个先开无关。
    expect(overflowSegments(colored('running', 'waiting', 'error', 'waiting', 'running'))).toEqual([
      { color: 'red', count: 1 },
      { color: 'yellow', count: 2 },
      { color: 'green', count: 2 },
    ])
    expect(overflowSegments([])).toEqual([])
  })

  it('builds a gap-free conic gradient from the segments', () => {
    expect(overflowRingGradient([])).toBe('')
    expect(overflowRingGradient([{ color: 'green', count: 3 }])).toBe('conic-gradient(var(--ufo-status-green) 0% 100%)')
    expect(
      overflowRingGradient([
        { color: 'red', count: 1 },
        { color: 'yellow', count: 2 },
        { color: 'green', count: 2 },
      ])
    ).toBe(
      'conic-gradient(var(--ufo-status-red) 0% 20%, var(--ufo-status-yellow) 20% 60%, var(--ufo-status-green) 60% 100%)'
    )
    // 除不尽的比例（1/3）：相邻两段共用同一个累计值，扇形之间不会留缝。
    const thirds = overflowRingGradient([
      { color: 'red', count: 1 },
      { color: 'yellow', count: 1 },
      { color: 'green', count: 1 },
    ])
    expect(thirds).toBe(
      'conic-gradient(var(--ufo-status-red) 0% 33.33%, var(--ufo-status-yellow) 33.33% 66.67%, var(--ufo-status-green) 66.67% 100%)'
    )
  })

  it('restores the folded list from the hover key', () => {
    const sessions = colored('running', 'waiting', 'error', 'waiting', 'running')

    // 不是溢出格的 key → null（调用方按单会话处理）。
    expect(overflowListFor('claude:0', sessions)).toBeNull()
    expect(overflowListFor('@overflow:diagonal:2', sessions)).toBeNull()

    // 方形 1 格（容量 1）放 5 个 → 5 个全折叠，且按注意力优先级排（红先）。
    const folded = overflowListFor(overflowHoverKey('square', 1), sessions)
    expect(folded?.map(item => item.phase)).toEqual(['error', 'waiting', 'waiting', 'running', 'running'])
    // 贴角 2 格放 5 个 → 1 灯 + 折叠 4 个。
    expect(overflowListFor(overflowHoverKey('corner', 2), sessions)?.length).toBe(4)
    // 会话在悬停期间结束到放得下 → 空数组（调用方给降级文案，不是「不是溢出格」）。
    expect(overflowListFor(overflowHoverKey('corner', 6), sessions)).toEqual([])
  })
})
