import { describe, expect, it } from 'vitest'
import {
  emptyProfileList,
  formatProfileTime,
  isEndpointProfileList,
  type EndpointProfile,
} from './endpoint-profiles'

/** 一条合法档案，字段与 Rust `EndpointProfile` 的序列化结果对齐。 */
const profile: EndpointProfile = {
  id: 'api.example.com:443',
  url: 'https://api.example.com/v1',
  first_seen_at_ms: 1_700_000_000_000,
  extensions: {},
}

describe('isEndpointProfileList', () => {
  it('accepts a well-formed list', () => {
    expect(isEndpointProfileList({ schema_version: 1, revision: 0, profiles: [profile] })).toBe(true)
  })

  it('accepts an empty profile array', () => {
    expect(isEndpointProfileList(emptyProfileList())).toBe(true)
  })

  it('rejects non-objects and null', () => {
    expect(isEndpointProfileList(null)).toBe(false)
    expect(isEndpointProfileList(undefined)).toBe(false)
    expect(isEndpointProfileList('nope')).toBe(false)
    expect(isEndpointProfileList(42)).toBe(false)
  })

  it('rejects a missing or negative revision', () => {
    // revision 是丢乱序事件的依据，缺失时必须整份拒绝而不是当 0 用。
    expect(isEndpointProfileList({ profiles: [] })).toBe(false)
    expect(isEndpointProfileList({ revision: -1, profiles: [] })).toBe(false)
    expect(isEndpointProfileList({ revision: 1.5, profiles: [] })).toBe(false)
  })

  it('rejects a non-array profiles field', () => {
    expect(isEndpointProfileList({ revision: 1, profiles: {} })).toBe(false)
  })

  it('rejects entries with a blank id or a missing field', () => {
    expect(
      isEndpointProfileList({ revision: 1, profiles: [{ ...profile, id: '' }] }),
    ).toBe(false)
    expect(
      isEndpointProfileList({ revision: 1, profiles: [{ ...profile, url: undefined }] }),
    ).toBe(false)
    expect(
      isEndpointProfileList({
        revision: 1,
        profiles: [{ ...profile, first_seen_at_ms: 'soon' }],
      }),
    ).toBe(false)
  })

  it('rejects when any entry is invalid', () => {
    expect(isEndpointProfileList({ revision: 1, profiles: [profile, { id: 'x' }] })).toBe(false)
  })
})

describe('formatProfileTime', () => {
  it('formats a timestamp as MM-DD HH:mm', () => {
    // 本地时区构造，避免测试依赖运行机器的时区。
    expect(formatProfileTime(new Date(2023, 10, 14, 9, 5).getTime())).toBe('11-14 09:05')
  })

  it('pads month, day and hour with a leading zero', () => {
    expect(formatProfileTime(new Date(2024, 0, 2, 3, 4).getTime())).toBe('01-02 03:04')
  })

  it('returns an empty string for missing or non-positive values', () => {
    expect(formatProfileTime(0)).toBe('')
    expect(formatProfileTime(-1)).toBe('')
    expect(formatProfileTime(Number.NaN)).toBe('')
    expect(formatProfileTime(Number.POSITIVE_INFINITY)).toBe('')
  })
})

describe('emptyProfileList', () => {
  it('starts at revision 0 with no profiles', () => {
    expect(emptyProfileList()).toEqual({ schema_version: 1, revision: 0, profiles: [] })
  })
})
