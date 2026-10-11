/**
 * `host` 的行为护栏。
 *
 * 为什么这些断言值得写：`host` 是插件唯一的能力入口，它的失败方式都不是「编译报错」——
 * - 漏一次声明检查 → 组件能用到没申请的宿主功能，权限徽标当场变成装饰；
 * - `get/post` 写成 `this.request` → 只在作者解构时炸（`const { get } = host.http`），单测不拆就看不到；
 * - 轮询没停 → 包卸载后每秒还在拉事件，表现是后台空转；
 * - 参数名与 Rust 差一个字母 → 运行时才报 invalid args，真窗里才看得见。
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPluginHost, PLUGIN_HOST_KEY } from '@/lib/plugin-host'
import hostRaw from './plugin-host.ts?raw'
import bleRaw from '../../src-tauri/src/plugin/ble.rs?raw'
import type { PluginAgentEvent, PluginHost } from '@/lib/plugin-host'
import { PLUGIN_ORIGIN } from '@/lib/plugin-runtime'
import { PLUGIN_LIMITS } from '@/lib/plugin-limits'
import { clearPluginTooltipSizes, pluginTooltipSizeOf } from '@/lib/plugin-tooltip-size'
import type { PluginRecordJson, PluginSurfacesJson } from '@/lib/plugin-types'

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }))

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))

/** 造一条记录：`permissions` / `hosts` 是这些测试唯一关心的清单字段。 */
function record(
  permissions: string[],
  hosts: string[] = ['api.example.com'],
  surfaces: PluginSurfacesJson = { cell: 1 }
): PluginRecordJson {
  return {
    id: 'balance-board',
    name: '余额看板',
    version: '1.2.0',
    manifest: {
      id: 'balance-board',
      name: '余额看板',
      version: '1.2.0',
      description: '看看余额',
      entry: 'main.js',
      surfaces,
      permissions,
      hosts,
    },
    dir: 'C:/plugins/balance-board',
    source: 'local',
    devReload: false,
    warnings: [],
    files: 1,
    bytes: 100,
    loadMs: 0,
    error: null,
  }
}

/** 全能力声明的 host（各条测试只改自己那一档）。 */
function fullHost(): PluginHost {
  return createPluginHost(
    record(['storage', 'http', 'events:agent', 'fs', 'secrets', 'frame', 'ble', 'serial'])
  )
}

/** 声明了 `surfaces.tooltip.own` 的 host：运行时改卡尺寸只有这一条路进得去。 */
function ownCardHost(): PluginHost {
  return createPluginHost(record([], ['api.example.com'], { tooltip: { own: true, size: [300, 300] } }))
}

/** 取某条命令**最近一次**调用的参数（断言「命令名 + 逐字段参数」与 Rust 一致）。 */
function callArg(cmd: string) {
  for (let i = invokeMock.mock.calls.length - 1; i >= 0; i -= 1) {
    if (invokeMock.mock.calls[i]?.[0] === cmd) return invokeMock.mock.calls[i]?.[1] as Record<string, unknown>
  }
  return undefined
}

beforeEach(() => {
  invokeMock.mockReset()
  invokeMock.mockResolvedValue(undefined)
  // 运行时卡尺寸是模块级状态：不清就会把上一条测试的申请带进下一条。
  clearPluginTooltipSizes()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('host 的形状与冻结', () => {
  it('只暴露注册过的能力，且不把 invoke 递出去', () => {
    // 白名单断言：以后谁加 `host.invoke` / `host.channel`，这一条先红。
    expect(Object.keys(fullHost()).sort()).toEqual([
      'ble',
      'event',
      'frame',
      'fs',
      'http',
      'log',
      'plugin',
      'secrets',
      'serial',
      'storage',
      'tooltip',
    ])
    const raw = fullHost() as unknown as Record<string, unknown>
    expect(raw.invoke).toBeUndefined()
    expect(raw.__tauri__).toBeUndefined()
  })

  it('整体与逐层都是冻结的：改不动自己的 storage，也改不动别人的', () => {
    const host = fullHost()
    expect(Object.isFrozen(host)).toBe(true)
    // 只冻一层是不够的：`host.storage.set = ...` 动的还是共享对象。
    for (
      const key of ['plugin', 'storage', 'http', 'fs', 'secrets', 'frame', 'ble', 'serial', 'event', 'tooltip'] as const
    ) {
      expect(Object.isFrozen(host[key]), `host.${key} 没冻住`).toBe(true)
    }
    expect(() => {
      ;(host.storage as { get: unknown }).get = () => Promise.resolve('伪造')
    }).toThrow()
  })

  it('plugin 段是清单的只读副本（作者改 permissions 数组也不会多出门）', () => {
    const host = createPluginHost(record(['storage']))
    expect(host.plugin.id).toBe('balance-board')
    expect(host.plugin.permissions).toEqual(['storage'])
    expect(Object.isFrozen(host.plugin.permissions)).toBe(true)
    expect(() => {
      ;(host.plugin.permissions as string[]).push('http')
    }).toThrow()
  })

  it('host 走 Symbol.for：产物与宿主各自打包也能 inject 到同一个键', () => {
    expect(PLUGIN_HOST_KEY).toBe(Symbol.for('ufo.plugin.host'))
  })
})

describe('没声明的能力一用就抛', () => {
  /** 六档能力各断一次「点名了哪一项」——只测拒绝、不测原因的话就挡不住改错文案。 */
  it.each([
    ['storage', (host: PluginHost) => host.storage.get('k')],
    ['http', (host: PluginHost) => host.http.request('GET', 'https://api.example.com')],
    ['events:agent', (host: PluginHost) => host.event.open()],
    ['fs', (host: PluginHost) => host.fs.pick()],
    ['secrets', (host: PluginHost) => host.secrets.get('API_KEY')],
    ['frame', (host: PluginHost) => host.frame.allow('https://example.com')],
    ['ble', (host: PluginHost) => host.ble.scan(2000)],
    ['serial', (host: PluginHost) => host.serial.ports()],
  ] as const)('%s 没写进清单时抛错，并且命令一次都没发', async (permission, use) => {
    const host = createPluginHost(record([]))
    await expect(use(host)).rejects.toThrow(permission)
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('声明了就放行，并把 id 一起交给命令层（闸在 Rust，按这个 id 查清单）', async () => {
    invokeMock.mockResolvedValue('v')
    const host = createPluginHost(record(['storage']))
    await expect(host.storage.get('token')).resolves.toBe('v')
    expect(callArg('plugin_storage_get')).toEqual({ id: 'balance-board', key: 'token' })
  })

  it('log 是特例：不声明任何权限也能打，且不查清单', () => {
    const host = createPluginHost(record([]))
    expect(() => host.log('info', 'hello')).not.toThrow()
    expect(callArg('plugin_log_write')).toEqual({
      id: 'balance-board',
      level: 'info',
      msg: 'hello',
      fields: null,
    })
  })

  it('未知级别夹成 info 再写（日志窗只认三档，原样递过去那行就筛不到了）', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    const host = createPluginHost(record([]))
    host.log('verbose' as 'info', 'x')
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('verbose'))
    expect(callArg('plugin_log_write')).toMatchObject({ level: 'info', msg: 'x' })
    warn.mockRestore()
  })
})

describe('storage 的参数形状', () => {
  it('五个方法各自打对应命令，参数名与 Rust 一致', async () => {
    invokeMock.mockResolvedValue(undefined)
    const host = createPluginHost(record(['storage']))
    await host.storage.set('k', { a: 1 })
    await host.storage.remove('k')
    await host.storage.keys()
    await host.storage.usage()
    expect(callArg('plugin_storage_set')).toEqual({ id: 'balance-board', key: 'k', value: { a: 1 } })
    expect(callArg('plugin_storage_remove')).toEqual({ id: 'balance-board', key: 'k' })
    expect(callArg('plugin_storage_keys')).toEqual({ id: 'balance-board' })
    expect(callArg('get_plugin_storage_usage')).toEqual({ id: 'balance-board' })
  })
})

describe('http 的规范化', () => {
  it('方法转大写、缺省超时 10000、无体传 null', async () => {
    invokeMock.mockResolvedValue({ status: 200, body: '', headers: {}, truncated: false })
    const host = fullHost()
    await host.http.request('post', 'https://api.example.com/x', 'body', {
      headers: { 'X-A': '1' },
      timeoutMs: 3000,
    })
    expect(callArg('plugin_http_fetch')).toEqual({
      id: 'balance-board',
      request: { method: 'POST', url: 'https://api.example.com/x', headers: { 'X-A': '1' }, body: 'body', timeoutMs: 3000 },
    })
    await host.http.request('GET', 'https://api.example.com/y')
    expect(callArg('plugin_http_fetch')?.request).toEqual({
      method: 'GET',
      url: 'https://api.example.com/y',
      headers: {},
      body: null,
      timeoutMs: 10_000,
    })
  })

  /** 作者常写 `const { get } = host.http`：这一条就是那条解构陷阱的护栏。 */
  it('get / post 拆下来单独用也发得出请求（不依赖 this）', async () => {
    invokeMock.mockResolvedValue({ status: 200, body: 'ok', headers: {}, truncated: false })
    const { get, post } = fullHost().http
    await expect(get('https://api.example.com/a')).resolves.toMatchObject({ body: 'ok' })
    expect(callArg('plugin_http_fetch')?.request).toMatchObject({ method: 'GET', body: null })
    await post('https://api.example.com/b', '{}')
    expect(callArg('plugin_http_fetch')?.request).toMatchObject({ method: 'POST', body: '{}' })
  })
})

describe('事件订阅的生命周期', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  /** 一条最小事件。 */
  function event(seq: number): PluginAgentEvent {
    return {
      seq,
      ts: 1,
      topic: 'light_changed',
      key: 'k',
      provider: 'claude',
      sessionId: 's',
      phase: 'running',
      previousPhase: 'idle',
      endpoint: null,
      prompt: null,
      startedAtMs: 0,
      lastSeenAtMs: 0,
      errorCode: null,
      waitingReason: null,
    }
  }

  it('open 记下主题并按秒开轮询，close 之后不再拉', async () => {
    invokeMock.mockResolvedValue(['light_changed'])
    const host = fullHost()
    await expect(host.event.open()).resolves.toEqual(['light_changed'])
    expect(callArg('open_plugin_events')).toEqual({ id: 'balance-board' })

    invokeMock.mockResolvedValue({ events: [], nextSeq: 0 })
    await vi.advanceTimersByTimeAsync(1000)
    const polling = invokeMock.mock.calls.filter(([name]) => name === 'poll_plugin_events')
    expect(polling).toHaveLength(1)

    await host.event.close()
    expect(callArg('close_plugin_events')).toEqual({ id: 'balance-board' })
    invokeMock.mockClear()
    await vi.advanceTimersByTimeAsync(5000)
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('每条新事件回调一次，游标跟着 nextSeq 走（不会把同一条重发）', async () => {
    invokeMock.mockResolvedValue(['light_changed'])
    const host = fullHost()
    await host.event.open()

    const seen: number[] = []
    host.event.subscribe(e => seen.push(e.seq))
    invokeMock.mockResolvedValue({ events: [event(7), event(8)], nextSeq: 9 })
    await vi.advanceTimersByTimeAsync(1000)
    expect(seen).toEqual([7, 8])
    expect(callArg('poll_plugin_events')).toEqual({ id: 'balance-board', since: 0 })

    invokeMock.mockResolvedValue({ events: [], nextSeq: 9 })
    await vi.advanceTimersByTimeAsync(1000)
    expect(callArg('poll_plugin_events')).toEqual({ id: 'balance-board', since: 9 })
  })

  it('退订只拿走自己那一个回调', async () => {
    invokeMock.mockResolvedValue(['light_changed'])
    const host = fullHost()
    await host.event.open()
    const a = vi.fn()
    const off = host.event.subscribe(a)
    off()
    host.event.subscribe(() => undefined)
    invokeMock.mockResolvedValue({ events: [event(1)], nextSeq: 2 })
    await vi.advanceTimersByTimeAsync(1000)
    expect(a).not.toHaveBeenCalled()
  })

  it('一个回调抛错不影响别的回调，也不打断轮询', async () => {
    invokeMock.mockResolvedValue(['light_changed'])
    const host = fullHost()
    await host.event.open()
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    const later = vi.fn()
    host.event.subscribe(() => {
      throw new Error('组件自己炸了')
    })
    host.event.subscribe(later)
    invokeMock.mockResolvedValue({ events: [event(1)], nextSeq: 2 })
    await vi.advanceTimersByTimeAsync(1000)
    expect(later).toHaveBeenCalledTimes(1)
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('事件回调抛错'), expect.anything())
    warn.mockRestore()
  })

  it('拉取失败就停轮询（包被停用时不该一秒抛一次）', async () => {
    invokeMock.mockResolvedValue(['light_changed'])
    const host = fullHost()
    await host.event.open()
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    invokeMock.mockRejectedValue(new Error('没订阅成功'))
    await vi.advanceTimersByTimeAsync(1000)
    invokeMock.mockClear()
    await vi.advanceTimersByTimeAsync(5000)
    expect(invokeMock).not.toHaveBeenCalled()
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('事件拉取停了'), expect.anything())
    warn.mockRestore()
  })

  it('重复 open 不会开出第二个轮询', async () => {
    invokeMock.mockResolvedValue(['light_changed'])
    const host = fullHost()
    await host.event.open()
    await host.event.open()
    invokeMock.mockResolvedValue({ events: [], nextSeq: 0 })
    await vi.advanceTimersByTimeAsync(1000)
    expect(invokeMock.mock.calls.filter(([name]) => name === 'poll_plugin_events')).toHaveLength(1)
  })
})

/**
 * `host.fs` 的编码与 URL 形状。
 *
 * 这一档最容易「代码看着对、真机上读不出来」：base64 与命令层的参数名错了不会编译失败，
 * 而 URL 少编一段会让条目名里的 `/` 变成路径分隔符 —— 表现是「只有根目录的条目读得到」。
 */
describe('fs 的编码与直链形状', () => {
  it('pick 把句柄换成可用的直链', async () => {
    invokeMock.mockResolvedValue({ handle: 'probe-1-abc', name: 'book.epub', size: 4096, archive: true })
    const picked = await fullHost().fs.pick('选一本书')
    expect(callArg('plugin_fs_pick')).toEqual({ id: 'balance-board', title: '选一本书' })
    expect(picked.url).toBe(`${PLUGIN_ORIGIN}/__pick/probe-1-abc`)
    expect(picked.size).toBe(4096)
  })

  it('read 把命令层的 base64 换成字节', async () => {
    invokeMock.mockResolvedValue('aGk=')
    const bytes = await fullHost().fs.read('probe-1-abc', 0, 2)
    expect(Array.from(bytes)).toEqual([104, 105])
    expect(callArg('plugin_fs_read')).toEqual({ id: 'balance-board', handle: 'probe-1-abc', start: 0, len: 2 })
  })

  it('write 按数据类型选编码：字符串走 utf8，字节走 base64', async () => {
    invokeMock.mockResolvedValue({ name: 'note.txt', size: 2 })
    const host = fullHost()
    await host.fs.write('note.txt', 'hi')
    expect(callArg('plugin_fs_write')).toEqual({
      id: 'balance-board',
      path: 'note.txt',
      data: 'hi',
      encoding: 'utf8',
    })
    await host.fs.write('raw.bin', new Uint8Array([0, 1, 250, 255]))
    expect(callArg('plugin_fs_write')).toEqual({
      id: 'balance-board',
      path: 'raw.bin',
      data: 'AAH6/w==',
      encoding: 'base64',
    })
  })

  /** 条目名必须整段编码：不编码的话 `OEBPS/Text/c1.xhtml` 会变成三段路径。 */
  it('容器条目的直链把条目名放进 query 并整段编码', () => {
    const host = fullHost()
    expect(host.fs.entryUrl('probe-1-abc', 'OEBPS/Text/c1.xhtml')).toBe(
      `${PLUGIN_ORIGIN}/__pick/probe-1-abc?entry=OEBPS%2FText%2Fc1.xhtml`
    )
    expect(host.fs.privateUrl('cache/note.txt')).toBe(`${PLUGIN_ORIGIN}/__fs/balance-board/cache/note.txt`)
    // 私有目录的路径逐段编码：段名里的空格要编，分隔符要留。
    expect(host.fs.privateUrl('cache/my note.txt')).toBe(
      `${PLUGIN_ORIGIN}/__fs/balance-board/cache/my%20note.txt`
    )
  })

  it('secrets 只把名字交给命令层，值不参与任何日志', async () => {
    invokeMock.mockResolvedValue('hunter2')
    const host = fullHost()
    await expect(host.secrets.get('API_KEY')).resolves.toBe('hunter2')
    expect(callArg('plugin_secret_get')).toEqual({ id: 'balance-board', name: 'API_KEY' })
    await host.secrets.set('API_KEY', 'hunter2')
    expect(callArg('plugin_secret_set')).toEqual({ id: 'balance-board', name: 'API_KEY', value: 'hunter2' })
  })
})

/**
 * 外设两档（`host.ble` / `host.serial`）的编解码与参数形状。
 *
 * 这两档最容易出现「Rust 与前端各说一套」：通知的值在 Rust 侧是 base64、字段是 camelCase，
 * 而作者要的是字节。转换必须只在这一层发生一次，测试钉住它。
 */
describe('外设的字节与参数形状', () => {
  it('蓝牙通知把 base64 换成字节，字段按 camelCase 读', async () => {
    invokeMock.mockResolvedValue([
      { uuid: 'u1', serviceUuid: 's1', value: 'aGk=', queuedBefore: 3 },
    ])
    const rows = await fullHost().ble.notifications('probe-1-abc', 4)
    expect(callArg('plugin_ble_notifications')).toEqual({ id: 'balance-board', handle: 'probe-1-abc', limit: 4 })
    expect(Array.from(rows[0].value)).toEqual([104, 105])
    expect(rows[0].queuedBefore).toBe(3)
    expect(rows[0].serviceUuid).toBe('s1')
  })

  /**
   * 广播包解析出的字段必须整份透出，且字节段在宿主这一层就换成 Uint8Array：
   * 作者若还得自己 atob，就等于把命令层的编码格式漏给了组件包。
   */
  it('scan 把广播的字节段解成 Uint8Array，其余字段原样给', async () => {
    invokeMock.mockResolvedValue([
      {
        handle: 'probe-1-abc',
        address: 'AA:BB:CC:DD:EE:FF',
        name: '手环',
        rssi: -60,
        advert: {
          advertisementName: '手环',
          localName: 'Band',
          appearance: 832,
          txPower: -40,
          addressType: 'random',
          deviceClass: null,
          serviceUuids: ['0000180d-0000-1000-8000-00805f9b34fb'],
          manufacturerData: [{ companyId: 76, data: 'AQUF' }],
          serviceData: [{ uuid: '6e400001-b5a3-f393-e0a9-e50e24dcca9e', data: 'AAH6/w==' }],
        },
      },
    ])
    const rows = await fullHost().ble.scan(2000)
    expect(callArg('plugin_ble_scan')).toEqual({ id: 'balance-board', ms: 2000 })
    expect(rows[0].advert.advertisementName).toBe('手环')
    expect(rows[0].advert.localName).toBe('Band')
    expect(rows[0].advert.appearance).toBe(832)
    expect(rows[0].advert.txPower).toBe(-40)
    expect(rows[0].advert.addressType).toBe('random')
    expect(rows[0].advert.deviceClass).toBeNull()
    expect(rows[0].advert.serviceUuids).toEqual(['0000180d-0000-1000-8000-00805f9b34fb'])
    expect(Array.from(rows[0].advert.manufacturerData[0].data)).toEqual([1, 5, 5])
    expect(Array.from(rows[0].advert.serviceData[0].data)).toEqual([0, 1, 250, 255])
  })

  it('蓝牙写把字符串按 utf8 编 base64，回执开关默认要（与多数设备的期望一致）', async () => {
    invokeMock.mockResolvedValue(2)
    const host = fullHost()
    await host.ble.write('probe-1-abc', '6e400001-b5a3-f393-e0a9-e50e24dcca9e', '6e400002-b5a3-f393-e0a9-e50e24dcca9e', 'hi')
    expect(callArg('plugin_ble_write')).toEqual({
      id: 'balance-board',
      handle: 'probe-1-abc',
      service: '6e400001-b5a3-f393-e0a9-e50e24dcca9e',
      characteristic: '6e400002-b5a3-f393-e0a9-e50e24dcca9e',
      data: 'aGk=',
      withResponse: true,
    })
    await host.ble.write('probe-1-abc', 's', 'c', new Uint8Array([0, 255]), false)
    expect(callArg('plugin_ble_write')).toMatchObject({ data: 'AP8=', withResponse: false })
  })

  it('串口写走同一套编码；读到空串是「这一轮没数据」而不是报错', async () => {
    invokeMock.mockResolvedValue(2)
    const host = fullHost()
    await host.serial.write('probe-1-abc', 'hi')
    expect(callArg('plugin_serial_write')).toEqual({ id: 'balance-board', handle: 'probe-1-abc', data: 'aGk=' })
    invokeMock.mockResolvedValue('')
    expect((await host.serial.read('probe-1-abc', 64)).length).toBe(0)
    expect(callArg('plugin_serial_read')).toEqual({ id: 'balance-board', handle: 'probe-1-abc', max: 64 })
  })

  it('串口打开把没给的超时传成 null（由宿主给默认值，而不是前端抄一份）', async () => {
    invokeMock.mockResolvedValue({ handle: 'probe-1-abc', name: 'COM3', baud: 115200 })
    await fullHost().serial.open('COM3', 115200)
    expect(callArg('plugin_serial_open')).toEqual({
      id: 'balance-board',
      path: 'COM3',
      baud: 115200,
      timeoutMs: null,
    })
  })
})

describe('host.tooltip.setSize', () => {
  it('没声明 surfaces.tooltip.own 就抛：改不了不属于自己的卡', () => {
    // 这条判据是「授权来自挂载点声明」的落点：去掉它，任何包都能把提示窗撑大。
    expect(() => fullHost().tooltip.setSize({ width: 300, height: 300 })).toThrow(/tooltip\.own/)
    expect(pluginTooltipSizeOf('balance-board')).toBeNull()
  })

  it('申请值被夹进清单同一对常量，并把生效的那一份报回来', () => {
    // 数值本身由 `plugin-limits.spec.ts` 手写字面量钉住；这里只断言「超了必被夹」。
    const host = ownCardHost()
    expect(host.tooltip.setSize({ width: 9999, height: 9999 })).toEqual({
      width: PLUGIN_LIMITS.MAX_TOOLTIP_WIDTH,
      height: PLUGIN_LIMITS.MAX_TOOLTIP_HEIGHT,
    })
    expect(host.tooltip.setSize({ width: 1, height: 1 })).toEqual({
      width: PLUGIN_LIMITS.MIN_TOOLTIP_WIDTH,
      height: PLUGIN_LIMITS.MIN_TOOLTIP_HEIGHT,
    })
    expect(host.tooltip.setSize({ width: 400, height: 600 })).toEqual({ width: 400, height: 600 })
    expect(pluginTooltipSizeOf('balance-board')).toEqual({ width: 400, height: 600 })
  })

  it('不是两个数就抛：NaN / 缺字段都不许落成尺寸', () => {
    const host = ownCardHost()
    expect(() => host.tooltip.setSize({ width: NaN, height: 300 })).toThrow(/width/)
    expect(() => host.tooltip.setSize({ height: 300 } as never)).toThrow(/width/)
    expect(pluginTooltipSizeOf('balance-board')).toBeNull()
  })

  it('按包各记各的：一个包改尺寸不顶掉另一个包的卡', () => {
    // `record()` 的 id 是写死的，第二个包要自己换 id —— 尺寸是按 pluginId 存的。
    const other = createPluginHost({
      ...record([], ['api.example.com'], { tooltip: { own: true } }),
      id: 'clock24',
    })
    ownCardHost().tooltip.setSize({ width: 360, height: 500 })
    other.tooltip.setSize({ width: 260, height: 200 })
    expect(pluginTooltipSizeOf('balance-board')).toEqual({ width: 360, height: 500 })
    expect(pluginTooltipSizeOf('clock24')).toEqual({ width: 260, height: 200 })
  })
})

/**
 * 广播字段的跨语言同源护栏。
 *
 * `host.ble.scan` 的返回体在 Rust 是 serde 的 camelCase、在前端是手抄的 interface，两边没有
 * 任何编译期联系：Rust 改一个字段名，前端只会读到 undefined，表现是「这台设备就是不带厂商数据」
 * —— 不上真机永远看不见。所以两边的字段集合必须按名字对撞，少一边、多一边都算红。
 */
describe('广播字段两边同名', () => {
  const rust = bleRaw.replace(/\r\n/g, '\n')
  const ts = hostRaw.replace(/\r\n/g, '\n')

  /** Rust 结构体里的 `pub` 字段名（snake_case）。 */
  function rustFields(structName: string): string[] {
    const start = rust.indexOf(`pub struct ${structName} {`)
    if (start < 0) throw new Error(`ble.rs 里找不到 ${structName}`)
    const names = [...rust.slice(start, rust.indexOf('\n}', start)).matchAll(/^ {4}pub (\w+):/gm)].map(
      match => match[1]
    )
    // 解析出 0 条必须抛：静默的空集合会让下面每条断言都假绿。
    if (names.length === 0) throw new Error(`${structName} 抠到 0 个字段：写法或正则变了`)
    return names
  }

  /** 前端 interface 的键名。 */
  function tsFields(interfaceName: string): string[] {
    const start = ts.indexOf(`interface ${interfaceName} {`)
    if (start < 0) throw new Error(`plugin-host.ts 里找不到 ${interfaceName}`)
    const names = [...ts.slice(start, ts.indexOf('\n}', start)).matchAll(/^ {2}(\w+):/gm)].map(
      match => match[1]
    )
    if (names.length === 0) throw new Error(`${interfaceName} 抠到 0 个字段`)
    return names
  }

  /** snake_case → serde `rename_all = "camelCase"` 的结果。 */
  const toCamel = (name: string) => name.replace(/_([a-z0-9])/g, (_all, ch: string) => ch.toUpperCase())

  /** 每个 Rust 结构体都要对上的前端形状：作者侧接口 + 命令层镜像。 */
  const pairs: Array<[string, string]> = [
    ['BleDevice', 'PluginBleDevice'],
    ['BleAdvertisement', 'PluginBleAdvertisement'],
    ['BleAdvertisement', 'BleAdvertisementJson'],
    ['BleManufacturerData', 'PluginBleManufacturerData'],
    ['BleServiceData', 'PluginBleServiceData'],
  ]

  for (const [structName, interfaceName] of pairs) {
    it(`${structName} 的每个字段在 ${interfaceName} 里都叫同一个名字`, () => {
      expect(tsFields(interfaceName).sort(), `${interfaceName} 与 Rust ${structName} 的键名不一致`).toEqual(
        rustFields(structName).map(toCamel).sort()
      )
    })
  }
})
