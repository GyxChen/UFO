import { describe, expect, it } from 'vitest'
import { weatherDemand } from './demand'

describe('weatherDemand', () => {
  it('恒定返回 2 格占用，不受布局形态影响', () => {
    expect(weatherDemand('square')).toBe(2)
    expect(weatherDemand('corner')).toBe(2)
    expect(weatherDemand('horizontal')).toBe(2)
    expect(weatherDemand('vertical')).toBe(2)
  })
})
