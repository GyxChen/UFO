import { describe, expect, it } from 'vitest'
import type { FloatingModuleConfig } from '@/lib/floating-modules'
import logConfigRs from '../../../src-tauri/src/modules/agent_status/status/log/config.rs?raw'
import {
  AGENT_STATUS_MODULE_ID,
  DEFAULT_AGENT_STATUS_SETTINGS,
  DEFAULT_LOG_SETTINGS,
  MAX_LOG_RETENTION_DAYS,
  MAX_RETENTION_SECONDS,
  MAX_RUNNING_TIMEOUT_SECONDS,
  MIN_LOG_RETENTION_DAYS,
  LOG_RETENTION_DAYS_CHOICES,
  agentStatusSettings,
  logBlockWithSwitch,
  logSwitchOn,
  nearestRetentionChoice,
  retentionMsFromSeconds,
  runningTimeoutMsFromMinutes,
  toMinutes,
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
    // 所以保留期 / 体积上限 / 尾部续读这些「界面上没有」的值必须跟着一起写回。
    const current = { ...DEFAULT_LOG_SETTINGS, retention_days: 14, live_tail_bytes: 1048576 }
    const next = logBlockWithSwitch(current, true)
    expect(next.retention_days).toBe(14)
    expect(next.live_tail_bytes).toBe(1048576)
    expect(next.level).toBe('verbose')
  })

  it('reads a stored log block and clamps each field on its own', () => {
    const settings = agentStatusSettings(
      configWith({
        log: { level: 'verbose', hook_enabled: true, retention_days: 999, live_tail_bytes: '5000' },
      })
    )
    expect(settings.log.level).toBe('verbose')
    expect(settings.log.hook_enabled).toBe(true)
    // 越界的保留期被夹到上限（14），同块里写错类型的续读窗口各自回落，不牵连别的字段。
    expect(settings.log.retention_days).toBe(MAX_LOG_RETENTION_DAYS)
    expect(settings.log.live_tail_bytes).toBe(DEFAULT_LOG_SETTINGS.live_tail_bytes)
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

/**
 * 保留天数是界面上唯一的回收旋钮，它的默认值与上下界都必须与 Rust 同源。
 *
 * 手法照 `push-providers.spec.ts` / `status-log-reasons.spec.ts`：`?raw` 读 Rust 源，正则抠常量再比。
 * 两边不一致的代价是看得见的错报 —— 面板写 7 天、实际留 14 天，或下拉给出一档而后台把它夹掉。
 */
describe('日志保留天数的跨语言口径', () => {
  /** 从 `log/config.rs` 里抠出一个 `const NAME: u32 = N;`。 */
  function rustConst(name: string): number {
    const found = new RegExp(`const ${name}: u32 = (\\d+);`).exec(logConfigRs)
    if (!found) throw new Error(`没能在 log/config.rs 里找到 ${name} —— 它改名或挪文件了`)
    return Number(found[1])
  }

  it('默认值与 Rust 的 DEFAULT_RETENTION_DAYS 同值', () => {
    expect(rustConst('DEFAULT_RETENTION_DAYS')).toBe(7)
    expect(DEFAULT_LOG_SETTINGS.retention_days).toBe(rustConst('DEFAULT_RETENTION_DAYS'))
    expect(DEFAULT_AGENT_STATUS_SETTINGS.log.retention_days).toBe(7)
  })

  it('上下界与 Rust 的 MIN/MAX_RETENTION_DAYS 同值', () => {
    expect(MIN_LOG_RETENTION_DAYS).toBe(rustConst('MIN_RETENTION_DAYS'))
    expect(MAX_LOG_RETENTION_DAYS).toBe(rustConst('MAX_RETENTION_DAYS'))
  })

  it('配置块里越界或写错类型的天数都夹进区间', () => {
    expect(agentStatusSettings(configWith({ log: { retention_days: 999 } })).log.retention_days).toBe(
      MAX_LOG_RETENTION_DAYS
    )
    expect(agentStatusSettings(configWith({ log: { retention_days: 0 } })).log.retention_days).toBe(
      MIN_LOG_RETENTION_DAYS
    )
    // 类型不对（字符串）回落默认，不跟着 '7' 这种写法走。
    expect(
      agentStatusSettings(configWith({ log: { retention_days: '30' } })).log.retention_days
    ).toBe(DEFAULT_LOG_SETTINGS.retention_days)
  })

  /**
   * 界面上那个下拉只有两档（7 / 14），而旧版数字输入框能在盘上留下 1–90 之间的任何数。
   * 「就近取档」必须永远落在两档之内，且两档都在 Rust 的夹取区间里 —— 否则界面显示一个
   * 后端马上又会夹掉的数，就是「面板说 14 天、实际留 X 天」。
   */
  it('下拉只有两档，任意盘上值都就近取到其中一档', () => {
    expect([...LOG_RETENTION_DAYS_CHOICES]).toEqual([7, 14])
    for (const choice of LOG_RETENTION_DAYS_CHOICES) {
      expect(choice).toBeGreaterThanOrEqual(MIN_LOG_RETENTION_DAYS)
      expect(choice).toBeLessThanOrEqual(MAX_LOG_RETENTION_DAYS)
      expect(nearestRetentionChoice(choice, 7)).toBe(choice)
    }

    expect(nearestRetentionChoice(3, 7)).toBe(7)
    expect(nearestRetentionChoice(12, 7)).toBe(14)
    expect(nearestRetentionChoice(999, 7)).toBe(MAX_LOG_RETENTION_DAYS)
    // 空串 / NaN（读不到值）回落当前值，不该顺手改档位。
    expect(nearestRetentionChoice('', 14)).toBe(14)
    expect(nearestRetentionChoice(Number.NaN, 14)).toBe(14)
  })
})

describe('seconds helpers', () => {
  it('converts and clamps the settings page input', () => {
    expect(retentionMsFromSeconds(45)).toBe(45_000)
    expect(retentionMsFromSeconds(0)).toBe(1_000)
    expect(retentionMsFromSeconds(MAX_RETENTION_SECONDS + 1)).toBe(MAX_RETENTION_SECONDS * 1000)
    expect(runningTimeoutMsFromMinutes(2)).toBe(120_000)
    expect(runningTimeoutMsFromMinutes(0)).toBe(60_000)
    expect(runningTimeoutMsFromMinutes(999)).toBe(MAX_RUNNING_TIMEOUT_SECONDS * 1000)
    expect(runningTimeoutMsFromMinutes(Number.NaN)).toBe(
      DEFAULT_AGENT_STATUS_SETTINGS.running_timeout_ms
    )
    expect(toSeconds(45_000)).toBe(45)
    expect(toMinutes(120_000)).toBe(2)
  })
})
