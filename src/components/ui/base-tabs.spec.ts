// @vitest-environment happy-dom

import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import BaseTabs from './BaseTabs.vue'

const tabs = [
  { key: 'floating-window', label: '悬浮窗设置' },
  { key: 'about', icon: '/gear.svg', ariaLabel: '关于', end: true },
]

describe('BaseTabs', () => {
  it('renders exactly one tablist, and only tabs live inside it', () => {
    const wrapper = mount(BaseTabs, {
      props: { tabs, modelValue: 'floating-window', idPrefix: 'main' },
      slots: { actions: '<button class="probe">theme</button>' },
    })

    const tablists = wrapper.findAll('[role="tablist"]')
    expect(tablists).toHaveLength(1)
    // tablist 的子项只能是 tab —— 混进普通按钮会让读屏的「共 N 个标签页、当前第 M 个」失准。
    expect(tablists[0].findAll('[role="tab"]')).toHaveLength(2)
  })

  it('keeps the actions slot outside the tablist', () => {
    const wrapper = mount(BaseTabs, {
      props: { tabs, modelValue: 'floating-window', idPrefix: 'main' },
      slots: { actions: '<button class="probe">theme</button>' },
    })

    const actions = wrapper.get('.main-tabs__actions')
    expect(actions.find('.probe').exists()).toBe(true)
    // 动作按钮不能落在 tablist 里（它是动作，不是标签页）。
    expect(wrapper.get('[role="tablist"]').find('.probe').exists()).toBe(false)
  })

  it('wires ids so a panel can be pointed at by aria-controls', () => {
    const wrapper = mount(BaseTabs, {
      props: { tabs, modelValue: 'floating-window', idPrefix: 'main' },
    })

    const [first, second] = wrapper.findAll('[role="tab"]')
    expect(first.attributes('id')).toBe('main-tab-floating-window')
    expect(first.attributes('aria-controls')).toBe('main-panel-floating-window')
    expect(second.attributes('id')).toBe('main-tab-about')
  })

  it('marks the icon-only tab with an accessible name and a right-aligned modifier', () => {
    const wrapper = mount(BaseTabs, {
      props: { tabs, modelValue: 'floating-window', idPrefix: 'main' },
    })

    const iconTab = wrapper.findAll('[role="tab"]')[1]
    // 只有图标、没有可见文字：读屏名称必须由 aria-label 补上。
    expect(iconTab.text()).toBe('')
    expect(iconTab.attributes('aria-label')).toBe('关于')
    expect(iconTab.find('.main-tabs__tab-icon').exists()).toBe(true)
    expect(iconTab.classes()).toContain('main-tabs__tab--end')
  })

  it('emits on click but stays quiet when re-selecting the current tab', async () => {
    const wrapper = mount(BaseTabs, {
      props: { tabs, modelValue: 'floating-window', idPrefix: 'main' },
    })

    const [first, second] = wrapper.findAll('[role="tab"]')
    await first.trigger('click')
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()

    await second.trigger('click')
    expect(wrapper.emitted('update:modelValue')).toEqual([['about']])
  })

  it('moves selection with arrow keys and skips the actions button', async () => {
    const wrapper = mount(BaseTabs, {
      props: { tabs, modelValue: 'floating-window', idPrefix: 'main' },
      slots: { actions: '<button class="probe">theme</button>' },
      attachTo: document.body,
    })

    await wrapper.findAll('[role="tab"]')[0].trigger('keydown', { key: 'ArrowRight' })
    expect(wrapper.emitted('update:modelValue')).toEqual([['about']])

    // 动作按钮是普通 button（不参与 roving tabindex），不属于方向键漫游的范围。
    expect(wrapper.get('.probe').attributes('tabindex')).toBeUndefined()
  })
})
