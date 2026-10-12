// @vitest-environment happy-dom

import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import SettingsField from './SettingsField.vue'

/**
 * 字段原语的版式与分工（一行 = 左读右调）：
 * - `hint` = 解释（收进问号角标，面板 Teleport 到 body）；
 * - `state` = 控件读不出的当前事实，**常驻在标题下面**（不在标题行右侧、更不许藏进 hover）；
 * - 默认横排：控件是 `.settings-field__control` 里的插槽内容，贴右、与左侧整块垂直居中；
 * - `stacked` = 复合控件变体（输入 + 按钮这类小表单），控件落到标题下面占满宽度。
 * 这几条一旦写反，表现是「读数挤回标题行顶到控件」或「复合块被压进 auto 列」——所以逐条锁住。
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
    // 标题行里有角标，正文只在展开的面板里（不常驻一行灰字）
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
    // 没有解释也没有读数时，标题行不该多出一个空角标
    expect(wrapper.find('.settings-hint').exists()).toBe(false)
  })

  it('shows the state readout below the label, not in the label row', async () => {
    const wrapper = mount(SettingsField, {
      props: {
        label: 'API Key',
        labelFor: 'sample-api-key',
        state: '已保存至凭据管理器',
        hint: '输入新值后点保存即可替换',
      },
      slots: { default: '<input id="sample-api-key" />' },
      attachTo: document.body,
    })
    await flushPromises()

    // 读数在标题**下面**那一块（`.settings-field__text`）里，标题行右侧不留读数位
    const text = wrapper.get('.settings-field__text')
    expect(text.get('.settings-field__state').text()).toBe('已保存至凭据管理器')
    expect(wrapper.get('.settings-field__head').text()).not.toContain('已保存')
    expect(hintText()).toContain('输入新值后点保存即可替换')
  })

  it('docks the control to the right edge by default, below the label only when stacked', async () => {
    const inline = mount(SettingsField, {
      props: { label: '保留天数', labelFor: 'retention-days-box', state: '占用 24.7 MiB' },
      slots: { default: '<input id="retention-days-box" type="number" />' },
      attachTo: document.body,
    })
    await flushPromises()

    // 默认横排：控件包在 `.settings-field__control` 里，是根节点的直接子节点
    //（不许塞进标题行 —— 那会打乱 label 的 for 目标与垂直居中）
    expect(inline.get('.settings-field').classes()).not.toContain('settings-field--stacked')
    const control = inline.get('.settings-field__control')
    expect(control.element.parentElement?.classList.contains('settings-field')).toBe(true)
    expect(control.get('input').attributes('id')).toBe('retention-days-box')

    const stacked = mount(SettingsField, {
      props: { label: 'API Token', labelFor: 'sample-api-token', stacked: true },
      slots: { default: '<input id="sample-api-token" />' },
      attachTo: document.body,
    })
    await flushPromises()
    // 复合控件变体：显式声明才落到标题下面，其它行不受影响
    expect(stacked.get('.settings-field').classes()).toContain('settings-field--stacked')
  })
})
