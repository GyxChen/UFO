import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
// ?raw 读 Rust 源：事件名两边各写一遍，这里断言的正是「文件里写着什么」。
import commandsRaw from '../../src-tauri/src/plugin/commands.rs?raw'
import { PLUGINS_CHANGED_EVENT } from '@/lib/plugin-events'
import type { CapabilityFactJson, LedgerBroadcastJson, PluginRecordJson } from '@/lib/plugin-types'
import { usePluginsStore } from './plugins'

const { invokeMock, listenMock, calls } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  /** 记调用序：`start()` 必须先订阅再首拉，否则中间发生的变更会永久丢。 */
  calls: [] as string[],
  listenMock: vi.fn(async () => {
    calls.push('listen')
    return () => {}
  }),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock, emitTo: vi.fn(async () => {}) }))

/** 一条最小的能力事实（徽标渲染只看 id / label / tone / icon）。 */
function fact(id: string): CapabilityFactJson {
  return { id, kind: id === 'cell' || id === 'settings' ? 'surface' : 'permission', label: id, icon: 'grid', tone: 'local', disclosure: `${id} 的披露文案` }
}

/** 一条已装记录（只填测试关心的字段）。 */
function record(patch: Partial<PluginRecordJson> = {}): PluginRecordJson {
  return {
    id: 'balance-board',
    name: '余额看板',
    version: '1.2.0',
    manifest: {
      id: 'balance-board',
      name: '余额看板',
      version: '1.2.0',
      description: '',
      entry: 'main.js',
      surfaces: { cell: 2, tooltip: { own: true, size: [340, 260] }, settings: true },
      permissions: ['http', 'storage'],
      hosts: ['api.example.com'],
    },
    dir: 'C:/plugins/balance-board',
    source: 'local',
    devReload: false,
    warnings: [],
    files: 2,
    bytes: 1024,
    loadMs: 0,
    error: null,
    ...patch,
  }
}

/** 一次广播。 */
function broadcast(revision: number, records: PluginRecordJson[]): LedgerBroadcastJson {
  return { revision, records }
}

beforeEach(() => {
  setActivePinia(createPinia())
  calls.length = 0
  invokeMock.mockReset()
  listenMock.mockClear()
})

describe('事件名与 Rust 同源', () => {
  it('Rust 里的事件名常量与前端那份是同一个串', () => {
    const source = commandsRaw.replace(/\r\n/g, '\n')
    const at = source.indexOf('pub const PLUGINS_CHANGED_EVENT')
    expect(at, 'Rust 侧少了这个常量').toBeGreaterThanOrEqual(0)
    const literal = source.slice(at).match(/"([^"]+)"/)
    expect(literal?.[1]).toBe(PLUGINS_CHANGED_EVENT)
  })
})

describe('plugins store 启动', () => {
  it('先订阅再首拉', async () => {
    invokeMock.mockImplementation((command: string) => {
      calls.push(`invoke:${command}`)
      return Promise.resolve(command === 'get_plugin_registry' ? [fact('cell')] : broadcast(1, []))
    })
    const store = usePluginsStore()
    await store.start()
    expect(calls[0]).toBe('listen')
    expect(calls).toEqual(['listen', 'invoke:get_plugin_registry', 'invoke:get_plugins'])
    expect(store.connection).toBe('connected')
  })

  it('重复 start 不会再订阅一次', async () => {
    invokeMock.mockResolvedValue(broadcast(1, []))
    const store = usePluginsStore()
    await store.start()
    await store.start()
    expect(listenMock).toHaveBeenCalledTimes(1)
  })

  it('IPC 不可用时退化成 degraded，而不是抛到界面', async () => {
    invokeMock.mockRejectedValue(new Error('state not managed'))
    const store = usePluginsStore()
    await store.start()
    expect(store.connection).toBe('degraded')
    expect(store.records).toEqual([])
  })
})

describe('安装记录的跟随规则', () => {
  it('迟到的广播被丢弃（revision 更小就不覆盖新状态）', () => {
    const store = usePluginsStore()
    store.applyBroadcast(broadcast(5, [record()]))
    store.applyBroadcast(broadcast(4, [record({ id: 'old', name: '旧的' })]))
    expect(store.revision).toBe(5)
    expect(store.records.map(item => item.id)).toEqual(['balance-board'])
  })

  it('等版本广播仍然应用（同一版的重扫要认）', () => {
    const store = usePluginsStore()
    store.applyBroadcast(broadcast(3, [record({ devReload: false })]))
    store.applyBroadcast(broadcast(3, [record({ devReload: true })]))
    expect(store.debugCount).toBe(1)
  })

  it('挂载清单只收「清单校验通过」的包', () => {
    const store = usePluginsStore()
    store.applyBroadcast(
      broadcast(1, [
        record({ id: 'a' }),
        record({ id: 'b', manifest: null, error: '清单缺少 entry' }),
        record({ id: 'c', manifest: null }),
      ])
    )
    expect(store.mountedRecords.map(item => item.id)).toEqual(['a'])
  })

  it('选中的包被卸载后，右栏回到还在的那条', () => {
    const store = usePluginsStore()
    store.applyBroadcast(broadcast(1, [record({ id: 'a' }), record({ id: 'b' })]))
    store.select('a')
    store.applyBroadcast(broadcast(2, [record({ id: 'b' })]))
    expect(store.selectedId).toBe('b')
  })
})

describe('徽标推导', () => {
  beforeEach(() => {
    usePluginsStore().$dispose()
  })

  it('挂载点在前、能力在后，未登记的名字跳过', () => {
    const store = usePluginsStore()
    store.registry = [fact('cell'), fact('settings'), fact('http')]
    const badges = store.badgesOf(record())
    // 清单里声明了 tooltip-own，但注册表没有它 → 不硬凑一枚空白徽标。
    expect(badges.map(item => item.id)).toEqual(['cell', 'settings', 'http'])
  })

  it('坏包（manifest 为 null）一枚徽标都不出，改由列表行说原因', () => {
    const store = usePluginsStore()
    store.registry = [fact('cell')]
    expect(store.badgesOf(record({ manifest: null }))).toEqual([])
  })
})

describe('写命令的失败口径', () => {
  it('失败原因留在行内提示，且不破坏已有安装记录', async () => {
    const store = usePluginsStore()
    store.applyBroadcast(broadcast(1, [record()]))
    invokeMock.mockRejectedValue('只有本地目录安装的组件能开热重载')
    await expect(store.setDevReload('balance-board', true)).rejects.toBeTruthy()
    expect(store.actionError).toBe('只有本地目录安装的组件能开热重载')
    expect(store.records).toHaveLength(1)
    expect(store.records[0].devReload).toBe(false)
  })

  it('成功后应用返回的安装记录并清掉旧错误', async () => {
    const store = usePluginsStore()
    invokeMock.mockResolvedValue(broadcast(2, [record({ manifest: null, error: '清单校验失败' })]))
    await store.setDevReload('balance-board', true)
    expect(store.actionError).toBe('')
    expect(store.revision).toBe(2)
    expect(store.mountedRecords).toEqual([])
  })

  it('上报装载耗时不打扰用户（失败只进 console）', async () => {
    const store = usePluginsStore()
    invokeMock.mockRejectedValue(new Error('nope'))
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    store.reportLoad('balance-board', 42.7)
    await Promise.resolve()
    await Promise.resolve()
    expect(invokeMock).toHaveBeenCalledWith('report_plugin_load_ms', { id: 'balance-board', loadMs: 43 })
    expect(store.actionError).toBe('')
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })
})
