// @vitest-environment happy-dom

/**
 * 溢出格（「分片环」）与折叠清单的集成护栏。
 *
 * 核心要锁住的是一件事：**屏上的环**与**悬停清单**必须同源 ——
 * 悬浮窗按「形态 + 格子数」算出被折叠的会话，提示窗是另一个窗口、拿不到这两样，
 * 只能靠溢出格的合成悬停 key（`@overflow:<layout>:<cells>`）把它们带过去，
 * 再用同一个纯函数重算一遍。任何一侧改了口径，这里的断言就会红。
 */
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { computed } from 'vue'
import AgentStatusModule from './AgentStatusModule.vue'
import AgentStatusTooltip from './AgentStatusTooltip.vue'
import { agentStatusModule } from './index'
import { buildMockStatusSnapshot } from './lib/mock'
import { overflowHoverKey } from './lib/agent-status-layout'
import {
  AGENT_TOOLTIP_SESSION_HEIGHT,
  AGENT_TOOLTIP_WIDTH,
  OVERFLOW_TOOLTIP_METRICS,
} from './lib/tooltip-size'
import { floatingModuleKey } from '@/lib/floating-module-context'
import type { FloatingLayout } from '@/lib/floating-layout'
import { useAgentStatusStore } from './stores/agentStatus'

/** 屏上折叠 5 个会话时的占比：红 1 / 黄 1 / 绿 3（假会话按阶段轮换，见 lib/mock.ts）。 */
const RING_5 =
  'conic-gradient(var(--ufo-status-red) 0% 20%, var(--ufo-status-yellow) 20% 40%, var(--ufo-status-green) 40% 100%)'

/**
 * 灌入假会话快照（假会话自带 prompt / provider / 阶段轮换，够覆盖清单的每一列）。
 *
 * ⚠️ 只给「不挂载组件」的断言用（如直接调 `hasContent`）：测试全局装的是 `@pinia/testing`
 * 的实例，挂载时组件会连上**它**，而不是 `setActivePinia` 设的那个 —— 所以挂载的场景
 * 统一走下面两个 helper（自建 pinia + 显式装进 app），数据才真的进得去。
 */
function seed(count: number) {
  useAgentStatusStore().applySnapshot(buildMockStatusSnapshot(count, { nowMs: 0 }))
}

/** 自建一份 pinia 并灌入假会话，供挂载的组件使用。 */
function freshPinia(count: number) {
  const pinia = createPinia()
  setActivePinia(pinia)
  seed(count)
  return pinia
}

/** 挂载状态灯模块，并把它「分到几格」固定成入参（模块通过注入的上下文读它）。 */
function mountModule(count: number, allocated: number, layout: FloatingLayout = 'square') {
  const reportHover = vi.fn()
  const wrapper = mount(AgentStatusModule, {
    props: { layoutMode: layout },
    global: {
      plugins: [freshPinia(count)],
      provide: { [floatingModuleKey as symbol]: { allocatedCells: computed(() => allocated), reportHover } },
    },
  })
  return { wrapper, reportHover }
}

/** 按悬停目标挂载信息卡（数据同样灌进组件那份 pinia）。 */
function mountTooltip(count: number, key: string) {
  return mount(AgentStatusTooltip, {
    props: { target: { moduleId: 'agent-status', key } },
    global: { plugins: [freshPinia(count)] },
  })
}

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('溢出格 · 分片环', () => {
  it('draws the folded sessions as a share ring and reports a composite hover key', async () => {
    const { wrapper, reportHover } = mountModule(5, 1)

    // 方形 1 格 = 容量 1：5 个会话全被折叠，整窗就是这块环（没有单独可见的灯）。
    expect(wrapper.findAll('.agent-status-module__item')).toHaveLength(1)
    const cell = wrapper.get('.agent-status-module__item--overflow')
    expect(cell.text()).toBe('+5')
    // 环按灯色占比切开，红在前 —— 缩到一格也能看出「折叠里有红的」。
    expect(cell.attributes('style')).toContain(RING_5)
    // 读屏文案：折叠数量 + 按灯色分组的状态。
    expect(cell.attributes('aria-label')).toBe('还有 5 个会话：1 个出错、1 个等待操作、3 个执行中')
    // 环与两圈光晕是纯装饰，不进无障碍树。
    expect(wrapper.findAll('.agent-status-module__ring[aria-hidden="true"]')).toHaveLength(1)
    expect(wrapper.findAll('.agent-status-module__halo[aria-hidden="true"]')).toHaveLength(2)

    // 悬停上报的是合成 key（形态 + 格子数都在里面），不是某个会话的 key。
    await cell.trigger('pointerenter')
    expect(reportHover).toHaveBeenCalledWith(overflowHoverKey('square', 1))
    await cell.trigger('pointerleave')
    expect(reportHover).toHaveBeenLastCalledWith(null)
  })

  it('narrows the count for three-digit folds and keeps the ring when squeezed', () => {
    // 被同级组件挤到 1 格：容量 1 → 折叠 12 个（三位数）。
    const { wrapper } = mountModule(12, 1)
    const count = wrapper.get('.agent-status-module__overflow')
    expect(count.text()).toBe('+12')
    expect(count.classes()).toContain('agent-status-module__overflow--wide')
  })

  it('keeps every lamp visible instead of a ring when everything fits', () => {
    // 贴角 2 格 = 容量 2 → 放得下，没有溢出格。
    const { wrapper } = mountModule(2, 2, 'corner')
    expect(wrapper.findAll('.agent-status-module__item')).toHaveLength(2)
    expect(wrapper.find('.agent-status-module__item--overflow').exists()).toBe(false)
  })
})

describe('溢出格 · 折叠清单', () => {
  it('lists the folded sessions when hovering the overflow cell', () => {
    const wrapper = mountTooltip(5, overflowHoverKey('square', 1))

    expect(wrapper.get('.tooltip-row--head').text()).toContain('后面还有 5 个会话')
    // 头部统计片：与环同一份分片（红 → 黄 → 绿）。
    expect(wrapper.findAll('.tooltip-overflow-chip').map(chip => chip.text())).toEqual(['1', '1', '3'])
    // 单会话信息卡的内容不该出现在清单里。
    expect(wrapper.find('.tooltip-prompt').exists()).toBe(false)

    // 一行一个会话，按注意力优先级排（红在前，与环从 12 点起的第一个扇形对应）。
    const rows = wrapper.findAll('.tooltip-row').filter(row => !row.classes('tooltip-row--head'))
    expect(rows).toHaveLength(5)
    expect(rows[0].find('.tooltip-overflow-row__phase--red').text()).toBe('出错')
    expect(rows[1].find('.tooltip-overflow-row__phase--yellow').text()).toBe('等待操作')
    expect(rows[4].find('.tooltip-overflow-row__phase--green').text()).toBe('执行中')
    // 每行都带平台图标与提问摘要（摘要一行省略，完整原文在 title）。
    expect(rows[0].find('.tooltip-provider-icon').exists()).toBe(true)
    expect(rows[0].get('.tooltip-overflow-row__prompt').text().length).toBeGreaterThan(0)
  })

  it('lists every folded session instead of folding the tail', () => {
    const wrapper = mountTooltip(12, overflowHoverKey('square', 1))

    // 折叠几个就列几行（用户要求全列出来）：12 个会话在 1 格里全被折叠，清单就得是 12 行。
    const rows = wrapper.findAll('.tooltip-row').filter(row => !row.classes('tooltip-row--head'))
    expect(rows).toHaveLength(12)
    expect(wrapper.find('.tooltip-overflow-note').exists()).toBe(false)
    // 行全部落在可滚动的清单容器里（行数超过屏幕时靠它滚，而不是把窗口撑出屏外）。
    expect(wrapper.get('.tooltip-overflow-list').findAll('.tooltip-overflow-row')).toHaveLength(12)
  })

  it('grows the tooltip window with the number of folded sessions', () => {
    const sizeFor = (count: number, key: string) => {
      freshPinia(count)
      const size = agentStatusModule.tooltip!.size
      expect(typeof size, '折叠清单的高度必须按目标现算').toBe('function')
      return (size as (target: { moduleId: string; key: string }) => { width: number; height: number })({
        moduleId: 'agent-status',
        key,
      })
    }

    const few = sizeFor(5, overflowHoverKey('square', 1))
    const many = sizeFor(12, overflowHoverKey('square', 1))
    const single = sizeFor(5, 'claude:mock-0')

    // 单会话卡片仍是固定的一屏预算（`overflowListFor` 对它返回 null）。
    expect(single.height).toBe(AGENT_TOOLTIP_SESSION_HEIGHT)
    // 折叠行越多窗口越高，且都宽 300。
    expect(many.height).toBeGreaterThan(few.height)
    expect([few.width, many.width]).toEqual([AGENT_TOOLTIP_WIDTH, AGENT_TOOLTIP_WIDTH])
    // 行数就是高度账的变量：多出来的高度正好是 7 行 + 7 条行间距。
    const { row, gap } = OVERFLOW_TOOLTIP_METRICS
    expect(many.height - few.height).toBe(7 * (row + gap))
  })

  it('keeps the card non-empty when the fold is gone', () => {
    // 容量 9 > 3 个会话：悬停期间会话结束到放得下，清单空了也不能只剩一个空框。
    const wrapper = mountTooltip(3, overflowHoverKey('square', 9))

    expect(wrapper.findAll('.tooltip-row')).toHaveLength(1) // 只剩头部
    expect(wrapper.get('.tooltip-overflow-note').text()).toBe('折叠的会话都已结束')
  })

  it('still renders the single-session card for a real session key', () => {
    const pinia = freshPinia(5)
    const key = useAgentStatusStore().sessions[0].key
    const wrapper = mount(AgentStatusTooltip, {
      props: { target: { moduleId: 'agent-status', key } },
      global: { plugins: [pinia] },
    })

    expect(wrapper.find('.tooltip-overflow-note').exists()).toBe(false)
    expect(wrapper.findAll('.tooltip-overflow-chip')).toHaveLength(0)
    // 单会话卡片的 Endpoint 行 / 关闭按钮都在。
    expect(wrapper.text()).toContain('Endpoint')
    expect(wrapper.find('.tooltip-close').exists()).toBe(true)
  })
})

describe('空态 · 机器人鬼影', () => {
  it('lays a robot ghost when there is no session', () => {
    const { wrapper } = mountModule(0, 1)

    // 零会话时窗口里什么都不画会像「挂了」，所以铺一层机器人图标的鬼影。
    const idle = wrapper.get('.agent-status-module__idle')
    expect(idle.findAll('.agent-status-module__idle-icon')).toHaveLength(1)
    // 图标是纯装饰（优先级信息没有可承载的对象），读屏拿 sr-only 那一句。
    expect(wrapper.findAll('.agent-status-module__idle-icon[aria-hidden="true"]')).toHaveLength(1)
    expect(idle.get('.sr-only').text()).toBe('当前没有 CLI 会话在运行')
    // 空态下不该有任何灯或溢出格。
    expect(wrapper.find('.agent-status-module__item').exists()).toBe(false)
  })

  it('drops the ghost as soon as a session shows up', () => {
    // 放不下 ≠ 空态：5 个会话挤在 1 格里是「有会话 + 溢出格」，鬼影必须让位。
    expect(mountModule(1, 1).wrapper.find('.agent-status-module__idle').exists()).toBe(false)
    expect(mountModule(5, 1).wrapper.find('.agent-status-module__idle').exists()).toBe(false)
  })
})

describe('溢出格 · 内容就绪判定', () => {
  it('claims tooltip content only when something is actually folded', () => {
    seed(5)
    const hasContent = agentStatusModule.tooltip!.hasContent!

    expect(hasContent({ moduleId: 'agent-status', key: overflowHoverKey('square', 1) })).toBe(true)
    // 容量比会话数还大（悬停期间会话结束了）：不弹空框。
    expect(hasContent({ moduleId: 'agent-status', key: overflowHoverKey('square', 9) })).toBe(false)
    // 真实会话 key 仍走原来的判定。
    expect(hasContent({ moduleId: 'agent-status', key: 'claude:mock-0' })).toBe(true)
    expect(hasContent({ moduleId: 'agent-status', key: 'claude:missing' })).toBe(false)
  })
})
