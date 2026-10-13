// @vitest-environment happy-dom

/**
 * 「转灯日志」独立窗口的来回链路：主窗递来哪条会话 → 正文挂起来 → 收起时把订阅停掉。
 *
 * 这一层之所以值得单独测：窗口是**按需创建、关窗即销毁**的，前端自己那条 `log.stop()`
 * 是「人走了就不再广播」的第一道保险（宿主的 `Destroyed` 补刀只清后端那本 label 账）。
 * 顺序反了或整条漏掉，表现都是「看不见窗口却还在每轮广播」，那种错在界面上没有痕迹。
 *
 * 两条载荷来源都要测：新建那一次是窗口挂载后**自取**暂存（`take_panel_payload`），
 * 之后每次 open 走事件 —— 少接一条就是「窗开了却停在空壳上」。
 */
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import StatusLogWindow from './StatusLogWindow.vue'
import { PANEL_WINDOW_OPEN_EVENT } from '@/lib/panel-window'
import { STATUS_LOG_WINDOW_LABEL } from '../lib/status-log-window'
import type { StatusLogEntry, StatusLogPage } from '../lib/status-log'
import { emptyLogUsage } from '../lib/status-log'

const { invokeMock, listenMock, closeMock, handlers, staged } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
  closeMock: vi.fn(),
  /** 窗口侧注册过的事件回调：事件名 → handler，测试用它扮演主窗那一侧。 */
  handlers: {} as Record<string, (event: { payload: unknown }) => void>,
  /** 建窗时宿主暂存的那一份载荷：按用例改它（null = 没有暂存，只走事件那条路）。 */
  staged: { payload: null as unknown },
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock, emitTo: vi.fn(), default: {} }))
vi.mock('@tauri-apps/api/window', () => ({
  getAllWindows: () => Promise.resolve([]),
  getCurrentWindow: () => ({ label: STATUS_LOG_WINDOW_LABEL, close: closeMock }),
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

/** 主窗那一次 open（窗口已经活着时走事件那一条）。 */
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
  staged.payload = null
  invokeMock.mockReset()
  invokeMock.mockImplementation(async (command: string) => {
    if (command === 'get_status_log') return page([entry(3), entry(2), entry(1)])
    if (command === 'count_status_log') {
      return { total: 3, as_of_ms: 1_790_000_000_000, scanned_bytes: 1024 }
    }
    // 新建那扇窗的自取载荷：默认没有（本文件多数用例走事件那一条）。
    if (command === 'take_panel_payload') return staged.payload
    return undefined
  })
  listenMock.mockReset()
  listenMock.mockImplementation((name: string, handler: (event: { payload: unknown }) => void) => {
    handlers[name] = handler
    return () => {}
  })
  closeMock.mockReset()
  for (const key of Object.keys(handlers)) delete handlers[key]
})

describe('StatusLogWindow', () => {
  it('收到会话才挂正文：两栏、行尾与右栏都是盘上坐标', async () => {
    const wrapper = await mountWindow()
    // 载荷到之前不画任何东西（新建那一瞬画出来也是一块没主人的空壳）。
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

  it('新建那一次靠自取暂存拿到会话：事件还没挂上时也不能停在空壳', async () => {
    staged.payload = { requester: 'main', data: { sessionKey: 'codex:s1' } }
    const wrapper = await mountWindow()
    expect(wrapper.find('.lg-window__body').exists(), '取到暂存载荷就该直接挂正文').toBe(true)
    expect(wrapper.text()).toContain('转灯日志 · codex:s1')
  })

  it('点标题栏的 × 收窗：先停订阅，再销毁自己', async () => {
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
    expect(closeMock).toHaveBeenCalledTimes(1)
    expect(wrapper.find('.lg-window__body').exists(), '正文要跟着卸掉，过滤条件不跨次残留').toBe(false)
  })

  it('形状不对的载荷整份丢掉，不拿半个对象去查日志', async () => {
    // 自取那条同样要过校验：暂存里塞半个对象也不能挂正文。
    staged.payload = { requester: 'main', data: { sessionKey: '' } }
    const wrapper = await mountWindow()
    handlers[PANEL_WINDOW_OPEN_EVENT]?.({ payload: { requester: 'main', data: { sessionKey: '' } } })
    handlers[PANEL_WINDOW_OPEN_EVENT]?.({ payload: { requester: 'main' } })
    await flushPromises()
    expect(wrapper.find('.lg-window__body').exists()).toBe(false)
    expect(invokeMock.mock.calls.some(call => call[0] === 'set_status_log_view')).toBe(false)
  })
})
