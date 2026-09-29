// @vitest-environment happy-dom

import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import SettingsField from './SettingsField.vue'

/**
 * 字段原语的分工：`hint` = 解释（收进问号角标，面板 Teleport 到 body），
 * `state` = 当前状态读数（**必须常驻**，藏进 hover 等于没写）。
 * 这两条一旦写反，表现是「扫一眼看不出这栏存没存过」——所以逐条锁住。
 */

/** 取角标悬浮面板（在 body 上，`wrapper.find` 够不到）。 */
function hintText(): string {
  return document.querySelector('.settings-hint__pop')?.textContent?.trim() ?? ''
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('SettingsField', () => {
  it('keeps the native label target and tucks the hint behind the question icon', async () => {
    const wrapper = mount(SettingsField, {
      props: {
        label: '代理解析器',
        labelFor: 'proxy-resolver',
        hint: '把本地代理地址反查成上游 endpoint',
      },
      slots: { default: '<button id="proxy-resolver">不解析</button>' },
      attachTo: document.body,
    })
    await flushPromises()

    expect(wrapper.get('label').attributes('for')).toBe('proxy-resolver')
    // 标签行里有角标，正文只在展开的面板里（不再常驻一行灰字）
    expect(wrapper.find('.settings-hint').exists()).toBe(true)
    expect(wrapper.get('.settings-field__head').text()).not.toContain('反查成上游')
    expect(hintText()).toContain('反查成上游')
  })

  it('supports aria-labelledby targets for custom controls', async () => {
    const wrapper = mount(SettingsField, {
      props: { label: 'Token 单位', labelId: 'token-unit-label' },
      slots: { default: '<div aria-labelledby="token-unit-label">Tokens</div>' },
      attachTo: document.body,
    })
    await flushPromises()

    expect(wrapper.get('.settings-field__label').attributes('id')).toBe('token-unit-label')
    // 没有解释也没有读数时，标签行不该多出一个空角标
    expect(wrapper.find('.settings-hint').exists()).toBe(false)
  })

  it('shows the state readout inline instead of behind the icon', async () => {
    const wrapper = mount(SettingsField, {
      props: {
        label: 'API Key',
        labelFor: 'weather-api-key',
        state: '已保存至凭据管理器',
        hint: '输入新值后点保存即可替换',
      },
      slots: { default: '<input id="weather-api-key" />' },
      attachTo: document.body,
    })
    await flushPromises()

    expect(wrapper.get('.settings-field__state').text()).toBe('已保存至凭据管理器')
    expect(hintText()).toContain('输入新值后点保存即可替换')
  })

  it('moves the control to the right edge only for the inline variant', async () => {
    const inline = mount(SettingsField, {
      props: { label: '保留天数', labelFor: 'retention-days-box', state: '占用 24.7 MiB', inline: true },
      slots: { default: '<input id="retention-days-box" type="number" />' },
      attachTo: document.body,
    })
    await flushPromises()

    expect(inline.get('.settings-field').classes()).toContain('settings-field--inline')
    // 控件仍是根节点的直接子节点（不许被塞进标签行里，那会打乱 label 的 for 目标）
    expect(inline.get('.settings-field__head').text()).toContain('占用 24.7 MiB')
    expect(inline.get('input').attributes('id')).toBe('retention-days-box')

    const stacked = mount(SettingsField, {
      props: { label: '保留天数', labelFor: 'stacked-days' },
      slots: { default: '<input id="stacked-days" />' },
      attachTo: document.body,
    })
    await flushPromises()
    // 默认竖排：其它设置页的字段版式不许被这个变体带偏
    expect(stacked.get('.settings-field').classes()).not.toContain('settings-field--inline')
  })
})
