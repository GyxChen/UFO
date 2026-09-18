// @vitest-environment happy-dom

import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import BaseButton from './BaseButton.vue'
import BaseInput from './BaseInput.vue'
import BaseSegmentedControl from './BaseSegmentedControl.vue'
import BaseSelect from './BaseSelect.vue'
import BaseSlider from './BaseSlider.vue'
import BaseSwitch from './BaseSwitch.vue'

describe('BaseButton', () => {
  it('defaults to a non-submit button and disables while loading', () => {
    const wrapper = mount(BaseButton, {
      props: { loading: true },
      slots: { default: '保存' },
    })

    expect(wrapper.attributes('type')).toBe('button')
    expect(wrapper.attributes('aria-busy')).toBe('true')
    expect(wrapper.attributes('disabled')).toBeDefined()
  })
})

describe('BaseInput', () => {
  it('applies the number v-model modifier before emitting', async () => {
    const wrapper = mount(BaseInput, {
      props: {
        modelValue: '',
        modelModifiers: { number: true },
      },
    })

    await wrapper.find('input').setValue('42')

    expect(wrapper.emitted('update:modelValue')).toEqual([[42]])
  })

  it('keeps plain text values as strings', async () => {
    const wrapper = mount(BaseInput, {
      props: { modelValue: '' },
    })

    await wrapper.find('input').setValue('https://api.example.com')

    expect(wrapper.emitted('update:modelValue')).toEqual([['https://api.example.com']])
  })
})

describe('BaseSlider', () => {
  it('emits the numeric value on drag and on commit', async () => {
    const wrapper = mount(BaseSlider, { props: { modelValue: 45, min: 32, max: 96 } })
    const input = wrapper.get<HTMLInputElement>('input')

    expect(input.attributes('type')).toBe('range')
    expect(input.attributes('min')).toBe('32')
    expect(input.attributes('max')).toBe('96')

    // 拖动发 update:modelValue，松手另外发 change —— 页面按这个分工决定何时落盘。
    await input.setValue('64')
    expect(wrapper.emitted('update:modelValue')).toEqual([[64]])
    expect(wrapper.emitted('change')).toHaveLength(1)
  })

  it('exposes the filled ratio as a css variable', () => {
    const wrapper = mount(BaseSlider, { props: { modelValue: 45, min: 32, max: 96 } })
    // (45 − 32) / (96 − 32) = 20.3125%
    expect(wrapper.get('.base-slider').attributes('style')).toContain('20.3125%')
  })

  it('reports a full and an empty track at the bounds', () => {
    const atMin = mount(BaseSlider, { props: { modelValue: 32, min: 32, max: 96 } })
    expect(atMin.get('.base-slider').attributes('style')).toContain('0%')
    const atMax = mount(BaseSlider, { props: { modelValue: 96, min: 32, max: 96 } })
    expect(atMax.get('.base-slider').attributes('style')).toContain('100%')
  })
})

describe('BaseSelect', () => {
  it('opens the listbox, selects an option and exposes combobox state', async () => {
    const wrapper = mount(BaseSelect, {
      props: {
        modelValue: 'a',
        ariaLabelledby: 'resolver-label',
        options: [
          { value: 'a', label: '解析器 A' },
          { value: 'b', label: '解析器 B' },
        ],
      },
    })

    const trigger = wrapper.get('[role="combobox"]')
    expect(trigger.attributes('aria-expanded')).toBe('false')

    await trigger.trigger('click')
    expect(trigger.attributes('aria-expanded')).toBe('true')
    expect(trigger.attributes('aria-labelledby')).toBe('resolver-label')
    expect(wrapper.find('[role="listbox"]').exists()).toBe(true)

    await wrapper.findAll('[role="option"]')[1].trigger('click')

    expect(wrapper.emitted('update:modelValue')).toEqual([['b']])
    expect(wrapper.emitted('change')).toEqual([['b']])
    expect(trigger.attributes('aria-expanded')).toBe('false')
  })

  it('supports arrow navigation and Enter selection', async () => {
    const wrapper = mount(BaseSelect, {
      props: {
        modelValue: 'a',
        options: [
          { value: 'a', label: '解析器 A' },
          { value: 'b', label: '解析器 B' },
        ],
      },
    })

    const trigger = wrapper.get('[role="combobox"]')
    await trigger.trigger('keydown', { key: 'ArrowDown' })
    await trigger.trigger('keydown', { key: 'ArrowDown' })
    await trigger.trigger('keydown', { key: 'Enter' })

    expect(wrapper.emitted('update:modelValue')).toEqual([['b']])
  })
})

describe('BaseSwitch', () => {
  it('emits the next checked state and a native change event', async () => {
    const wrapper = mount(BaseSwitch, {
      props: {
        modelValue: false,
        ariaLabel: '启用状态灯',
      },
    })

    await wrapper.find('input').setValue(true)

    expect(wrapper.emitted('update:modelValue')).toEqual([[true]])
    expect(wrapper.emitted('change')).toHaveLength(1)
    expect(wrapper.find('input').attributes('aria-label')).toBe('启用状态灯')
  })
})

describe('BaseSegmentedControl', () => {
  it('switches the selected radio value and exposes the group label', async () => {
    const wrapper = mount(BaseSegmentedControl, {
      props: {
        modelValue: 'tokens',
        name: 'token-unit',
        ariaLabelledby: 'token-unit-label',
        options: [
          { value: 'tokens', label: 'Tokens' },
          { value: 'm_tokens', label: 'M tokens' },
        ],
      },
    })

    await wrapper.findAll('input')[1].setValue()

    expect(wrapper.emitted('update:modelValue')).toEqual([['m_tokens']])
    expect(wrapper.attributes('aria-labelledby')).toBe('token-unit-label')
  })
})
