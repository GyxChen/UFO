import { describe, expect, it } from 'vitest'
import type { FloatingModuleConfig } from '@/lib/floating-modules'
import {
  AGENT_STATUS_MODULE_ID,
  DEFAULT_AGENT_STATUS_SETTINGS,
  MAX_RETENTION_SECONDS,
  MAX_RUNNING_TIMEOUT_SECONDS,
  agentStatusSettings,
  retentionMsFromSeconds,
  runningTimeoutMsFromSeconds,
  toSeconds,
} from './settings'

/** 造一份外壳配置，只填本组件关心的 `module_settings` 那一块。 */
function configWith(block: unknown): FloatingModuleConfig {
  return {
    schema_version: 1,
    revision: 1,
    modules: [],
    module_settings: { [AGENT_STATUS_MODULE_ID]: block },
  }
}

describe('agentStatusSettings', () => {
  it('falls back to defaults without a config or without its own block', () => {
    expect(agentStatusSettings(null)).toEqual(DEFAULT_AGENT_STATUS_SETTINGS)
    expect(agentStatusSettings(configWith(undefined))).toEqual(DEFAULT_AGENT_STATUS_SETTINGS)
    expect(agentStatusSettings(configWith({}))).toEqual(DEFAULT_AGENT_STATUS_SETTINGS)
  })

  it('reads the values the module itself stored', () => {
    expect(
      agentStatusSettings(
        configWith({
          completed_retention_ms: 45_000,
          running_timeout_ms: 120_000,
          token_unit: 'm_tokens',
          proxy_resolver: 'cc-switch',
          max_cells: 6,
        })
      )
    ).toEqual({
      completed_retention_ms: 45_000,
      running_timeout_ms: 120_000,
      token_unit: 'm_tokens',
      proxy_resolver: 'cc-switch',
      max_cells: 6,
    })
  })

  it('clamps out-of-range values', () => {
    const settings = agentStatusSettings(
      configWith({ completed_retention_ms: 0, running_timeout_ms: Number.MAX_SAFE_INTEGER })
    )
    expect(settings.completed_retention_ms).toBe(1_000)
    expect(settings.running_timeout_ms).toBe(MAX_RUNNING_TIMEOUT_SECONDS * 1000)

    const upper = agentStatusSettings(
      configWith({ completed_retention_ms: 999_999_999_999, running_timeout_ms: -1 })
    )
    expect(upper.completed_retention_ms).toBe(MAX_RETENTION_SECONDS * 1000)
    expect(upper.running_timeout_ms).toBe(60_000)
  })

  it('falls back per field when a value is broken', () => {
    // 关键回归：一个字段写错不能牵连其它字段（与 Rust 侧同一口径）。
    const settings = agentStatusSettings(
      configWith({
        completed_retention_ms: 45_000,
        running_timeout_ms: '120000',
        token_unit: 'millions',
        proxy_resolver: '   ',
      })
    )
    expect(settings.completed_retention_ms).toBe(45_000)
    expect(settings.running_timeout_ms).toBe(DEFAULT_AGENT_STATUS_SETTINGS.running_timeout_ms)
    expect(settings.token_unit).toBe('tokens')
    expect(settings.proxy_resolver).toBe('none')
  })
})

describe('seconds helpers', () => {
  it('converts and clamps the settings page input', () => {
    expect(retentionMsFromSeconds(45)).toBe(45_000)
    expect(retentionMsFromSeconds(0)).toBe(1_000)
    expect(retentionMsFromSeconds(MAX_RETENTION_SECONDS + 1)).toBe(MAX_RETENTION_SECONDS * 1000)
    expect(runningTimeoutMsFromSeconds(120)).toBe(120_000)
    expect(runningTimeoutMsFromSeconds(1)).toBe(60_000)
    expect(runningTimeoutMsFromSeconds(Number.NaN)).toBe(
      DEFAULT_AGENT_STATUS_SETTINGS.running_timeout_ms
    )
    expect(toSeconds(45_000)).toBe(45)
  })
})
