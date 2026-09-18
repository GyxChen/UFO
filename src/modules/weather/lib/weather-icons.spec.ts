import { describe, expect, it } from 'vitest'
import {
  CloudyIcon,
  FogIcon,
  OvercastIcon,
  RainIcon,
  resolveWeatherIcon,
  SnowIcon,
  SunnyIcon,
} from './weather-icons'

describe('resolveWeatherIcon', () => {
  it('正确映射 100 为 SunnyIcon', () => {
    expect(resolveWeatherIcon('100')).toBe(SunnyIcon)
  })

  it('正确映射 101~103 为 CloudyIcon', () => {
    expect(resolveWeatherIcon('101')).toBe(CloudyIcon)
    expect(resolveWeatherIcon('103')).toBe(CloudyIcon)
  })

  it('正确映射 104 为 OvercastIcon', () => {
    expect(resolveWeatherIcon('104')).toBe(OvercastIcon)
  })

  it('正确映射 3xx 为 RainIcon', () => {
    expect(resolveWeatherIcon('305')).toBe(RainIcon)
  })

  it('正确映射 4xx 为 SnowIcon', () => {
    expect(resolveWeatherIcon('400')).toBe(SnowIcon)
  })

  it('正确映射 5xx 为 FogIcon', () => {
    expect(resolveWeatherIcon('501')).toBe(FogIcon)
  })

  it('未知代码回退到 CloudyIcon', () => {
    expect(resolveWeatherIcon('999')).toBe(CloudyIcon)
    expect(resolveWeatherIcon(undefined)).toBe(CloudyIcon)
  })
})
