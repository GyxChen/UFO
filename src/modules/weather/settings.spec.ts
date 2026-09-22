import { describe, expect, it } from 'vitest'
import { DEFAULT_WEATHER_SETTINGS, weatherSettings } from './settings'

describe('weatherSettings', () => {
  it('传入 null 或空对象时返回默认配置', () => {
    const result = weatherSettings(null)
    expect(result).toEqual(DEFAULT_WEATHER_SETTINGS)
  })

  it('正确解析字段并 clamp 刷新周期在 10..=60 分钟', () => {
    const customConfig = {
      schema_version: 1,
      revision: 1,
      modules: [],
      module_settings: {
        weather: {
          has_key: true,
          host_type: 'commercial',
          location_mode: 'manual',
          manual_location: '101010100',
          manual_location_name: '北京',
          refresh_interval_min: 3, // 低于 10，应夹取为 10
        },
      },
    }

    const result = weatherSettings(customConfig)
    expect(result.has_key).toBe(true)
    expect(result.host_type).toBe('commercial')
    expect(result.location_mode).toBe('manual')
    expect(result.manual_location).toBe('101010100')
    expect(result.manual_location_name).toBe('北京')
    expect(result.refresh_interval_min).toBe(10)
  })

  it('越上限的刷新周期夹取为 60', () => {
    const customConfig = {
      schema_version: 1,
      revision: 1,
      modules: [],
      module_settings: {
        weather: {
          refresh_interval_min: 120,
        },
      },
    }

    const result = weatherSettings(customConfig)
    expect(result.refresh_interval_min).toBe(60)
  })
})
