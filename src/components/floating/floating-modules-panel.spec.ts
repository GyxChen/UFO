// @vitest-environment happy-dom

/**
 * 「悬浮窗设置」页里格子大小的回归测试。
 *
 * 这一页是**外壳自己**的设置页（不属于任何组件），锁三件事：
 * ① 读的是配置里的 `cell_px`，缺字段时回落默认值；
 * ② 拖动过程中只动预览，松手才落盘 —— 否则滑块每动一像素都要写一次盘 + 让真窗口重新吸附；
 * ③ 落盘失败时界面退回后端已知值，不停在一个没存下来的数上。
 *
 * ⚠️ `setValue` 会连发 input + change（等于「拖到底并松手」）；只想模拟拖动就手动改
 * `element.value` 再 `trigger('input')`。
 */
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import FloatingModulesPanel from './FloatingModulesPanel.vue'
import { FLOATING_BASE_CELL_PX, MAX_CELL_PX, MIN_CELL_PX } from '@/lib/floating-layout'
import type { FloatingModuleConfig, FloatingRuntimeSnapshot } from '@/lib/floating-modules'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import type { VueWrapper } from '@vue/test-utils'

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))

/** 一份后端配置；不传 `cellPx` 表示旧配置文件（没有这个键）。 */
function backendConfig(cellPx?: number): FloatingModuleConfig {
  return {
    schema_version: 1,
    revision: 6,
    modules: [{ id: 'stub-module', enabled: true, order: 0 }],
    max_cells_by_layout: { square: 6, corner: 6, horizontal: 8, vertical: 8 },
    ...(cellPx === undefined ? {} : { cell_px: cellPx }),
    module_settings: { 'other-module': { untouched: true } },
  }
}

/** 悬浮窗广播来的运行时快照：4 格，方形与四角都是 2×2。 */
const runtime: FloatingRuntimeSnapshot = {
  layout: 'square',
  byLayout: {
    square: { demands: { 'stub-module': 4 }, windowCells: 4 },
    corner: { demands: { 'stub-module': 4 }, windowCells: 4 },
    horizontal: { demands: { 'stub-module': 4 }, windowCells: 4 },
    vertical: { demands: { 'stub-module': 4 }, windowCells: 4 },
  },
}

/** 记下最后一次落盘的配置，供断言「写回了哪个字段」。 */
let saved: FloatingModuleConfig | null = null

beforeEach(() => {
  saved = null
  invokeMock.mockReset()
  invokeMock.mockImplementation(async (command: string, args?: { config?: FloatingModuleConfig }) => {
    if (command === 'set_floating_module_config' && args?.config) {
      saved = args.config
      return { ...args.config, revision: args.config.revision + 1 }
    }
    throw new Error(`unexpected command: ${command}`)
  })
})

/** 灌一份配置 + 运行时快照，然后挂载这一页（pinia 显式注入，保证两边是同一份 store）。 */
function mountWith(config: FloatingModuleConfig) {
  const pinia = createPinia()
  setActivePinia(pinia)
  const store = useFloatingModulesStore()
  store.applyConfig(config)
  store.applyRuntime(runtime)
  return mount(FloatingModulesPanel, { global: { plugins: [pinia] } })
}

/** 滑块元素（原生 range 输入）。 */
function slider(wrapper: VueWrapper) {
  return wrapper.get<HTMLInputElement>('.base-slider__input')
}

describe('FloatingModulesPanel · 格子大小', () => {
  it('renders the configured cell size with the allowed range', () => {
    const wrapper = mountWith(backendConfig(64))
    const input = slider(wrapper)

    expect(input.element.value).toBe('64')
    expect(input.attributes('min')).toBe(String(MIN_CELL_PX))
    expect(input.attributes('max')).toBe(String(MAX_CELL_PX))
    expect(wrapper.get('.module-catalog__cell-value').text()).toBe('64px')
  })

  it('falls back to the default cell size when the config has no cell_px', () => {
    const wrapper = mountWith(backendConfig())
    expect(slider(wrapper).element.value).toBe(String(FLOATING_BASE_CELL_PX))
  })

  it('scales the preview while dragging without saving anything', async () => {
    const wrapper = mountWith(backendConfig(FLOATING_BASE_CELL_PX))
    const input = slider(wrapper)
    // 只发 input（拖动过程中），不发 change（松手）。
    input.element.value = '64'
    await input.trigger('input')
    await flushPromises()

    // 预览立刻按新尺寸重算（4 格 → 四角边长 2×2 格）。
    expect(wrapper.get('.shape-preview__size').text()).toContain('未贴边 64×64')
    expect(wrapper.get('.module-catalog__cell-value').text()).toBe('64px')
    // 拖动过程中不落盘：真窗口与配置文件都不该被打扰。
    expect(saved).toBeNull()
  })

  it('saves the cell size on release and leaves other fields untouched', async () => {
    const wrapper = mountWith(backendConfig(FLOATING_BASE_CELL_PX))
    // setValue 连发 input + change，等于「拖到 64 松手」。
    await slider(wrapper).setValue('64')
    await flushPromises()

    expect(saved?.cell_px).toBe(64)
    expect(saved?.max_cells_by_layout).toEqual({ square: 6, corner: 6, horizontal: 8, vertical: 8 })
    expect(saved?.module_settings['other-module']).toEqual({ untouched: true })
    expect(saved?.modules).toEqual([{ id: 'stub-module', enabled: true, order: 0 }])
  })

  it('resets to the default cell size from the button', async () => {
    const wrapper = mountWith(backendConfig(80))
    await wrapper.get('.module-catalog__cell-size button').trigger('click')
    await flushPromises()

    expect(saved?.cell_px).toBe(FLOATING_BASE_CELL_PX)
    expect(wrapper.get('.module-catalog__cell-value').text()).toBe(`${FLOATING_BASE_CELL_PX}px`)
  })

  it('keeps the cell-size control in its own card below the module list', () => {
    const wrapper = mountWith(backendConfig(FLOATING_BASE_CELL_PX))
    const cards = wrapper.get('.module-catalog__column').findAll('.ufo-card')

    // 左列两张卡：组件列表在上、格子大小在下（尺寸基准不属于任何一个组件）。
    expect(cards.map(card => card.classes().find(name => name.startsWith('module-catalog__')))).toEqual([
      'module-catalog__list',
      'module-catalog__cell-card',
    ])
    // 滑块搬出预览卡：预览只负责显示形态，不再夹带尺寸调节。
    expect(wrapper.get('.module-catalog__preview').find('.base-slider__input').exists()).toBe(false)
  })
  it('reverts the slider when the write fails', async () => {
    const wrapper = mountWith(backendConfig(FLOATING_BASE_CELL_PX))
    invokeMock.mockRejectedValueOnce(new Error('disk full'))
    await slider(wrapper).setValue('70')
    await flushPromises()

    // 落盘失败：滑块退回后端已知值，界面不会停在一个没存下来的数上。
    expect(saved).toBeNull()
    expect(slider(wrapper).element.value).toBe(String(FLOATING_BASE_CELL_PX))
    expect(wrapper.get('.module-catalog__cell-value').text()).toBe(`${FLOATING_BASE_CELL_PX}px`)
  })
})

describe('FloatingModulesStore · 格子大小', () => {
  /**
   * 原生 range 会把越界值截到边界，所以「落盘前夹取」这条防线只能从 store 这一层验。
   * 它保护的是程序化调用（组件、将来别的入口）不会把 0 或 4000 写进配置 ——
   * 0 会让窗口塌成一条线。
   */
  it('clamps the cell size before persisting', async () => {
    const store = useFloatingModulesStore()
    store.applyConfig(backendConfig(FLOATING_BASE_CELL_PX))

    await store.updateCellPx(5)
    expect(saved?.cell_px).toBe(MIN_CELL_PX)

    await store.updateCellPx(4000)
    expect(saved?.cell_px).toBe(MAX_CELL_PX)
  })
})