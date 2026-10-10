// @vitest-environment happy-dom

import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
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
 * 注册表替身：宿主**不注入 `definitions`** 时读的就是这一份。
 *
 * 为什么要替身而不是直接用 `@/modules` 那份真注册表：真表里是内置模块（各自要 store 与 IPC），
 * 宿主单测要验的只是「清单换了宿主跟不跟」，把三个真模块装进测试进程只会引入无关失败。
 */
const { registry, handOver } = vi.hoisted(() => {
  let held: { value: unknown[] } | undefined
  return {
    /** 测试取用：换掉 `.value` 就等于换掉整份注册表。 */
    registry: () => {
      if (!held) throw new Error('注册表替身没建起来：vi.mock 的工厂没跑')
      return held
    },
    /** mock 工厂把建好的那一份交出来（工厂只在首次 import 时跑一次）。 */
    handOver: (ref: { value: unknown[] }) => {
      held = ref
    },
  }
})

vi.mock('@/modules', async () => {
  const { shallowRef } = await import('vue')
  const ref = shallowRef<unknown[]>([])
  handOver(ref)
  return { FLOATING_MODULE_DEFINITIONS: ref, BUILTIN_MODULE_DEFINITIONS: [] }
})

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
  ...(cellPx === undefined ? {} : { cell_px: cellPx }),
  module_settings: {},
})

/** 装配一次宿主：注入 pinia 与模块清单，并按形态挂载。 */
function mountHost(
  layoutMode: FloatingLayout,
  options: {
    extraEnabled?: boolean
    definitions?: FloatingModuleDefinition[]
    cellPx?: number
    /** 不给 `definitions` 这条 prop：宿主该自己去读注册表（真窗里走的就是这条路）。 */
    fromRegistry?: boolean
  } = {}
) {
  const pinia = createPinia()
  setActivePinia(pinia)
  useFloatingModulesStore().applyConfig(config(options.extraEnabled ?? true, options.cellPx))
  return mount(FloatingModuleHost, {
    props: options.fromRegistry
      ? { layoutMode }
      : { layoutMode, definitions: options.definitions ?? definitions },
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

  afterEach(() => {
    // 注册表替身是模块级状态：不清会把上一条测试的清单带进下一条。
    registry().value = []
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

  it('splits corner slots by the rows each module needs', async () => {
    const wrapper = mountHost('corner')
    await wrapper.vm.$nextTick()
    // 四角逐个模块各占满一整行，行高按它自己占几行：4 格 = 2×2 → 2 行，2 格 = 1×2 → 1 行。
    // （若按原始需求面积分会是 4fr 2fr，只在各模块同宽时才对；这里锁「按行数」的正确口径。）
    expect(wrapper.find('.floating-module-host').attributes('style')).toContain('grid-template-rows: 2fr 1fr')
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

  it('不注入清单时读注册表，并且注册表晚到也要跟上', async () => {
    // 真窗里没有人给宿主传清单（那条路只有测试走），它读的是注册表；
    // 而组件包是**异步**装载的 —— 悬浮窗挂载那一刻注册表里只有内置模块，
    // 晚到的那一份如果进不了格子账，表现就是「包装上了、徽标也齐，但悬浮窗少一格」。
    registry().value = []
    const wrapper = mountHost('square', { fromRegistry: true })
    await wrapper.vm.$nextTick()
    expect(wrapper.findAll('.stub')).toHaveLength(0)

    registry().value = [{ ...definitions[0], id: 'late', demand: () => 2 }]
    await wrapper.vm.$nextTick()

    expect(wrapper.findAll('.stub').map(item => item.text())).toEqual(['square:2'])
    expect(lastCells(wrapper)).toBe(2)
  })

  it('格子用 Symbol.for 的那枚键上报悬停（组件包产物走的正是这一条）', async () => {
    // 组件包的 JS 是另一份产物，拿不到宿主的 `floatingModuleKey` 变量本身，
    // 只能按注册表名取键 —— 所以这里刻意不用 import 的那一枚，模拟「外来的」组件。
    const ForeignCell = defineComponent({
      name: 'ForeignCell',
      setup: () => {
        const api = inject(Symbol.for('ufo.floating.module')) as { reportHover: (key: string | null) => void } | null
        return () =>
          h('button', { class: 'foreign', onPointerenter: () => api?.reportHover('from-plugin') }, 'f')
      },
    })
    registry().value = [{ ...definitions[0], id: 'foreign', component: ForeignCell, demand: () => 1 }]

    const wrapper = mountHost('square', { fromRegistry: true })
    await wrapper.vm.$nextTick()
    await wrapper.find('.foreign').trigger('pointerenter')

    expect(wrapper.emitted('hover')?.at(-1)).toEqual([{ moduleId: 'foreign', key: 'from-plugin' }])
  })
})
