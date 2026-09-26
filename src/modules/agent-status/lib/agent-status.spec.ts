import { describe, expect, it } from 'vitest'
import {
  colorForSession,
  displayEndpoint,
  formatTokenCount,
  formatTokenMillions,
  isStatusSnapshot,
  tokenTotal,
  type SessionSnapshot,
  type StatusSnapshot,
} from './agent-status'

const session = (
  phase: SessionSnapshot['phase'],
  overrides?: Partial<SessionSnapshot>,
): SessionSnapshot => ({
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
  ...overrides,
})

const validSnapshot: StatusSnapshot = {
  schema_version: 1,
  revision: 5,
  generated_at_ms: Date.now(),
  sessions: [session('running')],
  counts: { running: 1, waiting: 0, error: 0, unknown: 0, completed: 0 },
  source_health: 'healthy',
}

describe('colorForSession', () => {
  it('maps running to green', () => {
    expect(colorForSession(session('running'))).toBe('green')
  })

  it('maps completed to green', () => {
    expect(colorForSession(session('completed'))).toBe('green')
  })

  it('maps waiting to yellow', () => {
    expect(colorForSession(session('waiting'))).toBe('yellow')
  })

  it('maps flashing waiting (Stop turn complete) to green', () => {
    expect(colorForSession(session('waiting', { is_flashing: true }))).toBe('green')
  })

  it('maps unknown to yellow', () => {
    expect(colorForSession(session('unknown'))).toBe('yellow')
  })

  it('maps error to red', () => {
    expect(colorForSession(session('error'))).toBe('red')
  })

  it('never returns aggregate colors', () => {
    const phases: SessionSnapshot['phase'][] = ['running', 'waiting', 'error', 'unknown', 'completed']
    for (const phase of phases) {
      const color = colorForSession(session(phase))
      expect(['green', 'yellow', 'red']).toContain(color)
    }
  })
})

describe('displayEndpoint', () => {
  it('prefers resolved upstream over the local proxy base_url', () => {
    const resolved = session('running', {
      base_url: 'http://127.0.0.1:15721',
      upstream_url: 'https://api.b.ai',
    })
    expect(displayEndpoint(resolved)).toBe('https://api.b.ai')
  })

  it('falls back to base_url when no upstream was resolved', () => {
    const direct = session('running', { base_url: 'https://api.anthropic.com' })
    expect(displayEndpoint(direct)).toBe('https://api.anthropic.com')
  })

  it('returns null when neither is known', () => {
    expect(displayEndpoint(session('running'))).toBeNull()
  })
})

describe('isStatusSnapshot', () => {
  it('accepts a valid snapshot', () => {
    expect(isStatusSnapshot(validSnapshot)).toBe(true)
  })

  it('rejects null', () => {
    expect(isStatusSnapshot(null)).toBe(false)
  })

  it('rejects non-object', () => {
    expect(isStatusSnapshot('string')).toBe(false)
    expect(isStatusSnapshot(42)).toBe(false)
    expect(isStatusSnapshot(undefined)).toBe(false)
  })

  it('rejects wrong schema version', () => {
    expect(isStatusSnapshot({ ...validSnapshot, schema_version: 2 })).toBe(false)
  })

  it('rejects negative revision', () => {
    expect(isStatusSnapshot({ ...validSnapshot, revision: -1 })).toBe(false)
  })

  it('rejects non-integer revision', () => {
    expect(isStatusSnapshot({ ...validSnapshot, revision: 1.5 })).toBe(false)
  })

  it('rejects missing generated_at_ms', () => {
    expect(isStatusSnapshot({ ...validSnapshot, generated_at_ms: undefined })).toBe(false)
  })

  it('rejects sessions not being an array', () => {
    expect(isStatusSnapshot({ ...validSnapshot, sessions: 'not-array' })).toBe(false)
  })

  it('rejects sessions exceeding 1000', () => {
    const sessions = Array.from({ length: 1001 }, () => session('running'))
    expect(isStatusSnapshot({ ...validSnapshot, sessions })).toBe(false)
  })

  it('rejects session missing key', () => {
    expect(isStatusSnapshot({
      ...validSnapshot,
      sessions: [{ ...session('running'), key: undefined }],
    })).toBe(false)
  })

  it('accepts a session from an unregistered provider', () => {
    // 这条锁住「接入新平台灯会亮」：早先这里是 claude/codex 白名单，未知 provider 的
    // 每一帧都被**整帧**丢弃，而首帧走 invoke 不经此校验 —— 症状是灯冻结在首帧、永不更新，
    // 比全盲更隐蔽。展示名/图标改由 provider 注册表降级，不再作为快照的准入条件。
    const snapshot = {
      ...validSnapshot,
      sessions: [{ ...session('running'), provider: 'gpt' }],
    }
    expect(isStatusSnapshot(snapshot)).toBe(true)
    // 灯色只看 phase：新平台不必被认得也能出灯。
    expect(colorForSession(snapshot.sessions[0])).toBe('green')
  })

  it('rejects session with a blank or absurdly long provider', () => {
    // 放宽不等于不验：空串与超长串仍是脏数据（展示位放不下，也不可能是真平台名）。
    expect(isStatusSnapshot({ ...validSnapshot, sessions: [{ ...session('running'), provider: '' }] })).toBe(false)
    expect(
      isStatusSnapshot({ ...validSnapshot, sessions: [{ ...session('running'), provider: 'x'.repeat(65) }] }),
    ).toBe(false)
  })

  it('rejects session missing termination_confirmed', () => {
    expect(isStatusSnapshot({
      ...validSnapshot,
      sessions: [{ ...session('running'), termination_confirmed: undefined }],
    })).toBe(false)
  })

  it('accepts codex provider', () => {
    expect(isStatusSnapshot({
      ...validSnapshot,
      sessions: [session('running', { provider: 'codex', key: 'codex:abc' })],
    })).toBe(true)
  })

  it('accepts empty sessions array', () => {
    expect(isStatusSnapshot({ ...validSnapshot, sessions: [] })).toBe(true)
  })

  it('rejects missing counts', () => {
    expect(isStatusSnapshot({ ...validSnapshot, counts: undefined })).toBe(false)
  })
})

describe('tokenTotal', () => {
  it('sums all four buckets including cache reads', () => {
    // 缓存读通常占比最大，漏掉会显著低估；口径必须与 Rust 侧 total() 一致。
    expect(
      tokenTotal({
        input_tokens: 100,
        cache_creation_input_tokens: 20,
        cache_read_input_tokens: 3000,
        output_tokens: 50,
      }),
    ).toBe(3170)
  })

  it('treats missing totals as zero', () => {
    expect(tokenTotal(null)).toBe(0)
    expect(tokenTotal(undefined)).toBe(0)
  })

  it('handles an all-zero session', () => {
    expect(
      tokenTotal({
        input_tokens: 0,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 0,
        output_tokens: 0,
      }),
    ).toBe(0)
  })
})

describe('formatTokenCount', () => {
  it('groups thousands with commas', () => {
    expect(formatTokenCount(0)).toBe('0')
    expect(formatTokenCount(999)).toBe('999')
    expect(formatTokenCount(1000)).toBe('1,000')
    expect(formatTokenCount(1_261_847)).toBe('1,261,847')
  })

  it('truncates fractions and clamps negatives to zero', () => {
    expect(formatTokenCount(1234.9)).toBe('1,234')
    expect(formatTokenCount(-5)).toBe('0')
  })

  it('falls back to zero for non-finite values', () => {
    // 快照里的字段理论上来自 Rust 的 u64，但事件来源不可信，不能让它渲染出 NaN。
    expect(formatTokenCount(Number.NaN)).toBe('0')
    expect(formatTokenCount(Number.POSITIVE_INFINITY)).toBe('0')
  })
})

describe('formatTokenMillions', () => {
  it('renders millions with up to two decimals', () => {
    expect(formatTokenMillions(1_261_847)).toBe('1.26M')
    expect(formatTokenMillions(12_600_000)).toBe('12.6M')
  })

  it('trims trailing zeros so whole millions stay short', () => {
    expect(formatTokenMillions(1_000_000)).toBe('1M')
    expect(formatTokenMillions(2_500_000)).toBe('2.5M')
    expect(formatTokenMillions(20_000_000)).toBe('20M')
  })

  it('shows a floor marker below 0.01M instead of a useless 0M', () => {
    // 小数位不足以表达时，0M 会让人误以为没有消耗。
    expect(formatTokenMillions(1_234)).toBe('<0.01M')
    expect(formatTokenMillions(9_999)).toBe('<0.01M')
    expect(formatTokenMillions(10_000)).toBe('0.01M')
  })

  it('handles non-finite values', () => {
    expect(formatTokenMillions(Number.NaN)).toBe('0M')
    expect(formatTokenMillions(Number.POSITIVE_INFINITY)).toBe('0M')
  })
})
