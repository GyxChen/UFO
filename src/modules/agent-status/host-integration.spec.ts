// @vitest-environment happy-dom

import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import FloatingModuleHost from '@/components/floating/FloatingModuleHost.vue'
import type { FloatingModuleConfig } from '@/lib/floating-modules'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import { useAgentStatusStore } from './stores/agentStatus'

/**
 * 模块 × 宿主的契约测试：**不注入假模块**，走真实注册表，
 * 验证内置的状态灯模块与宿主 / 槽位（格子需求、分槽、渲染）的契约对得上。
 *
 * 刻意放在模块目录里：宿主自己的测试只用假模块，模块的集成验证属于模块 ——
 * 这样外壳测试不会因为某个组件的内部改动而红，模块也能自己加验证。
 */
const config: FloatingModuleConfig = {
  schema_version: 1,
  revision: 5,
  modules: [{ id: 'agent-status', enabled: true, order: 0 }],
  max_cells_by_layout: { square: 8, corner: 8, horizontal: 8, vertical: 8 },
  // 本组件自己的配置块（外壳只按 id 存，不解释内容）。
  module_settings: { 'agent-status': { completed_retention_ms: 30_000, running_timeout_ms: 60_000 } },
}

/** 造 5 个假会话，返回最近一次上报的窗口格子数。 */
function seedSessions(count: number) {
  useAgentStatusStore().applySnapshot({
    revision: 1,
    source_health: 'healthy',
    sessions: Array.from({ length: count }, (_, index) => ({
      key: `claude:session-${index}`,
      provider: 'claude',
      session_id: `session-${index}`,
      phase: 'running',
      source: 'hook',
      confidence: 100,
      started_at_ms: index,
      last_seen_at_ms: index,
      completed_at_ms: null,
      remove_at_ms: null,
      is_flashing: false,
      pid: null,
      error_code: null,
      base_url: null,
      upstream_url: null,
      prompt: null,
      waiting_reason: null,
      tokens: null,
      last_sequence: null,
      termination_confirmed: false,
    })),
  })
}

describe('agent-status 与宿主的契约', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('mounts through the registry and reports its cell demand', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    useFloatingModulesStore().applyConfig(config)
    seedSessions(5)

    // 不注入 definitions：走真实注册表，验证内置模块与新宿主 / 槽位的契约对得上。
    const wrapper = mount(FloatingModuleHost, {
      props: { layoutMode: 'horizontal' },
      global: { plugins: [pinia] },
    })
    await wrapper.vm.$nextTick()

    // 5 个会话 → 5 盏灯；窗口按模块上报的 5 格汇总，分槽也是单模块独占。
    expect(wrapper.findAll('.agent-status-module__item')).toHaveLength(5)
    const cells = wrapper.emitted('cells') as [number][] | undefined
    expect(cells?.at(-1)?.[0]).toBe(5)
    expect(wrapper.find('.floating-module-host').attributes('style')).toContain('grid-template-columns: 5fr')
  })
})