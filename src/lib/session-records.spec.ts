import { describe, expect, it } from 'vitest'
import { emptyRecordList, formatRecordTime, isSessionRecordList, type SessionRecord } from './session-records'

/** 一条合法存档，字段与 Rust `SessionRecord` 的序列化结果对齐。 */
const record: SessionRecord = {
  key: 'claude:sess-1',
  provider: 'claude',
  session_id: 'sess-1',
  prompt: '修一下这个 bug',
  base_url: 'https://api.example.com',
  started_at_ms: 1_700_000_000_000,
  last_seen_at_ms: 1_700_000_060_000,
  tokens: {
    input_tokens: 100,
    cache_creation_input_tokens: 0,
    cache_read_input_tokens: 3_000,
    output_tokens: 50,
  },
}

describe('isSessionRecordList', () => {
  it('accepts a well-formed list', () => {
    expect(isSessionRecordList({ schema_version: 1, revision: 0, records: [record] })).toBe(true)
  })

  it('accepts an empty record array', () => {
    expect(isSessionRecordList(emptyRecordList())).toBe(true)
  })

  it('accepts null prompt, base_url and tokens', () => {
    // 这三项在 Rust 侧都是 Option，序列化成 null；守卫必须放行。
    expect(
      isSessionRecordList({
        revision: 0,
        records: [{ ...record, prompt: null, base_url: null, tokens: null }],
      }),
    ).toBe(true)
  })

  it('rejects non-objects and null', () => {
    expect(isSessionRecordList(null)).toBe(false)
    expect(isSessionRecordList(undefined)).toBe(false)
    expect(isSessionRecordList('nope')).toBe(false)
    expect(isSessionRecordList(42)).toBe(false)
  })

  it('rejects a missing, fractional or negative revision', () => {
    // revision 是丢乱序事件的依据，缺失时整份拒绝而不是当 0 用。
    expect(isSessionRecordList({ records: [] })).toBe(false)
    expect(isSessionRecordList({ revision: -1, records: [] })).toBe(false)
    expect(isSessionRecordList({ revision: 1.5, records: [] })).toBe(false)
  })

  it('rejects a non-array records field', () => {
    expect(isSessionRecordList({ revision: 1, records: {} })).toBe(false)
  })

  it('rejects entries with a blank key or an unknown provider', () => {
    expect(isSessionRecordList({ revision: 1, records: [{ ...record, key: '' }] })).toBe(false)
    expect(
      isSessionRecordList({ revision: 1, records: [{ ...record, provider: 'gemini' }] }),
    ).toBe(false)
  })

  it('rejects entries missing a required numeric field', () => {
    expect(
      isSessionRecordList({ revision: 1, records: [{ ...record, started_at_ms: undefined }] }),
    ).toBe(false)
    expect(
      isSessionRecordList({ revision: 1, records: [{ ...record, last_seen_at_ms: 'soon' }] }),
    ).toBe(false)
  })

  it('rejects when any entry is invalid', () => {
    expect(isSessionRecordList({ revision: 1, records: [record, { key: 'x' }] })).toBe(false)
  })
})

describe('emptyRecordList', () => {
  it('is a valid, empty v1 list', () => {
    const list = emptyRecordList()
    expect(list.revision).toBe(0)
    expect(list.records).toEqual([])
    expect(isSessionRecordList(list)).toBe(true)
  })
})

describe('formatRecordTime', () => {
  it('renders month-day and time without a year', () => {
    // 与端点档案同一紧凑读法；用固定本地时区构造避免环境差异。
    const local = new Date(2024, 0, 5, 9, 8)
    expect(formatRecordTime(local.getTime())).toBe('01-05 09:08')
  })

  it('returns an empty string for missing or invalid values', () => {
    expect(formatRecordTime(0)).toBe('')
    expect(formatRecordTime(-1)).toBe('')
    expect(formatRecordTime(Number.NaN)).toBe('')
  })
})
