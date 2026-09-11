import { describe, expect, it } from 'vitest'
import {
  colorForSession,
  isStatusSnapshot,
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

  it('rejects session with invalid provider', () => {
    expect(isStatusSnapshot({
      ...validSnapshot,
      sessions: [{ ...session('running'), provider: 'gpt' }],
    })).toBe(false)
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
