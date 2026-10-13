// @vitest-environment happy-dom

/**
 * 脚本编辑器与「整窗编辑」独立窗口之间那条来回同步的链路。
 *
 * 这一层之所以值得单独测：正文有两个可能的写入方（内嵌框与另一扇窗），而草稿只有一份。
 * 「窗口开着时内嵌框必须转只读」和「只认当前这枚令牌的回传」两条一旦失守，表现就是
 * 用户以为在改 A，落盘的却是 B —— 那种错在界面上完全看不出来。
 *
 * 窗口是按需创建的，所以这里只断言「向宿主提了什么样的建窗请求」与「关掉它的动作有没有做」，
 * 真建不建得出来归宿主（`open_panel_window`）与护栏 `panel-windows.spec.ts`。
 */
import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import ScriptEditor from './ScriptEditor.vue'
import { PANEL_WINDOW_CLOSED_EVENT } from '@/lib/panel-window'
import {
  SCRIPT_EDITOR_SYNC_EVENT,
  SCRIPT_EDITOR_WINDOW,
  SCRIPT_EDITOR_WINDOW_LABEL,
  type ScriptEditorSync,
} from '@/lib/script-editor-window'

const { invokeMock, emitToMock, getAllWindowsMock, getCurrentWindowMock, listenMock, handlers, scriptWindow } =
  vi.hoisted(() => ({
    invokeMock: vi.fn(),
    emitToMock: vi.fn(),
    getAllWindowsMock: vi.fn(),
    getCurrentWindowMock: vi.fn(),
    listenMock: vi.fn(),
    /** 组件注册过的事件回调：事件名 → handler，测试用它模拟窗口那一侧说话。 */
    handlers: {} as Record<string, (event: { payload: unknown }) => void>,
    /** 假的目标窗口：只关心这几枚方法被不被调到。 */
    scriptWindow: { label: 'script-editor', close: vi.fn() },
  }))

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock, default: {} }))
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

/** 取那一次建窗请求的完整入参。 */
function openRequest() {
  const call = invokeMock.mock.calls.find(([name]) => name === 'open_panel_window')
  return call?.[1] as { request: typeof SCRIPT_EDITOR_WINDOW & { data: { token: string; script: string; title: string; caret: object } } }
}

beforeEach(() => {
  invokeMock.mockReset()
  // 默认回答「这次真的新建了一扇」。
  invokeMock.mockResolvedValue(true)
  emitToMock.mockReset()
  emitToMock.mockResolvedValue(undefined)
  getAllWindowsMock.mockReset()
  getAllWindowsMock.mockResolvedValue([scriptWindow])
  getCurrentWindowMock.mockReset()
  getCurrentWindowMock.mockReturnValue({ label: 'main' })
  listenMock.mockReset()
  listenMock.mockImplementation((name: string, handler: (event: { payload: unknown }) => void) => {
    handlers[name] = handler
    return () => {}
  })
  for (const key of Object.keys(handlers)) delete handlers[key]
  scriptWindow.close.mockReset()
})

describe('ScriptEditor（内嵌框 ↔ 独立窗口）', () => {
  it('点「扩大」把当前草稿交给宿主建窗，并把内嵌框转只读', async () => {
    const wrapper = mountEditor()
    expect(wrapper.get('.jse__input').attributes('readonly')).toBeUndefined()

    await wrapper.get('.jse__expand').trigger('click')
    await flushPromises()

    const request = openRequest()?.request
    expect(request, '开窗要走宿主的按需建窗命令').toBeDefined()
    // 身份与几何取自这扇窗自己的契约：主窗不在这儿另写一套尺寸。
    expect(request).toMatchObject(SCRIPT_EDITOR_WINDOW)
    expect(request?.data.script).toBe('return 1')
    expect(request?.data.title).toContain('api.example.com')
    // 发起方身份不由前端写：宿主按调用方窗口取，前端谎报也没用。
    expect(request).not.toHaveProperty('requester')
    // 光标交接的四项读数都要带上（具体位置由 happy-dom 决定，这里只锁「带全了」）。
    expect(Object.keys(request?.data.caret ?? {}).sort()).toEqual([
      'scrollLeft',
      'scrollTop',
      'selectionEnd',
      'selectionStart',
    ])
    // 读数常驻在编辑框下面：不能只把只读做成「点不动」，用户得知道为什么。
    expect(wrapper.get('.jse__editing').text()).toContain('独立窗口')
    expect(wrapper.get('.jse__input').attributes('readonly')).toBeDefined()
  })

  it('只接受当前这一次编辑的回传，过期令牌整条丢掉', async () => {
    const wrapper = mountEditor()
    await wrapper.get('.jse__expand').trigger('click')
    await flushPromises()
    const token = openRequest().request.data.token

    const sync = (patch: Partial<ScriptEditorSync>) =>
      handlers[SCRIPT_EDITOR_SYNC_EVENT]({ payload: { token: '', script: '', ...patch } })

    sync({ token, script: 'return 2' })
    expect(wrapper.emitted('update:modelValue')?.at(-1)).toEqual(['return 2'])

    // 换了一份草稿（令牌作废）之后迟到的正文不许写进现在这一份。
    sync({ token: `${token}-stale`, script: 'return 999' })
    expect(wrapper.emitted('update:modelValue')?.at(-1)).toEqual(['return 2'])
  })

  it('窗口被销毁后，内嵌框重新可写', async () => {
    const wrapper = mountEditor()
    await wrapper.get('.jse__expand').trigger('click')
    await flushPromises()
    expect(wrapper.get('.jse__input').attributes('readonly')).toBeDefined()

    handlers[PANEL_WINDOW_CLOSED_EVENT]({ payload: { window: SCRIPT_EDITOR_WINDOW_LABEL } })
    await flushPromises()
    expect(wrapper.find('.jse__editing').exists()).toBe(false)
    expect(wrapper.get('.jse__input').attributes('readonly')).toBeUndefined()

    // 别的窗口发的销毁回报不许把这里解锁（按 label 过滤）。
    await wrapper.get('.jse__expand').trigger('click')
    await flushPromises()
    handlers[PANEL_WINDOW_CLOSED_EVENT]({ payload: { window: 'status-log' } })
    await flushPromises()
    expect(wrapper.get('.jse__input').attributes('readonly')).toBeDefined()
  })

  it('换草稿（scope 一变）就关掉那扇窗，不让上一份正文写进这一份', async () => {
    const wrapper = mountEditor('ep-1')
    await wrapper.get('.jse__expand').trigger('click')
    await flushPromises()
    const stale = openRequest().request.data.token

    await wrapper.setProps({ scope: 'ep-2' })
    await flushPromises()
    expect(scriptWindow.close, '这一页换了草稿，窗口留着只会写错对象').toHaveBeenCalledTimes(1)
    expect(wrapper.get('.jse__input').attributes('readonly')).toBeUndefined()

    handlers[SCRIPT_EDITOR_SYNC_EVENT]({ payload: { token: stale, script: 'return 77' } })
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()
  })

  it('建窗失败把原因写在行内，而不是安静地什么都不发生', async () => {
    invokeMock.mockImplementation((name: string) =>
      name === 'open_panel_window' ? Promise.reject('同时打开的独立窗口已达上限（3 扇），先关掉一个') : Promise.resolve(true)
    )
    const wrapper = mountEditor()
    await wrapper.get('.jse__expand').trigger('click')
    await flushPromises()
    expect(wrapper.get('.jse__editing').text()).toContain('已达上限')
    expect(wrapper.get('.jse__input').attributes('readonly'), '开不了窗就不该把内嵌框锁死').toBeUndefined()
  })
})
