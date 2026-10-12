// @vitest-environment happy-dom

import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, nextTick } from 'vue'
import TooltipWindow from './TooltipWindow.vue'
import { TOOLTIP_HOVER_EVENT, plugBlockHeight, type TooltipHoverPayload } from '@/lib/tooltip'
import { pluginExtensions, type PluginExtensionSlot } from '@/lib/plugin-loader'
import { clearPluginTooltipSizes, setPluginTooltipSize } from '@/lib/plugin-tooltip-size'
import { useFloatingModulesStore } from '@/stores/floatingModules'

/** 本用例的 pinia：`mount` 必须显式装上，否则组件里的 `useXxxStore()` 会拿到另一份实例。 */
let pinia: Pinia

/** 由测试修改的信息卡就绪状态；mock 里会换成一个 Vue ref 以便触发 computed。 */
const testState = vi.hoisted(() => ({
  hasContent: null as null | { value: boolean },
}))

/** Tauri API mock：组件测试只验证窗口调用顺序，不碰真实 IPC。 */
const tauri = vi.hoisted(() => {
  class MockPhysicalPosition {
    constructor(
      public x: number,
      public y: number
    ) {}
  }

  class MockPhysicalSize {
    constructor(
      public width: number,
      public height: number
    ) {}
  }

  const listeners = new Map<string, (event: { payload: unknown }) => void>()
  const currentWindow = {
    outerSize: vi.fn(async () => new MockPhysicalSize(300, 260)),
    setSize: vi.fn(async () => undefined),
    setPosition: vi.fn(async () => undefined),
    show: vi.fn(async () => undefined),
    hide: vi.fn(async () => undefined),
  }

  return {
    listeners,
    currentWindow,
    monitorFromPoint: vi.fn(),
    listen: vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
      listeners.set(event, handler)
      return () => listeners.delete(event)
    }),
    emitTo: vi.fn(async () => undefined),
    invoke: vi.fn(async () => undefined),
    PhysicalPosition: MockPhysicalPosition,
    PhysicalSize: MockPhysicalSize,
  }
})

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => tauri.currentWindow,
  monitorFromPoint: tauri.monitorFromPoint,
  PhysicalPosition: tauri.PhysicalPosition,
  PhysicalSize: tauri.PhysicalSize,
}))

vi.mock('@tauri-apps/api/event', () => ({
  emitTo: tauri.emitTo,
  listen: tauri.listen,
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: tauri.invoke,
}))

vi.mock('@/modules', async () => {
  const { computed, defineComponent, h, ref } = await import('vue')
  const hasContent = ref(true)
  testState.hasContent = hasContent
  const TooltipStub = defineComponent({
    name: 'TooltipStub',
    props: { target: { type: Object, required: true } },
    setup(props) {
      return () => h('div', { class: 'tooltip-stub' }, String((props.target as { key?: string }).key ?? ''))
    },
  })

  return {
    // 消费点读的是 computed（合并注册表要能随装载重算），mock 保持同一形状。
    FLOATING_MODULE_DEFINITIONS: computed(() => [
      {
        id: 'test-module',
        label: 'Test module',
        description: '',
        version: 1,
        defaultEnabled: false,
        supportedLayouts: ['square'],
        component: TooltipStub,
        tooltip: {
          component: TooltipStub,
          hasContent: (target: unknown) => hasContent.value && Boolean(target),
        },
      },
    ]),
  }
})

enableAutoUnmount(afterEach)

const workAreaMonitor = {
  workArea: {
    position: { x: 0, y: 0 },
    size: { width: 1920, height: 1040 },
  },
}

/** 造一个可由测试手动完成的 Promise。 */
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(done => {
    resolve = done
  })
  return { promise, resolve }
}

/** 造一条 hover 事件 payload；隐藏事件按契约不带目标。 */
function hoverPayload(sequence: number, visible: boolean, key = 'session-a'): TooltipHoverPayload {
  return {
    sequence,
    visible,
    target: visible ? { moduleId: 'test-module', key } : null,
    anchor: { x: 400, y: 400, width: 45, height: 45 },
    docked: { left: false, top: false, right: false, bottom: false },
  }
}

/** 把测试 payload 送进组件注册的 Tauri 事件处理器。 */
function emitHover(payload: TooltipHoverPayload) {
  const handler = tauri.listeners.get(TOOLTIP_HOVER_EVENT)
  expect(handler).toBeDefined()
  handler?.({ payload })
}

/** 挂载提示窗并等 onMounted 的默认尺寸与事件监听就绪。 */
async function mountTooltip() {
  const wrapper = mount(TooltipWindow, { global: { plugins: [pinia] } })
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  vi.clearAllMocks()
  // 提示窗现在要读「这个包启用没有」，那是外壳 store 的数据，所以测试得有 pinia。
  pinia = createPinia()
  setActivePinia(pinia)
  pluginExtensions.value = []
  clearPluginTooltipSizes()
  tauri.listeners.clear()
  testState.hasContent!.value = true

  tauri.monitorFromPoint.mockResolvedValue(workAreaMonitor)
  tauri.currentWindow.outerSize.mockResolvedValue(new tauri.PhysicalSize(300, 260))
  tauri.currentWindow.setSize.mockResolvedValue(undefined)
  tauri.currentWindow.setPosition.mockResolvedValue(undefined)
  tauri.currentWindow.show.mockResolvedValue(undefined)
  tauri.currentWindow.hide.mockResolvedValue(undefined)
  tauri.emitTo.mockResolvedValue(undefined)
  tauri.invoke.mockResolvedValue(undefined)
})

/** 插入块的替身组件：把拿到的 props 直接写进文本，好让断言看得见。 */
const PlugStub = defineComponent({
  name: 'PlugStub',
  props: { target: { type: Object, required: true }, maxRows: { type: Number, required: true } },
  setup: props => () => h('p', { class: 'plug-stub' }, `rows:${props.maxRows}`),
})

describe('信息卡的插入块（组件包往别人的卡尾部追加）', () => {
  /** 造一块挂在 test-module 卡上的挂载位。 */
  function plug(patch: Partial<PluginExtensionSlot> = {}): PluginExtensionSlot {
    return {
      hostId: 'test-module',
      pluginId: 'balance-board',
      label: '余额看板',
      component: PlugStub,
      maxRows: 2,
      ...patch,
    }
  }

  it('挂上来源标注，并把清单里的行数交给插件组件', async () => {
    pluginExtensions.value = [plug()]
    const wrapper = await mountTooltip()
    emitHover(hoverPayload(1, true))
    await flushPromises()
    await nextTick()

    const plugs = wrapper.find('.tooltip-plugs')
    expect(plugs.exists()).toBe(true)
    expect(plugs.text()).toContain('插件 · 余额看板')
    expect(wrapper.find('.plug-stub').text()).toBe('rows:2')
  })

  it('提示窗按插入块的预算长高', async () => {
    pluginExtensions.value = [plug({ maxRows: 2 })]
    const wrapper = await mountTooltip()
    emitHover(hoverPayload(1, true))
    await flushPromises()
    await nextTick()

    // mock 的模块没声明 size ⇒ 基准是默认 300×260，加上一块的预算。
    expect(tauri.currentWindow.setSize).toHaveBeenCalledWith(
      expect.objectContaining({ width: 300, height: 260 + plugBlockHeight(2) })
    )
    expect(wrapper.find('.tooltip-plugs').exists()).toBe(true)
  })

  it('挂在别人卡上的块不会串到这里', async () => {
    pluginExtensions.value = [plug({ hostId: 'other-module' })]
    const wrapper = await mountTooltip()
    emitHover(hoverPayload(1, true))
    await flushPromises()
    await nextTick()
    expect(wrapper.find('.tooltip-plugs').exists()).toBe(false)
  })

  it('包被停用就整块不出现（启用状态读的是外壳配置）', async () => {
    pluginExtensions.value = [plug()]
    useFloatingModulesStore().applyConfig({
      schema_version: 1,
      revision: 1,
      modules: [{ id: 'balance-board', enabled: false, order: 0 }],
      module_settings: {},
    })
    const wrapper = await mountTooltip()
    emitHover(hoverPayload(1, true))
    await flushPromises()
    await nextTick()
    expect(wrapper.find('.tooltip-plugs').exists()).toBe(false)
  })

  it('块自己说没内容就不留一条空虚线', async () => {
    pluginExtensions.value = [plug({ hasContent: () => false })]
    const wrapper = await mountTooltip()
    emitHover(hoverPayload(1, true))
    await flushPromises()
    await nextTick()
    expect(wrapper.find('.tooltip-plugs').exists()).toBe(false)
  })
})

describe('TooltipWindow transitions', () => {
  it('aborts a pending show when hide arrives before the monitor lookup resolves', async () => {
    const monitor = deferred<typeof workAreaMonitor>()
    tauri.monitorFromPoint.mockImplementationOnce(() => monitor.promise)

    await mountTooltip()
    emitHover(hoverPayload(1, true))
    await flushPromises()

    emitHover(hoverPayload(2, false))
    await nextTick()
    monitor.resolve(workAreaMonitor)
    await flushPromises()

    // 旧 show 已经失去请求代次，不能在 hide 后又把空内容窗口唤醒。
    expect(tauri.currentWindow.show).not.toHaveBeenCalled()
    expect(tauri.currentWindow.hide).toHaveBeenCalledTimes(1)
    expect(tauri.invoke).toHaveBeenCalledWith('set_tooltip_interactive', { interactive: false })
  })

  it('drops show A when show B arrives before A commits', async () => {
    const monitor = deferred<typeof workAreaMonitor>()
    tauri.monitorFromPoint.mockImplementationOnce(() => monitor.promise)

    await mountTooltip()
    emitHover(hoverPayload(1, true, 'a'))
    await flushPromises()

    emitHover(hoverPayload(2, true, 'b'))
    await nextTick()
    monitor.resolve(workAreaMonitor)
    await flushPromises()

    // A 停在 monitorFromPoint，恢复后应直接放弃；只有 B 能完成 setPosition/show。
    expect(tauri.currentWindow.setPosition).toHaveBeenCalledTimes(1)
    expect(tauri.currentWindow.show).toHaveBeenCalledTimes(1)
    expect(tauri.invoke).toHaveBeenCalledWith('set_tooltip_interactive', { interactive: true })
  })

  it('commits a show only after positioning and interaction are ready', async () => {
    await mountTooltip()

    emitHover(hoverPayload(1, true))
    await flushPromises()

    // show 必须排在 set_tooltip_interactive(true) 与 setPosition 之后，避免在旧位置或点穿状态下闪现。
    const interactiveOrder = tauri.invoke.mock.invocationCallOrder[0]!
    const positionOrder = tauri.currentWindow.setPosition.mock.invocationCallOrder[0]!
    const showOrder = tauri.currentWindow.show.mock.invocationCallOrder[0]!
    expect(showOrder).toBeGreaterThan(interactiveOrder)
    expect(showOrder).toBeGreaterThan(positionOrder)
  })

  it('ignores late hover events whose sequence is not newer', async () => {
    await mountTooltip()

    emitHover(hoverPayload(2, true))
    await flushPromises()
    emitHover(hoverPayload(3, false))
    await flushPromises()

    // sequence=2 的重复 show 是迟到事件，不能在 newer hide 之后重新显示。
    emitHover(hoverPayload(2, true))
    await flushPromises()
    expect(tauri.currentWindow.show).toHaveBeenCalledTimes(1)
    expect(tauri.currentWindow.hide).toHaveBeenCalledTimes(1)

    emitHover(hoverPayload(4, true))
    await flushPromises()

    // sequence=3 的重复 hide 同样过期，不能把 newer show 关掉。
    emitHover(hoverPayload(3, false))
    await flushPromises()
    expect(tauri.currentWindow.show).toHaveBeenCalledTimes(2)
    expect(tauri.currentWindow.hide).toHaveBeenCalledTimes(1)
  })

  it('keeps processing the transition queue after repeated show/hide changes', async () => {
    await mountTooltip()

    emitHover(hoverPayload(1, true))
    await flushPromises()
    emitHover(hoverPayload(2, false))
    await flushPromises()
    emitHover(hoverPayload(3, true))
    await flushPromises()
    emitHover(hoverPayload(4, false))
    await flushPromises()

    expect(tauri.currentWindow.show).toHaveBeenCalledTimes(2)
    expect(tauri.currentWindow.hide).toHaveBeenCalledTimes(2)
  })

  it('presents when content becomes ready after a visible hover event', async () => {
    await mountTooltip()

    testState.hasContent!.value = false
    emitHover(hoverPayload(1, true))
    await flushPromises()
    expect(tauri.currentWindow.show).not.toHaveBeenCalled()

    // 模块数据晚到且没有新的 hover 事件时，displayKey 仍要触发一次 present。
    testState.hasContent!.value = true
    await flushPromises()
    expect(tauri.currentWindow.show).toHaveBeenCalledTimes(1)
  })
})

describe('组件包运行时改自己信息卡的尺寸（host.tooltip.setSize）', () => {
  /** 走到「已定位已显示」那一步之后清空窗口调用记录，只观察尺寸变更引发的重排。 */
  async function presentThenRecord() {
    await mountTooltip()
    emitHover(hoverPayload(1, true))
    await flushPromises()
    tauri.currentWindow.setSize.mockClear()
    tauri.currentWindow.setPosition.mockClear()
    tauri.currentWindow.show.mockClear()
  }

  it('运行时申请的那一份优先于清单初值', async () => {
    await presentThenRecord()
    // mock 的模块没声明 size ⇒ 基准是默认 300×260；申请过就该换成 460×620。
    setPluginTooltipSize('test-module', { width: 400, height: 600 })
    await flushPromises()

    expect(tauri.currentWindow.setSize).toHaveBeenCalledWith(expect.objectContaining({ width: 400, height: 600 }))
  })

  it('正显示时改尺寸要重新定位并再显示一次（只换尺寸会留在旧位置上）', async () => {
    await presentThenRecord()
    setPluginTooltipSize('test-module', { width: 400, height: 600 })
    await flushPromises()

    expect(tauri.currentWindow.setPosition).toHaveBeenCalledTimes(1)
    expect(tauri.currentWindow.show).toHaveBeenCalledTimes(1)
  })

  it('别的包改尺寸不动正显示的这张卡', async () => {
    await presentThenRecord()
    setPluginTooltipSize('another-plugin', { width: 500, height: 800 })
    await flushPromises()

    expect(tauri.currentWindow.setSize).not.toHaveBeenCalled()
    expect(tauri.currentWindow.show).not.toHaveBeenCalled()
  })
})
