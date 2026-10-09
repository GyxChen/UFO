// @vitest-environment happy-dom

/**
 * 「转灯日志」独立窗口的来回链路：主窗递来哪条会话 → 正文挂起来 → 收起时把订阅停掉。
 *
 * 这一层之所以值得单独测：窗口是**隐藏而非销毁**的，收起时如果漏掉 `log.stop()`，
 * 后端的按-label 订阅就一直挂着 —— 用户看不见窗口，monitor 却每轮都在发那批 IPC，
 * 这种「没人看还在广播」的错在界面上完全没有痕迹。
 */
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import StatusLogWindow from './StatusLogWindow.vue'
import { PANEL_WINDOW_CLOSED_EVENT, PANEL_WINDOW_OPEN_EVENT } from '@/lib/panel-window'
import { STATUS_LOG_WINDOW_LABEL } from '../lib/status-log-window'
import type { StatusLogEntry, StatusLogPage } from '../lib/status-log'
import { emptyLogUsage } from '../lib/status-log'

const { invokeMock, listenMock, emitToMock, hideMock, handlers } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
  emitToMock: vi.fn(),
  hideMock: vi.fn(),
  /** 窗口侧注册过的事件回调：事件名 → handler，测试用它扮演主窗那一侧。 */
  handlers: {} as Record<string, (event: { payload: unknown }) => void>,
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock, emitTo: emitToMock, default: {} }))
vi.mock('@tauri-apps/api/window', () => ({
  getAllWindows: () => Promise.resolve([]),
  getCurrentWindow: () => ({ label: STATUS_LOG_WINDOW_LABEL, hide: hideMock }),
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: vi.fn() }))

/** 造一条记录（字段按线协议命名；`id` 是盘上身份）。 */
function entry(seq: number, patch: Partial<StatusLogEntry> = {}): StatusLogEntry {
  return {
    seq,
    id: `2026-09-28.0.${seq}`,
    ts_ms: 1_790_000_000_000 + seq * 1000,
    writer: 'monitor',
    stage: 'kernel_verdict',
    reason: 'duplicate_in_seen_window',
    trace_id: 'c4-1',
    key: 'codex:s1',
    ...patch,
  } as StatusLogEntry
}

function page(entries: StatusLogEntry[]): StatusLogPage {
  return {
    head: entries[0]?.seq ?? 0,
    total: entries.length,
    dropped: 0,
    dir: 'C:\\log',
    usage: emptyLogUsage(),
    next_before: null,
    exhausted: true,
    scanned_bytes: 0,
    entries,
  }
}

/** 主窗那一次 open：把载荷喂给窗口注册的监听器。 */
async function open(sessionKey: string) {
  handlers[PANEL_WINDOW_OPEN_EVENT]?.({ payload: { requester: 'main', data: { sessionKey } } })
  await flushPromises()
}

/**
 * 挂一扇窗口。
 *
 * ⚠️ pinia 必须显式注入 `mount`（写法照 `StatusLogFeed.spec.ts`）：仓库的 setup 里那枚
 * testing pinia 是**整个文件共用一份 store 状态**的，不注入的话上一条测试留在 `started = true`，
 * 这一条的 `start()` 就整段跳过，测出来的 attach 顺序是假的。
 */
async function mountWindow() {
  const pinia = createPinia()
  setActivePinia(pinia)
  const wrapper = mount(StatusLogWindow, { attachTo: document.body, global: { plugins: [pinia] } })
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  invokeMock.mockImplementation(async (command: string) => {
    if (command === 'get_status_log') return page([entry(3), entry(2), entry(1)])
    if (command === 'count_status_log') {
      return { total: 3, as_of_ms: 1_790_000_000_000, scanned_bytes: 1024 }
    }
    return undefined
  })
  listenMock.mockReset()
  listenMock.mockImplementation((name: string, handler: (event: { payload: unknown }) => void) => {
    handlers[name] = handler
    return () => {}
  })
  emitToMock.mockReset()
  emitToMock.mockResolvedValue(undefined)
  hideMock.mockReset()
  for (const key of Object.keys(handlers)) delete handlers[key]
})

describe('StatusLogWindow', () => {
  it('收到会话才挂正文：两栏、行尾与右栏都是盘上坐标', async () => {
    const wrapper = await mountWindow()
    // open 之前不画任何东西（窗口这时还藏着，画出来也是一块没主人的空壳）。
    expect(wrapper.find('.lg-window__body').exists()).toBe(false)

    await open('codex:s1')
    expect(wrapper.find('.lg-window__body').exists()).toBe(true)
    expect(wrapper.findAll('.lg-split > *')).toHaveLength(2, '左右两栏')
    expect(wrapper.text()).toContain('c4-1')
    expect(wrapper.text()).toMatch(/\d+@\d+/, '行身份是「文件序号@字节偏移」')
    expect(wrapper.text()).toContain('盘上的字节坐标')
    expect(wrapper.text()).toContain('转灯日志 · codex:s1')
    // 标题栏与主窗同版式：左端 UFO 品牌位（图标 + 名字）必须在，右端是 × 而不是文字钮。
    expect(wrapper.find('.panel-bar__logo').exists()).toBe(true)
    expect(wrapper.get('.panel-bar__name').text()).toBe('UFO')
    // 订阅按窗口记账：开窗必须先 attach。
    expect(invokeMock.mock.calls.find(call => call[0] === 'set_status_log_view')?.[1]).toEqual({ attached: true })
  })

  it('点标题栏的 × 收窗：先停订阅，再隐藏自己并回报主窗', async () => {
    const wrapper = await mountWindow()
    await open('codex:s1')

    const close = wrapper.find('.panel-bar__close')
    expect(close.exists(), '关闭钮必须与主窗同一枚图标、同一个类名').toBe(true)
    await close.trigger('click')
    await flushPromises()

    const attach = invokeMock.mock.calls
      .filter(call => call[0] === 'set_status_log_view')
      .map(call => (call[1] as { attached: boolean }).attached)
    // 开窗先 attach、收起再 detach：顺序反了或漏一条，表现都是「看不见窗口却还在每轮广播」。
    expect(attach).toEqual([true, false])
    expect(hideMock).toHaveBeenCalledTimes(1)
    expect(wrapper.find('.lg-window__body').exists(), '正文要跟着卸掉，过滤条件不跨次残留').toBe(false)
    const closed = emitToMock.mock.calls.find(call => call[1] === PANEL_WINDOW_CLOSED_EVENT)
    expect(closed?.[0]).toBe('main')
    expect(closed?.[2]).toEqual({ window: STATUS_LOG_WINDOW_LABEL })
  })

  it('形状不对的 open 载荷整份丢掉，不拿半个对象去查日志', async () => {
    const wrapper = await mountWindow()
    handlers[PANEL_WINDOW_OPEN_EVENT]?.({ payload: { requester: 'main', data: { sessionKey: '' } } })
    handlers[PANEL_WINDOW_OPEN_EVENT]?.({ payload: { requester: 'main' } })
    await flushPromises()
    expect(wrapper.find('.lg-window__body').exists()).toBe(false)
    expect(invokeMock.mock.calls.some(call => call[0] === 'set_status_log_view')).toBe(false)
  })
})
