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
    // 无响应超时的输入单位是分钟：120_000ms 显示为 2。
    expect(wrapper.get<HTMLInputElement>('#running-timeout-minutes').element.value).toBe('2')
  })

  it('falls back to defaults when the config has no block for this module', async () => {
    const wrapper = await mountWith(backendConfig(null))
    expect(wrapper.get<HTMLInputElement>('#completed-retention-seconds').element.value).toBe('30')
    expect(wrapper.get<HTMLInputElement>('#running-timeout-minutes').element.value).toBe('1')
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

  it('saves the max-cells slider into its own config block', async () => {
    const wrapper = await mountWith(backendConfig({}))
    const input = wrapper.get<HTMLInputElement>('.base-slider__input')
    // 滑块按档位下标驱动：拖到下标 2 = AGENT_MAX_CELL_STEPS[2] = 3 盏灯，松手落盘。
    await input.setValue('2')
    await input.trigger('change')
    await flushPromises()

    // 格子上限是本组件自己的配置，写进 module_settings['agent-status']。
    const block = saved?.module_settings[AGENT_STATUS_MODULE_ID] as Record<string, unknown>
    expect(block?.max_cells).toBe(3)
  })

  it('labels every slider tick with the corner shape of its step', async () => {
    const wrapper = await mountWith(backendConfig({}))

    // 刻度与档位一一对应，标注是各档贴到四角时的方块形状。
    const ticks = wrapper.findAll('.base-slider__tick')
    expect(ticks.map(tick => tick.text())).toEqual(['1×1', '1×2', '1×3', '2×2', '2×3', '3×3'])

    // 当前档的刻度加亮，其余保持灰。
    const active = ticks.filter(tick => tick.classes().includes('base-slider__tick--active'))
    expect(active).toHaveLength(1)
    expect(active[0]?.text()).toBe('3×3') // 默认上限 9 盏灯 = 3×3
  })

  it('renders the status sound switch from its own config block', async () => {
    // 缺字段 = 默认开：静音是明确动作，不该由「没配过」推出来。
    const fallback = await mountWith(backendConfig({}))
    expect(fallback.get<HTMLInputElement>('.base-switch__input').element.checked).toBe(true)

    const muted = await mountWith(backendConfig({ sound_enabled: false }))
    expect(muted.get<HTMLInputElement>('.base-switch__input').element.checked).toBe(false)
    // 可见标签与无障碍名称都在（开关本身没有可见文字）。
    expect(muted.text()).toContain('状态音效')
    expect(muted.get('.base-switch__input').attributes('aria-label')).toBe('状态音效')
  })

  it('merges the cells slider and status sound rows into one header-less right card', async () => {
    const wrapper = await mountWith(backendConfig({}))

    // 两块卡并排、都是 `.general-cards` 的直接子节点；都不带卡头（「行为 / 即时生效」已删）。
    const cards = [...wrapper.get('.general-cards').element.children]
    expect(cards).toHaveLength(2)
    for (const card of cards) {
      expect(card.querySelector('.ufo-card__header'), '设置卡不再要汇总性卡头').toBeNull()
    }

    // 左卡 = 四个取值行；右卡 = 「窗口格子」（滑块贴右）+「状态音效」（开关贴右）。
    expect(cards[0]?.textContent).toContain('Token 单位')
    const fields = [...cards[1]!.querySelectorAll('.settings-field')]
    expect(fields.map(f => f.querySelector('.settings-field__label')?.textContent?.trim())).toEqual([
      '窗口格子',
      '状态音效',
    ])
    expect(fields[0]?.querySelector('.settings-field__control .base-slider')).toBeTruthy()
    // 「最多 N 盏灯」读数常驻在「窗口格子」标题下面（刻度只标形状，盏数要换算）。
    expect(fields[0]?.querySelector('.settings-field__state')?.textContent).toBe('最多 9 盏灯')
    expect(fields[1]?.querySelector('.settings-field__control .base-switch')).toBeTruthy()
  })

  it('points the user at the replaceable sound folder under the install path', async () => {
    await mountWith(backendConfig({}))
    // 换音频说明是静态解释 → 收进问号角标（面板 Teleport 到 body，常驻 DOM）；
    // 音频在安装路径下，提示写的是那句固定文案（不回显开发期的绝对路径）。
    const pops = [...document.body.querySelectorAll('.settings-hint__pop')].map(p => p.textContent ?? '')
    expect(
      pops.some(t => t.includes('安装路径') && t.includes('data/bgm') && t.includes('goError.mp3'))
    ).toBe(true)
  })

  it('writes the status sound toggle into its own config block', async () => {
    const wrapper = await mountWith(backendConfig({ sound_enabled: false, completed_retention_ms: 45_000 }))
    await wrapper.get('.base-switch__input').setValue(true)
    await flushPromises()

    const block = saved?.module_settings[AGENT_STATUS_MODULE_ID] as Record<string, unknown>
    expect(block?.sound_enabled).toBe(true)
    // 同块的其它字段必须原样保留（浅合并，不是整块替换）。
    expect(block?.completed_retention_ms).toBe(45_000)
    expect(saved?.module_settings['other-module']).toEqual({ untouched: true })
  })

  it('reverts the status sound switch when saving fails', async () => {
    // 落盘失败时开关要弹回已知值，不能显示一个并没生效的状态。
    const wrapper = await mountWith(backendConfig({ sound_enabled: false }))
    invokeMock.mockRejectedValueOnce(new Error('offline'))

    const toggle = wrapper.get<HTMLInputElement>('.base-switch__input')
    await toggle.setValue(true)
    await flushPromises()

    expect(toggle.element.checked).toBe(false)
  })
})
