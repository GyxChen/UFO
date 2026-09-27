import { describe, expect, it } from 'vitest'
import type { FloatingModuleConfig } from '@/lib/floating-modules'
import {
  AGENT_STATUS_MODULE_ID,
  DEFAULT_AGENT_STATUS_SETTINGS,
  DEFAULT_LOG_SETTINGS,
  MAX_RETENTION_SECONDS,
  MAX_RUNNING_TIMEOUT_SECONDS,
  agentStatusSettings,
  logBlockWithSwitch,
  logSwitchOn,
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
          sound_enabled: false,
        })
      )
    ).toEqual({
      completed_retention_ms: 45_000,
      running_timeout_ms: 120_000,
      token_unit: 'm_tokens',
      proxy_resolver: 'cc-switch',
      max_cells: 6,
      sound_enabled: false,
      // 配置块里没有 log = 没人打开过开关 → 整块回落默认（默认关）。
      log: DEFAULT_LOG_SETTINGS,
    })
  })

  it('treats the single switch as level + hook together', () => {
    // 界面上只有一个 switch：开 = 明细级 + hook 同记，关 = 完全不记。
    // 两个字段必须成对写：只写 level 会留下 hook_enabled:false，hook 那一侧（含
    // 适配器静默忽略那些行）就永远是瞎的。
    expect(logBlockWithSwitch(DEFAULT_LOG_SETTINGS, true)).toMatchObject({
      level: 'verbose',
      hook_enabled: true,
    })
    expect(logBlockWithSwitch(DEFAULT_LOG_SETTINGS, false)).toMatchObject({
      level: 'off',
      hook_enabled: false,
    })
    expect(logSwitchOn(logBlockWithSwitch(DEFAULT_LOG_SETTINGS, true))).toBe(true)
    expect(logSwitchOn(DEFAULT_LOG_SETTINGS)).toBe(false)
  })

  it('keeps the un-exposed log fields when the switch flips', () => {
    // ⚠️ 落盘走 `updateModuleSettings`（模块层浅合并）：整块提交时漏掉的字段会被抹掉，
    // 所以保留期 / 体积上限 / 环形容量这些「界面上没有」的值必须跟着一起写回。
    const current = { ...DEFAULT_LOG_SETTINGS, retention_days: 3, ring_capacity: 900 }
    const next = logBlockWithSwitch(current, true)
    expect(next.retention_days).toBe(3)
    expect(next.ring_capacity).toBe(900)
    expect(next.level).toBe('verbose')
  })

  it('reads a stored log block and clamps each field on its own', () => {
    const settings = agentStatusSettings(
      configWith({
        log: { level: 'verbose', hook_enabled: true, retention_days: 999, ring_capacity: '5000' },
      })
    )
    expect(settings.log.level).toBe('verbose')
    expect(settings.log.hook_enabled).toBe(true)
    // 越界的保留期被夹住，同块里写错类型的环形容量各自回落，不牵连别的字段。
    expect(settings.log.retention_days).toBe(90)
    expect(settings.log.ring_capacity).toBe(DEFAULT_LOG_SETTINGS.ring_capacity)
  })

  it('falls back to recording, not to silence, when a stored level is unreadable', () => {
    // 有 log 块 = 有人想记；`level` 写坏时不能顺着默认值变成「什么都不记」（与 Rust 同口径）。
    const settings = agentStatusSettings(configWith({ log: { level: 'chatty' } }))
    expect(settings.log.level).toBe('normal')
    expect(logSwitchOn(settings.log)).toBe(true)
  })

  it('keeps the status sound on unless it was explicitly switched off', () => {
    // 缺字段 / 写错类型都回落「开」：静音是个明确的动作，不该由坏配置猜出来。
    expect(DEFAULT_AGENT_STATUS_SETTINGS.sound_enabled).toBe(true)
    expect(agentStatusSettings(configWith({})).sound_enabled).toBe(true)
    expect(agentStatusSettings(configWith({ sound_enabled: 'false' })).sound_enabled).toBe(true)
    expect(agentStatusSettings(configWith({ sound_enabled: true })).sound_enabled).toBe(true)
    expect(agentStatusSettings(configWith({ sound_enabled: false })).sound_enabled).toBe(false)
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
