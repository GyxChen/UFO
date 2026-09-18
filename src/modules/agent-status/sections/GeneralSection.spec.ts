// @vitest-environment happy-dom

/**
 * 常规设置页的回归测试：读 / 写都必须走**本组件自己的配置块**。
 *
 * 这是「配置下放」之后最容易悄悄退化的地方 —— 页面一旦又去读外壳 store 上的
 * 状态灯专有 getter，外壳就重新认识了模块字段（结构上不应该再存在那些 getter，
 * 所以这里锁的是「页面的取值来源」）。
 */
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import GeneralSection from './GeneralSection.vue'
import { AGENT_STATUS_MODULE_ID } from '../settings'
import type { FloatingModuleConfig } from '@/lib/floating-modules'
import { useFloatingModulesStore } from '@/stores/floatingModules'

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))

/** 造一份后端配置；本组件的取值全在它自己的配置块里，另外再挂一块别的组件的。 */
function backendConfig(block: Record<string, unknown> | null = {}): FloatingModuleConfig {
  return {
    schema_version: 1,
    revision: 4,
    modules: [{ id: AGENT_STATUS_MODULE_ID, enabled: true, order: 0 }],
    max_cells_by_layout: { square: 8, corner: 8, horizontal: 8, vertical: 8 },
    module_settings: {
      'other-module': { untouched: true },
      ...(block ? { [AGENT_STATUS_MODULE_ID]: { ...block } } : {}),
    },
  }
}

/** 记下最后一次落盘的配置，供断言「写回了哪个字段」。 */
let saved: FloatingModuleConfig | null = null

beforeEach(() => {
  saved = null
  invokeMock.mockReset()
  invokeMock.mockImplementation(async (command: string, args?: { config?: FloatingModuleConfig }) => {
    if (command === 'get_proxy_resolvers') {
      return [
        { id: 'none', label: '不解析' },
        { id: 'cc-switch', label: 'cc-switch' },
      ]
    }
    if (command === 'set_floating_module_config' && args?.config) {
      saved = args.config
      return { ...args.config, revision: args.config.revision + 1 }
    }
    throw new Error(`unexpected command: ${command}`)
  })
})

/** 把给定配置灌进外壳 store，然后挂载设置页（pinia 显式注入，确保两边是同一份 store）。 */
async function mountWith(config: FloatingModuleConfig) {
  const pinia = createPinia()
  setActivePinia(pinia)
  useFloatingModulesStore().applyConfig(config)
  const wrapper = mount(GeneralSection, { global: { plugins: [pinia] } })
  await flushPromises()
  return wrapper
}

describe('GeneralSection', () => {
  it('renders the values stored in its own config block', async () => {
    const wrapper = await mountWith(
      backendConfig({ completed_retention_ms: 45_000, running_timeout_ms: 120_000 })
    )
    expect(wrapper.get<HTMLInputElement>('#completed-retention-seconds').element.value).toBe('45')
    expect(wrapper.get<HTMLInputElement>('#running-timeout-seconds').element.value).toBe('120')
  })

  it('falls back to defaults when the config has no block for this module', async () => {
    const wrapper = await mountWith(backendConfig(null))
    expect(wrapper.get<HTMLInputElement>('#completed-retention-seconds').element.value).toBe('30')
    expect(wrapper.get<HTMLInputElement>('#running-timeout-seconds').element.value).toBe('60')
  })

  it('writes edits into its own block (seconds in the form, milliseconds on disk)', async () => {
    const wrapper = await mountWith(
      backendConfig({ completed_retention_ms: 45_000, proxy_resolver: 'cc-switch' })
    )
    const input = wrapper.get<HTMLInputElement>('#completed-retention-seconds')
    await input.setValue('90')
    await input.trigger('change')
    await flushPromises()

    const block = saved?.module_settings[AGENT_STATUS_MODULE_ID] as Record<string, unknown>
    expect(block?.completed_retention_ms).toBe(90_000)
    // 同块里的其它字段与别的组件的配置块都必须原样保留。
    expect(block?.proxy_resolver).toBe('cc-switch')
    expect(saved?.module_settings['other-module']).toEqual({ untouched: true })
  })

  it('clamps an out-of-range edit before saving', async () => {
    const wrapper = await mountWith(backendConfig({ completed_retention_ms: 45_000 }))
    const input = wrapper.get<HTMLInputElement>('#completed-retention-seconds')
    await input.setValue('0')
    await input.trigger('change')
    await flushPromises()

    const block = saved?.module_settings[AGENT_STATUS_MODULE_ID] as Record<string, unknown>
    expect(block?.completed_retention_ms).toBe(1_000)
  })
})
