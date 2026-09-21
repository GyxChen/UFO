import { afterEach, describe, expect, it, vi } from 'vitest'
import { isStatusSnapshot } from './agent-status'
import {
  buildMockStatusSnapshot,
  mockSessionCount,
  mockSessionsFlash,
  shouldInjectMockSessions,
} from './mock'

/**
 * 开发期假会话工具的回归：生成器产出合法快照、字段口径与真实后端一致，
 * env 解析对坏值稳健。固定 nowMs 保证输出可复现。
 */
const NOW = 1_700_000_000_000

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('buildMockStatusSnapshot', () => {
  it('产出能过 isStatusSnapshot 校验的合法快照', () => {
    const snapshot = buildMockStatusSnapshot(8, { nowMs: NOW })
    expect(isStatusSnapshot(snapshot)).toBe(true)
    expect(snapshot.sessions).toHaveLength(8)
    expect(snapshot.source_health).toBe('healthy')
  })

  it('counts 与各 phase 数量对齐、总和等于会话数', () => {
    const snapshot = buildMockStatusSnapshot(6, { nowMs: NOW })
    const counts = snapshot.counts
    const sum = counts.running + counts.waiting + counts.error + counts.unknown + counts.completed
    expect(sum).toBe(6)
    // 6 个一轮正好覆盖 running / waiting / completed / error / running / unknown。
    expect(counts.running).toBe(2)
    expect(counts.waiting).toBe(1)
    expect(counts.completed).toBe(1)
    expect(counts.error).toBe(1)
    expect(counts.unknown).toBe(1)
  })

  it('key 唯一且确定（跨窗口悬停目标能对上），provider 覆盖两种', () => {
    const snapshot = buildMockStatusSnapshot(4, { nowMs: NOW })
    const keys = snapshot.sessions.map((s) => s.key)
    expect(new Set(keys).size).toBe(4)
    expect(new Set(snapshot.sessions.map((s) => s.provider))).toEqual(new Set(['claude', 'codex']))
  })

  it('默认只有「完成」会话闪，几种灯态不被闪盖住', () => {
    const snapshot = buildMockStatusSnapshot(6, { nowMs: NOW })
    const flashing = snapshot.sessions.filter((s) => s.is_flashing).map((s) => s.phase)
    expect(flashing).toEqual(['completed'])
  })

  it('flashing=false 时无人闪', () => {
    const snapshot = buildMockStatusSnapshot(6, { nowMs: NOW, flashing: false })
    expect(snapshot.sessions.some((s) => s.is_flashing)).toBe(false)
  })

  it('固定 nowMs 时输出完全可复现', () => {
    expect(buildMockStatusSnapshot(3, { nowMs: NOW })).toEqual(buildMockStatusSnapshot(3, { nowMs: NOW }))
  })
})

describe('mockSessionCount', () => {
  it('解析正整数', () => {
    vi.stubEnv('VITE_AGENT_STATUS_MOCK_COUNT', '8')
    expect(mockSessionCount()).toBe(8)
  })

  it('夹到上限 64', () => {
    vi.stubEnv('VITE_AGENT_STATUS_MOCK_COUNT', '999')
    expect(mockSessionCount()).toBe(64)
  })

  it('0 / 负数 / 非数字 / 未设置一律按 0', () => {
    vi.stubEnv('VITE_AGENT_STATUS_MOCK_COUNT', '0')
    expect(mockSessionCount()).toBe(0)
    vi.stubEnv('VITE_AGENT_STATUS_MOCK_COUNT', '-3')
    expect(mockSessionCount()).toBe(0)
    vi.stubEnv('VITE_AGENT_STATUS_MOCK_COUNT', 'abc')
    expect(mockSessionCount()).toBe(0)
    vi.stubEnv('VITE_AGENT_STATUS_MOCK_COUNT', undefined as unknown as string)
    expect(mockSessionCount()).toBe(0)
  })
})

describe('mockSessionsFlash', () => {
  it('未设置默认 true', () => {
    vi.stubEnv('VITE_AGENT_STATUS_MOCK_FLASHING', undefined as unknown as string)
    expect(mockSessionsFlash()).toBe(true)
  })

  it('只有显式 false/0/no 才关', () => {
    vi.stubEnv('VITE_AGENT_STATUS_MOCK_FLASHING', 'false')
    expect(mockSessionsFlash()).toBe(false)
    vi.stubEnv('VITE_AGENT_STATUS_MOCK_FLASHING', '0')
    expect(mockSessionsFlash()).toBe(false)
    vi.stubEnv('VITE_AGENT_STATUS_MOCK_FLASHING', 'true')
    expect(mockSessionsFlash()).toBe(true)
  })
})

describe('shouldInjectMockSessions', () => {
  it('开发构建 + 数量 >0 才注入', () => {
    // vitest 下 import.meta.env.DEV 默认为 true。
    vi.stubEnv('VITE_AGENT_STATUS_MOCK_COUNT', '8')
    expect(shouldInjectMockSessions()).toBe(true)
    vi.stubEnv('VITE_AGENT_STATUS_MOCK_COUNT', '0')
    expect(shouldInjectMockSessions()).toBe(false)
  })

  it('数量 >0 但非开发构建（DEV=false）不注入', () => {
    vi.stubEnv('VITE_AGENT_STATUS_MOCK_COUNT', '8')
    vi.stubEnv('DEV', false)
    expect(shouldInjectMockSessions()).toBe(false)
  })
})
