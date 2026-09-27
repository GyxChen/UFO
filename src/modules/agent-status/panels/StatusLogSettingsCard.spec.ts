// @vitest-environment happy-dom

/**
 * 「转灯日志」设置卡的回归测试：一个 switch 必须**成对**写两个字段、**整块**提交、失败要回退。
 *
 * 这三条都是界面上看不出来但会错的：
 * - 只写 `level` 会留下 `hook_enabled: false`，hook 侧那一行账（含适配器静默忽略）永远是瞎的；
 * - `updateModuleSettings` 是模块层浅合并，`log` 只传半个子块会把保留期等字段抹掉；
 * - 落盘失败若不回退本地值，界面就停在「显示在记、盘上没记」。
 */
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import StatusLogSettingsCard from './StatusLogSettingsCard.vue'
import { AGENT_STATUS_MODULE_ID, DEFAULT_AGENT_STATUS_SETTINGS } from '../settings'
import type { FloatingModuleConfig } from '@/lib/floating-modules'
import { useFloatingModulesStore } from '@/stores/floatingModules'

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))

/** 造一份后端配置；只填本组件的配置块。 */
function backendConfig(block: Record<string, unknown>): FloatingModuleConfig {
  return {
    schema_version: 1,
    revision: 4,
    modules: [{ id: AGENT_STATUS_MODULE_ID, enabled: true, order: 0 }],
    module_settings: { [AGENT_STATUS_MODULE_ID]: block },
  }
}

let saved: FloatingModuleConfig | null = null

beforeEach(() => {
  saved = null
  setActivePinia(createPinia())
  invokeMock.mockReset()
  invokeMock.mockImplementation(async (command: string, args?: { config?: FloatingModuleConfig }) => {
    if (command !== 'set_floating_module_config') return null
    saved = args?.config ?? null
    return saved
  })
})

/**
 * 挂好卡片并把后端配置灌进外壳 store。
 *
 * ⚠️ pinia 必须显式注入 mount（同 `GeneralSection.spec.ts`）：不注入时组件与测试各持一份
 * store，配置改了组件也不会重算 —— 测试会假绿。
 */
async function cardWith(block: Record<string, unknown>) {
  const pinia = createPinia()
  setActivePinia(pinia)
  useFloatingModulesStore().applyConfig(backendConfig(block))
  const wrapper = mount(StatusLogSettingsCard, { global: { plugins: [pinia] } })
  await flushPromises()
  return wrapper
}

/** 取最后一次落盘里本组件的 `log` 子块。 */
function savedLog(): Record<string, unknown> {
  return (saved?.module_settings?.[AGENT_STATUS_MODULE_ID] as Record<string, unknown>)?.log as Record<
    string,
    unknown
  >
}

describe('StatusLogSettingsCard', () => {
  it('shows the switch as off when no log block was stored', async () => {
    const wrapper = await cardWith({})
    expect(wrapper.find('input[type=checkbox]').attributes('checked')).toBeUndefined()
    expect(wrapper.text()).toContain('已关闭 · 零写盘')
    // 关闭态不给风险文案：那时候根本没有东西在写。
    expect(wrapper.find('.lg-settings__risk').exists()).toBe(false)
  })

  it('writes level and hook_enabled together, keeping the rest of the block', async () => {
    const wrapper = await cardWith({
      log: { ...DEFAULT_AGENT_STATUS_SETTINGS.log, retention_days: 3, ring_capacity: 900 },
    })
    await wrapper.find('input[type=checkbox]').setValue(true)
    await flushPromises()

    expect(savedLog()).toMatchObject({
      level: 'verbose',
      hook_enabled: true,
      // ⚠️ 未暴露的两个字段必须原样带回去：浅合并下漏掉就等于把它们删了。
      retention_days: 3,
      ring_capacity: 900,
    })
    expect(wrapper.text()).toContain('正在记 · 明细级 + hook')
    // 打开后风险告知常驻（明细级会落提问摘录）。
    expect(wrapper.find('.lg-settings__risk').exists()).toBe(true)
  })

  it('reverts the switch when the write fails', async () => {
    const wrapper = await cardWith({})
    invokeMock.mockImplementationOnce(async () => {
      throw new Error('disk busy')
    })
    await wrapper.find('input[type=checkbox]').setValue(true)
    await flushPromises()

    // 界面不许停在「看起来开了」而配置没落盘。
    expect(wrapper.find('input[type=checkbox]').element.checked).toBe(false)
    expect(wrapper.text()).toContain('已关闭 · 零写盘')
  })
})
