// @vitest-environment happy-dom

import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import FloatingModuleHost from '@/components/floating/FloatingModuleHost.vue'
import type { FloatingModuleConfig } from '@/lib/floating-modules'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import { useThemeStore } from '@/stores/theme'
import AgentStatusModule from './AgentStatusModule.vue'
import { useAgentStatusStore } from './stores/agentStatus'
import codexIcon from '@/assets/icon/codex.svg'
import codexIconLight from '@/assets/icon/codex-light.svg'

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
  // 本组件自己的配置块（外壳只按 id 存，不解释内容）。
  module_settings: { 'agent-status': { completed_retention_ms: 30_000, running_timeout_ms: 60_000 } },
}

/** 造 N 个假会话（provider 默认 claude；测图标分档时用 codex —— 它两档资源不同）。 */
function seedSessions(count: number, provider = 'claude') {
  useAgentStatusStore().applySnapshot({
    revision: 1,
    source_health: 'healthy',
    sessions: Array.from({ length: count }, (_, index) => ({
      key: `${provider}:session-${index}`,
      provider,
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

  it('平台图标按主题换资源，不靠 CSS 反相', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    seedSessions(1, 'codex')
    const theme = useThemeStore(pinia)

    const wrapper = mount(AgentStatusModule, { props: { layoutMode: 'corner' }, global: { plugins: [pinia] } })
    const icon = () => wrapper.get('.agent-status-module__icon')
    // 深色档取白色那份；浅色档取深色墨那份 —— 两档各有各的资源。
    expect(icon().attributes('src')).toBe(codexIcon)

    theme.$patch({ theme: 'light' })
    await wrapper.vm.$nextTick()
    expect(icon().attributes('src')).toBe(codexIconLight)
    // ⚠️ 反相滤镜只留给纯白单色的控件图标：平台 mark 上再挂 `.ufo-mono-icon` 就是把老方案请回来，
    // 白 + 品牌绿的 Qoder 会被翻成品红。
    expect(icon().classes()).not.toContain('ufo-mono-icon')
  })
})