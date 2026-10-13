// @vitest-environment happy-dom

/**
 * 按需建窗那条协议本身：`openPanelWindow` 的入参形状，与「先 listen 再取自取暂存」的顺序。
 *
 * 这一层之所以值得单独测：面板窗不再预先存在，新窗的 JS 挂上监听器需要时间，
 * 于是宿主把同一份数据发两条（建窗时暂存 + 后续 open 事件）。**顺序写反**的表现为
 * 「窗口开出来了却停在空壳上」—— 两步之间宿主 emit 的那一次永远收不到，而且不报错。
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { closePanelWindow, openPanelWindow, usePanelWindowOpen } from './panel-window'

const { invokeMock, listenMock, getAllWindowsMock, getCurrentWindowMock, calls } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
  getAllWindowsMock: vi.fn(),
  getCurrentWindowMock: vi.fn(),
  /** 协议动作的时间线：谁先谁后是这里唯一的看点。 */
  calls: [] as string[]
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
vi.mock('@tauri-apps/api/event', () => ({
  listen: (name: string, handler: (event: { payload: unknown }) => void) => {
    calls.push('listen')
    listenMock(name, handler)
    return Promise.resolve(() => {})
  },
  emitTo: vi.fn(),
  default: {}
}))
vi.mock('@tauri-apps/api/window', () => ({
  getAllWindows: getAllWindowsMock,
  getCurrentWindow: getCurrentWindowMock
}))

const SPEC = { label: 'demo', title: 'UFO · 示例', width: 800, height: 600, minWidth: 400, minHeight: 300 }
const isText = (value: unknown): value is string => typeof value === 'string' && value !== ''

beforeEach(() => {
  calls.length = 0
  invokeMock.mockReset()
  invokeMock.mockResolvedValue(true)
  listenMock.mockReset()
  getAllWindowsMock.mockReset()
  getCurrentWindowMock.mockReset()
})

describe('openPanelWindow', () => {
  it('把几何与数据整份交给宿主，本层不再 show 也不 emit', async () => {
    await openPanelWindow(SPEC, 'hello')
    expect(invokeMock).toHaveBeenCalledWith('open_panel_window', {
      request: { ...SPEC, data: 'hello' }
    })
    // 建窗与投递都在宿主那一侧：这里多发一次就多一次「两个写入方」。
    expect(listenMock).not.toHaveBeenCalled()
  })

  it('宿主拒绝时把原因原样抛给调用方（前端有行内反馈的位置）', async () => {
    invokeMock.mockRejectedValue('同时打开的独立窗口已达上限（3 扇），先关掉一个')
    await expect(openPanelWindow(SPEC, 'hello')).rejects.toThrow('已达上限')
  })
})

describe('usePanelWindowOpen', () => {
  it('先挂监听再取自取暂存：顺序反了就会丢掉中间那一次 open', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'take_panel_payload') calls.push('take')
      return null
    })
    await usePanelWindowOpen<string>(isText, () => {})
    expect(calls).toEqual(['listen', 'take'])
  })

  it('新建那一次吃到暂存，之后的 open 事件也照样生效', async () => {
    const seen: string[] = []
    let panelHandler: ((event: { payload: unknown }) => void) | undefined
    listenMock.mockImplementation((_name: string, handler: (event: { payload: unknown }) => void) => {
      panelHandler = handler
    })
    invokeMock.mockImplementation(async (command: string) =>
      command === 'take_panel_payload' ? { requester: 'main', data: 'first' } : true
    )

    await usePanelWindowOpen<string>(isText, data => seen.push(data))
    expect(seen).toEqual(['first'])

    panelHandler?.({ payload: { requester: 'main', data: 'second' } })
    expect(seen).toEqual(['first', 'second'])
  })

  it('两条路上的坏载荷都整份丢掉，不拿半个对象进界面', async () => {
    const seen: string[] = []
    let panelHandler: ((event: { payload: unknown }) => void) | undefined
    listenMock.mockImplementation((_name: string, handler: (event: { payload: unknown }) => void) => {
      panelHandler = handler
    })
    invokeMock.mockResolvedValue({ requester: 'main' })

    await usePanelWindowOpen<string>(isText, data => seen.push(data))
    panelHandler?.({ payload: { requester: 'main', data: '' } })
    panelHandler?.({ payload: { data: 'ok', requester: 1 } })
    expect(seen).toEqual([])
  })
})

describe('closePanelWindow', () => {
  it('按 label 找到那扇窗并关掉（找不到就什么都不做）', async () => {
    const target = { label: 'demo', close: vi.fn() }
    getAllWindowsMock.mockResolvedValue([{ label: 'other', close: vi.fn() }, target])
    await closePanelWindow('demo')
    expect(target.close).toHaveBeenCalledTimes(1)

    getAllWindowsMock.mockResolvedValue([])
    await expect(closePanelWindow('demo')).resolves.toBeUndefined()
  })
})
