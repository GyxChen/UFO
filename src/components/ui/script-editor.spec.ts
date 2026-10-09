// @vitest-environment happy-dom

/**
 * 脚本编辑器与「整窗编辑」独立窗口之间那条来回同步的链路。
 *
 * 这一层之所以值得单独测：正文有两个可能的写入方（内嵌框与另一扇窗），而草稿只有一份。
 * 「窗口开着时内嵌框必须转只读」和「只认当前这枚令牌的回传」两条一旦失守，表现就是
 * 用户以为在改 A，落盘的却是 B —— 那种错在界面上完全看不出来。
 */
import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import ScriptEditor from './ScriptEditor.vue'
import { PANEL_WINDOW_CLOSED_EVENT, PANEL_WINDOW_OPEN_EVENT } from '@/lib/panel-window'
import { SCRIPT_EDITOR_SYNC_EVENT, SCRIPT_EDITOR_WINDOW_LABEL, type ScriptEditorSync } from '@/lib/script-editor-window'

const { emitToMock, getAllWindowsMock, getCurrentWindowMock, listenMock, handlers, scriptWindow } = vi.hoisted(() => ({
  emitToMock: vi.fn(),
  getAllWindowsMock: vi.fn(),
  getCurrentWindowMock: vi.fn(),
  listenMock: vi.fn(),
  /** 组件注册过的事件回调：事件名 → handler，测试用它模拟窗口那一侧说话。 */
  handlers: {} as Record<string, (event: { payload: unknown }) => void>,
  /** 假的目标窗口：只关心这几枚方法被不被调到。 */
  scriptWindow: { label: 'script-editor', show: vi.fn(), unminimize: vi.fn(), setFocus: vi.fn(), hide: vi.fn() },
}))

vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock, emitTo: emitToMock, default: {} }))
vi.mock('@tauri-apps/api/window', () => ({ getAllWindows: getAllWindowsMock, getCurrentWindow: getCurrentWindowMock }))

/** 挂上编辑器；`scope` 是调用方给这份草稿的身份。 */
function mountEditor(scope = 'ep-1') {
  return mount(ScriptEditor, {
    props: {
      modelValue: 'return 1',
      scope,
      title: '编辑脚本 · https://api.example.com',
      placeholder: 'return []',
      ariaLabel: '流程脚本源码',
    },
    attachTo: document.body,
  })
}

/** 从开窗那次 `emitTo` 里取回载荷（`[label, event, payload]`）。 */
function openPayload() {
  const call = emitToMock.mock.calls.find(item => item[1] === PANEL_WINDOW_OPEN_EVENT)
  return call?.[2] as { requester: string; data: { token: string; script: string; title: string; caret: object } }
}

beforeEach(() => {
  emitToMock.mockReset()
  emitToMock.mockResolvedValue(undefined)
  getAllWindowsMock.mockReset()
  getAllWindowsMock.mockResolvedValue([scriptWindow])
  getCurrentWindowMock.mockReset()
  getCurrentWindowMock.mockReturnValue({ label: 'main', hide: scriptWindow.hide })
  listenMock.mockReset()
  listenMock.mockImplementation((name: string, handler: (event: { payload: unknown }) => void) => {
    handlers[name] = handler
    return () => {}
  })
  for (const key of Object.keys(handlers)) delete handlers[key]
  scriptWindow.show.mockReset()
  scriptWindow.setFocus.mockReset()
  scriptWindow.hide.mockReset()
})

describe('ScriptEditor（内嵌框 ↔ 独立窗口）', () => {
  it('点「扩大」把当前草稿开进独立窗口，并把内嵌框转只读', async () => {
    const wrapper = mountEditor()
    expect(wrapper.get('.jse__input').attributes('readonly')).toBeUndefined()

    await wrapper.get('.jse__expand').trigger('click')
    await flushPromises()

    const payload = openPayload()
    expect(payload?.requester, '回报对象必须是发起方窗口').toBe('main')
    expect(payload?.data.script).toBe('return 1')
    expect(payload?.data.title).toContain('api.example.com')
    // 光标交接的四项读数都要带上（具体位置由 happy-dom 决定，这里只锁「带全了」）。
    expect(Object.keys(payload?.data.caret ?? {}).sort()).toEqual([
      'scrollLeft',
      'scrollTop',
      'selectionEnd',
      'selectionStart',
    ])
    expect(scriptWindow.show).toHaveBeenCalledTimes(1)
    expect(scriptWindow.setFocus).toHaveBeenCalledTimes(1)
    // 读数常驻在编辑框下面：不能只把只读做成「点不动」，用户得知道为什么。
    expect(wrapper.get('.jse__editing').text()).toContain('独立窗口')
    expect(wrapper.get('.jse__input').attributes('readonly')).toBeDefined()
  })

  it('只接受当前这一次编辑的回传，过期令牌整条丢掉', async () => {
    const wrapper = mountEditor()
    await wrapper.get('.jse__expand').trigger('click')
    await flushPromises()
    const token = openPayload().data.token

    const sync = (patch: Partial<ScriptEditorSync>) =>
      handlers[SCRIPT_EDITOR_SYNC_EVENT]({ payload: { token: '', script: '', ...patch } })

    sync({ token, script: 'return 2' })
    expect(wrapper.emitted('update:modelValue')?.at(-1)).toEqual(['return 2'])

    // 换了一份草稿（令牌作废）之后迟到的正文不许写进现在这一份。
    sync({ token: `${token}-stale`, script: 'return 999' })
    expect(wrapper.emitted('update:modelValue')?.at(-1)).toEqual(['return 2'])
  })

  it('窗口回报收起后，内嵌框重新可写', async () => {
    const wrapper = mountEditor()
    await wrapper.get('.jse__expand').trigger('click')
    await flushPromises()
    expect(wrapper.get('.jse__input').attributes('readonly')).toBeDefined()

    handlers[PANEL_WINDOW_CLOSED_EVENT]({ payload: { window: SCRIPT_EDITOR_WINDOW_LABEL } })
    await flushPromises()
    expect(wrapper.find('.jse__editing').exists()).toBe(false)
    expect(wrapper.get('.jse__input').attributes('readonly')).toBeUndefined()

    // 别的窗口发的收起回报不许把这里解锁（按 label 过滤）。
    await wrapper.get('.jse__expand').trigger('click')
    await flushPromises()
    handlers[PANEL_WINDOW_CLOSED_EVENT]({ payload: { window: 'status-log' } })
    await flushPromises()
    expect(wrapper.get('.jse__input').attributes('readonly')).toBeDefined()
  })

  it('换草稿（scope 一变）就把窗口收起，不让上一份正文写进这一份', async () => {
    const wrapper = mountEditor('ep-1')
    await wrapper.get('.jse__expand').trigger('click')
    await flushPromises()
    const stale = openPayload().data.token

    await wrapper.setProps({ scope: 'ep-2' })
    await flushPromises()
    expect(scriptWindow.hide, '这一页换了草稿，窗口留着只会写错对象').toHaveBeenCalledTimes(1)
    expect(wrapper.get('.jse__input').attributes('readonly')).toBeUndefined()

    handlers[SCRIPT_EDITOR_SYNC_EVENT]({ payload: { token: stale, script: 'return 77' } })
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()
  })

  it('窗口没在配置里声明时把话说清，而不是安静地什么都不发生', async () => {
    getAllWindowsMock.mockResolvedValue([])
    const wrapper = mountEditor()
    await wrapper.get('.jse__expand').trigger('click')
    await flushPromises()
    expect(wrapper.get('.jse__editing').text()).toContain('打不开')
    expect(wrapper.get('.jse__input').attributes('readonly'), '开不了窗就不该把内嵌框锁死').toBeUndefined()
  })
})
