import { describe, expect, it } from 'vitest'
import { todoDemand } from './demand'

describe('todoDemand', () => {
  it('恒定返回 2 格，不受贴边形态影响', () => {
    expect(todoDemand('square')).toBe(2)
    expect(todoDemand('corner')).toBe(2)
    expect(todoDemand('horizontal')).toBe(2)
    expect(todoDemand('vertical')).toBe(2)
  })
})
