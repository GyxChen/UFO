import { describe, expect, it } from 'vitest'
import {
  emptyFlowSnapshot,
  flowColorClass,
  flowFromExtensions,
  isFlowField,
  isFlowSnapshot,
  type FlowField,
} from './flow'

/** 一条合法字段，字段与 Rust `flow::FlowField` 的序列化结果对齐。 */
const field: FlowField = { label: '余额', value: '¥12.30', color: 'green' }

describe('isFlowSnapshot', () => {
  it('accepts a well-formed snapshot', () => {
    expect(
      isFlowSnapshot({ revision: 0, sessions: { 'claude:s1': [field] } }),
    ).toBe(true)
  })

  it('accepts an empty sessions map', () => {
    expect(isFlowSnapshot(emptyFlowSnapshot())).toBe(true)
  })

  it('rejects non-objects and null', () => {
    expect(isFlowSnapshot(null)).toBe(false)
    expect(isFlowSnapshot(undefined)).toBe(false)
    expect(isFlowSnapshot('nope')).toBe(false)
  })

  it('rejects a missing or negative revision', () => {
    // revision 是丢乱序事件的依据，缺失时必须整份拒绝而不是当 0 用。
    expect(isFlowSnapshot({ sessions: {} })).toBe(false)
    expect(isFlowSnapshot({ revision: -1, sessions: {} })).toBe(false)
  })

  it('rejects a missing sessions map', () => {
    expect(isFlowSnapshot({ revision: 1 })).toBe(false)
  })

  it('rejects when any field is malformed', () => {
    expect(isFlowSnapshot({ revision: 1, sessions: { k: [{ label: 'a' }] } })).toBe(false)
    expect(isFlowSnapshot({ revision: 1, sessions: { k: 'not-array' } })).toBe(false)
  })
})

describe('isFlowField', () => {
  it('accepts null or a known color', () => {
    expect(isFlowField({ label: 'a', value: 'b', color: null })).toBe(true)
    expect(isFlowField({ label: 'a', value: 'b', color: 'red' })).toBe(true)
  })

  it('rejects non-string label or value', () => {
    expect(isFlowField({ label: 1, value: 'b', color: null })).toBe(false)
    expect(isFlowField({ label: 'a', value: undefined, color: null })).toBe(false)
  })
})

describe('flowColorClass', () => {
  it('passes known colors through', () => {
    expect(flowColorClass('green')).toBe('green')
    expect(flowColorClass('red')).toBe('red')
  })

  it('falls back to muted for null or unknown', () => {
    expect(flowColorClass(null)).toBe('muted')
    expect(flowColorClass(undefined)).toBe('muted')
    expect(flowColorClass('purple' as never)).toBe('muted')
  })
})

describe('flowFromExtensions', () => {
  it('reads a stored flow config', () => {
    const stored = flowFromExtensions({
      flow: {
        enabled: true,
        interval_sec: 30,
        timeout_ms: 4000,
        script: 'return []',
        script_hash: 9,
        secret_names: ['K'],
      },
    })
    expect(stored?.enabled).toBe(true)
    expect(stored?.interval_sec).toBe(30)
    expect(stored?.secret_names).toEqual(['K'])
  })

  it('returns null when flow key is absent or wrong shape', () => {
    expect(flowFromExtensions(undefined)).toBeNull()
    expect(flowFromExtensions({})).toBeNull()
    expect(flowFromExtensions({ flow: 'nope' })).toBeNull()
    // enabled 必须是布尔，否则整块视为无配置（不猜测默认）。
    expect(flowFromExtensions({ flow: { interval_sec: 30 } })).toBeNull()
  })

  it('fills defaults for missing numeric / list fields', () => {
    const stored = flowFromExtensions({ flow: { enabled: false } })
    expect(stored?.interval_sec).toBe(60)
    expect(stored?.script).toBe('')
    expect(stored?.secret_names).toEqual([])
  })
})
