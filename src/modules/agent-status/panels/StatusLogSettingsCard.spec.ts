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
import BaseSelect from '@/components/ui/BaseSelect.vue'
import { AGENT_STATUS_MODULE_ID, DEFAULT_AGENT_STATUS_SETTINGS } from '../settings'
import type { StatusLogPage } from '../lib/status-log'
import { useStatusLogStore } from '../stores/statusLog'
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

/** 下拉当前显示的那一档（读数落在触发器里，不在原生 input 上）。 */
function selectLabel(wrapper: ReturnType<typeof mount>): string {
  return wrapper.get('.base-select__value').text().trim()
}

/** 选某一档：`BaseSelect` 的 v-model 与 change 一起发（原生下拉也是这个顺序）。 */
async function chooseRetention(wrapper: ReturnType<typeof mount>, value: string) {
  const select = wrapper.findComponent(BaseSelect)
  await select.vm.$emit('update:modelValue', value)
  await select.vm.$emit('change', value)
  await flushPromises()
}

describe('StatusLogSettingsCard', () => {
  it('shows the switch as off when no log block was stored', async () => {
    const wrapper = await cardWith({})
    expect(wrapper.find('input[type=checkbox]').attributes('checked')).toBeUndefined()
    // 卡头常驻说明已收进问号角标（面板 Teleport 到 body，常驻 DOM）：
    // 红线句必须还在 —— 关闭 = 不写盘、且不影响下面的会话存档。
    const pops = [...document.body.querySelectorAll('.settings-hint__pop')].map(p => p.textContent ?? '')
    expect(pops.some(t => t.includes('不影响下面的会话存档'))).toBe(true)
    // 保留天数那一栏在关闭态照样可用：不再写新的不等于已有账不必回收。
    expect(wrapper.findComponent(BaseSelect).exists()).toBe(true)
  })

  it('writes level and hook_enabled together, keeping the rest of the block', async () => {
    const wrapper = await cardWith({
      // 14 是下拉里真能选中的一档：这次落盘就该只有 level 那一对变了，保留期不许被
      // 「就近取档」顺手改掉（那种归一只该在用户拨它、或盘上值选不出来时发生）。
      log: { ...DEFAULT_AGENT_STATUS_SETTINGS.log, retention_days: 14, live_tail_bytes: 1048576 },
    })
    await wrapper.find('input[type=checkbox]').setValue(true)
    await flushPromises()

    expect(savedLog()).toMatchObject({
      level: 'verbose',
      hook_enabled: true,
      // ⚠️ 这一行没动的字段必须原样带回去：浅合并下漏掉就等于把它们删了。
      retention_days: 14,
      live_tail_bytes: 1048576,
    })
    // 开关落盘不许把下拉的显示顶回去：同一份块、两个控件，各自说各自的事。
    expect(selectLabel(wrapper)).toBe('14 天')
  })

  /**
   * 下拉只有 7 / 14 两档，而旧版数字输入框能在盘上留下 1–90 之间的任何数。
   * 显示哪一档就必须把那一档写回去：否则「面板说 14 天、后端按 3 天删」。
   */
  it('snaps an off-grid stored value to a selectable choice and writes it back', async () => {
    const wrapper = await cardWith({
      log: { ...DEFAULT_AGENT_STATUS_SETTINGS.log, retention_days: 3, live_tail_bytes: 1048576 },
    })
    await flushPromises()

    // 就近取档（3 → 7），并且整块写回：未暴露字段一个都不能丢。
    expect(selectLabel(wrapper)).toBe('7 天')
    expect(savedLog()).toMatchObject({ retention_days: 7, live_tail_bytes: 1048576 })
  })

  it('writes the chosen day count and keeps the rest of the block', async () => {
    const wrapper = await cardWith({
      log: { ...DEFAULT_AGENT_STATUS_SETTINGS.log, retention_days: 7, live_tail_bytes: 1048576 },
    })
    await chooseRetention(wrapper, '14')

    expect(savedLog()).toMatchObject({ retention_days: 14, live_tail_bytes: 1048576 })
    expect(selectLabel(wrapper)).toBe('14 天')
  })

  it('reverts the choice when the write fails', async () => {
    const wrapper = await cardWith({
      log: { ...DEFAULT_AGENT_STATUS_SETTINGS.log, retention_days: 7 },
    })
    invokeMock.mockImplementationOnce(async () => {
      throw new Error('disk busy')
    })
    await chooseRetention(wrapper, '14')

    // 落盘失败不许停在「看起来改成 14 天了」。
    expect(selectLabel(wrapper)).toBe('7 天')
  })

  /**
   * 占用读数是「当前状态」，按项目约定常驻，不藏进问号角标。
   *
   * ⚠️ 首拉之前那句必须是「占用读取中…」而不是「还没写过日志」：后者是一个会被当成
   * 事实的错误读数。
   */
  it('shows the disk usage readout only after the log store has answered', async () => {
    const wrapper = await cardWith({})
    expect(wrapper.text()).toContain('占用读取中…')

    const page = {
      head: 0,
      total: 0,
      dropped: 0,
      dir: 'X:\\log',
      entries: [],
      usage: {
        bytes: Math.round(1.5 * 1024 * 1024),
        files: 3,
        oldest_day: '2026-09-22',
        cut_by_size_cap: true,
      },
    } as unknown as StatusLogPage
    useStatusLogStore().applyPage(page, 'replace')
    await flushPromises()

    expect(wrapper.text()).toContain('占用 1.5 MiB · 3 个文件 · 最早 09-22')
    // 位置：读数走 `SettingsField` 的 state，在标题**下面**一行 —— 右侧那一格属于下拉控件。
    expect(wrapper.get('.settings-field__state').text()).toContain('占用 1.5 MiB')
    expect(wrapper.get('.settings-field__head').text(), '读数不许挤回标题行').not.toContain('占用')
    // 容量线动过手就必须写出来：界面上只有天数一个旋钮，不说等于承诺了一个做不到的保留期。
    expect(wrapper.text()).toContain('更早的已按容量上限回收')
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
  })
})
