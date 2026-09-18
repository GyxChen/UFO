// @vitest-environment happy-dom

import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import { defineComponent, h, inject } from 'vue'
import FloatingModuleHost from './FloatingModuleHost.vue'
import { floatingModuleKey } from '@/lib/floating-module-context'
import type { FloatingLayout } from '@/lib/floating-layout'
import type {
  FloatingModuleConfig,
  FloatingModuleDefinition,
  FloatingRuntimeSnapshot,
} from '@/lib/floating-modules'
import { useFloatingModulesStore } from '@/stores/floatingModules'

/**
 * 假模块：像真模块一样注入 floatingModuleKey，
 * 把分到的格子数与当前形态渲染进 DOM 供断言（格子需求声明在定义里）。
 */
const stubModule = (cells: number, key = 'stub-key') =>
  defineComponent({
    name: `StubModule${cells}`,
    props: { layoutMode: { type: String, required: true } },
    setup(props) {
      const api = inject(floatingModuleKey, null)
      return () =>
        h(
          'button',
          {
            class: 'stub',
            onClick: () => api?.reportHover(key),
            onKeydown: () => api?.reportHover(null),
          },
          `${props.layoutMode}:${api?.allocatedCells.value ?? 'none'}`
        )
    },
  })

const definitions: FloatingModuleDefinition[] = [
  {
    id: 'stub-a',
    label: '假模块 A',
    description: '占位模块',
    version: 1,
    defaultEnabled: true,
    supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
    component: stubModule(4),
    demand: () => 4,
  },
  {
    id: 'extra',
    label: '同级组件',
    description: '占位模块',
    version: 1,
    defaultEnabled: true,
    supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
    component: stubModule(2, 'extra-key'),
    demand: () => 2,
  },
]

const config = (extraEnabled: boolean, cellPx?: number): FloatingModuleConfig => ({
  schema_version: 1,
  revision: 5,
  modules: [
    { id: 'stub-a', enabled: true, order: 0 },
    { id: 'extra', enabled: extraEnabled, order: 1 },
  ],
  max_cells_by_layout: { square: 8, corner: 8, horizontal: 8, vertical: 8 },
  ...(cellPx === undefined ? {} : { cell_px: cellPx }),
  module_settings: {},
})

/** 装配一次宿主：注入 pinia 与模块清单，并按形态挂载。 */
function mountHost(
  layoutMode: FloatingLayout,
  options: { extraEnabled?: boolean; definitions?: FloatingModuleDefinition[]; cellPx?: number } = {}
) {
  const pinia = createPinia()
  setActivePinia(pinia)
  useFloatingModulesStore().applyConfig(config(options.extraEnabled ?? true, options.cellPx))
  return mount(FloatingModuleHost, {
    props: { layoutMode, definitions: options.definitions ?? definitions },
    global: { plugins: [pinia] },
  })
}

/** 最近一次上报的窗口格子数。 */
function lastCells(wrapper: ReturnType<typeof mountHost>): number {
  const events = wrapper.emitted('cells') as [number][] | undefined
  return events?.[events.length - 1]?.[0] ?? Number.NaN
}

describe('FloatingModuleHost', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('mounts peer modules and sums their cell demands into the window size', async () => {
    const wrapper = mountHost('square')
    await wrapper.vm.$nextTick()

    // 两个同级模块（4 格 + 2 格）共享窗口：窗口应为 6 格。
    expect(wrapper.findAll('.stub')).toHaveLength(2)
    expect(lastCells(wrapper)).toBe(6)
  })

  it('splits the window between modules by their demands', async () => {
    const wrapper = mountHost('square')
    await wrapper.vm.$nextTick()

    // 6 格按 4:2 切分，各模块渲染出自己分到的格子数。
    expect(wrapper.findAll('.stub').map(item => item.text())).toEqual(['square:4', 'square:2'])
  })

  it('lays the slots out along the axis of the current shape', async () => {
    const wrapper = mountHost('horizontal')
    await wrapper.vm.$nextTick()
    // 横条：按宽度分槽，模块左右并排。
    expect(wrapper.find('.floating-module-host').attributes('style')).toContain('grid-template-columns: 4fr 2fr')

    const vertical = mountHost('vertical')
    await vertical.vm.$nextTick()
    // 竖条 / 方形：按高度分槽，模块上下堆叠。
    expect(vertical.find('.floating-module-host').attributes('style')).toContain('grid-template-rows: 4fr 2fr')
  })

  it('skips modules that do not support the current shape', async () => {
    // extra 只声明支持竖条：横条下不应挂载，窗口尺寸回到单模块的 4 格。
    const portraitOnly = [
      { ...definitions[0] },
      { ...definitions[1], supportedLayouts: ['vertical'] as FloatingLayout[] },
    ]
    const wrapper = mountHost('horizontal', { definitions: portraitOnly })
    await wrapper.vm.$nextTick()
    expect(wrapper.findAll('.stub')).toHaveLength(1)
    expect(lastCells(wrapper)).toBe(4)

    const portrait = mountHost('vertical', { definitions: portraitOnly })
    await portrait.vm.$nextTick()
    expect(portrait.findAll('.stub')).toHaveLength(2)
  })

  it('forwards hover reports with the reporting module id', async () => {
    const wrapper = mountHost('square')
    await wrapper.vm.$nextTick()

    await wrapper.findAll('.stub')[1].trigger('click')
    expect(wrapper.emitted('hover')?.at(-1)).toEqual([{ moduleId: 'extra', key: 'extra-key' }])

    await wrapper.findAll('.stub')[0].trigger('keydown')
    expect(wrapper.emitted('hover')?.at(-1)).toEqual([null])
  })

  it('emits a per-shape runtime snapshot for the main window preview', async () => {
    const wrapper = mountHost('square')
    await wrapper.vm.$nextTick()

    const snapshot = (wrapper.emitted('runtime') as [FloatingRuntimeSnapshot][]).at(-1)?.[0]
    // 需求按形态分别求值：当前形态是方形，但四种形态都要有数据（主窗口要画三形态预览）。
    expect(Object.keys(snapshot?.byLayout ?? {})).toEqual(['square', 'corner', 'horizontal', 'vertical'])
    expect(snapshot?.byLayout.horizontal.demands).toEqual({ 'stub-a': 4, extra: 2 })
    expect(snapshot?.byLayout.horizontal.windowCells).toBe(6)
  })

  it('leaves shape-specific modules out of the other shapes in the snapshot', async () => {
    const portraitOnly = [
      { ...definitions[0] },
      { ...definitions[1], supportedLayouts: ['vertical'] as FloatingLayout[] },
    ]
    const wrapper = mountHost('horizontal', { definitions: portraitOnly })
    await wrapper.vm.$nextTick()

    const snapshot = (wrapper.emitted('runtime') as [FloatingRuntimeSnapshot][]).at(-1)?.[0]
    // 只支持竖条的模块不出现在横条 / 方形快照里，窗口也随之回到 4 格。
    expect(snapshot?.byLayout.horizontal.demands).toEqual({ 'stub-a': 4 })
    expect(snapshot?.byLayout.horizontal.windowCells).toBe(4)
    expect(snapshot?.byLayout.vertical.demands).toEqual({ 'stub-a': 4, extra: 2 })
  })

  it('ignores disabled modules', async () => {
    const wrapper = mountHost('square', { extraEnabled: false })
    await wrapper.vm.$nextTick()
    expect(wrapper.findAll('.stub')).toHaveLength(1)
    expect(lastCells(wrapper)).toBe(4)
  })

  it('publishes the cell size to modules as inherited css variables', async () => {
    // 90px = 设计基准 45px 的两倍：模块用这个倍率把写死的设计像素（字号 / 描边 / 图标上限）等比放大。
    const wrapper = mountHost('square', { cellPx: 90 })
    await wrapper.vm.$nextTick()

    const style = wrapper.get<HTMLElement>('.floating-module-host').element.style
    expect(style.getPropertyValue('--floating-cell-px')).toBe('90px')
    expect(style.getPropertyValue('--floating-cell-scale')).toBe('2')
  })

  it('falls back to the design scale when the config has no cell size', async () => {
    const wrapper = mountHost('square')
    await wrapper.vm.$nextTick()

    expect(wrapper.get<HTMLElement>('.floating-module-host').element.style.getPropertyValue('--floating-cell-scale')).toBe(
      '1'
    )
  })

  it('republishes the scale when the user changes the cell size', async () => {
    const wrapper = mountHost('square')
    await wrapper.vm.$nextTick()

    // 配置变化（主窗口拖滑块）后倍率要立刻跟上，真窗口与模块一起重算。
    useFloatingModulesStore().applyConfig({ ...config(true, 64), revision: 6 })
    await wrapper.vm.$nextTick()

    // 64 / 45 ≈ 1.422，保留三位小数。
    expect(wrapper.get<HTMLElement>('.floating-module-host').element.style.getPropertyValue('--floating-cell-scale')).toBe(
      '1.422'
    )
  })
})
