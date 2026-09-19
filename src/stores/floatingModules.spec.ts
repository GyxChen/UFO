// @vitest-environment happy-dom

/**
 * 外壳配置 store 的启动路径：**首帧 IPC 可能比后端状态注册更早**，
 * 这里锁住「失败要重试、重试用尽才退化成兜底配置」。
 */
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useFloatingModulesStore } from './floatingModules'
import type { FloatingModuleConfig } from '@/lib/floating-modules'

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(async () => () => {}),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock, emitTo: vi.fn(async () => {}) }))

/** 后端真配置：带一个启用的组件，用来区分「拿到真配置」与「兜底配置」。 */
function backendConfig(): FloatingModuleConfig {
  return {
    schema_version: 1,
    revision: 7,
    modules: [{ id: 'stub-module', enabled: true, order: 0 }],
    max_cells_by_layout: { square: 8, corner: 8, horizontal: 8, vertical: 8 },
    module_settings: {},
  }
}

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  listenMock.mockClear()
})

describe('floatingModules store 启动', () => {
  it('首帧 IPC 失败后重试，拿到真配置就不会退化成兜底配置', async () => {
    invokeMock
      .mockRejectedValueOnce(new Error('state not managed for field `state`'))
      .mockResolvedValueOnce(backendConfig())

    const store = useFloatingModulesStore()
    await store.start()

    expect(invokeMock).toHaveBeenCalledTimes(2)
    expect(store.connection).toBe('connected')
    expect(store.config?.modules).toEqual([{ id: 'stub-module', enabled: true, order: 0 }])
  })

  it('重试用尽仍失败时落到兜底配置并标记 degraded', async () => {
    invokeMock.mockRejectedValue(new Error('state not managed for field `state`'))

    const store = useFloatingModulesStore()
    await store.start()

    expect(invokeMock.mock.calls.length).toBeGreaterThan(1)
    expect(store.connection).toBe('degraded')
    // 兜底配置不点名任何模块：启用状态一律回落到各组件定义里的默认值。
    expect(store.config?.modules).toEqual([])
  })

  it('成功后再 start 不会重复订阅', async () => {
    invokeMock.mockResolvedValue(backendConfig())

    const store = useFloatingModulesStore()
    await store.start()
    await store.start()

    expect(listenMock).toHaveBeenCalledTimes(1)
  })
})
