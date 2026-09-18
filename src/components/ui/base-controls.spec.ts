// @vitest-environment happy-dom

import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import BaseButton from './BaseButton.vue'
import BaseInput from './BaseInput.vue'
import BaseSegmentedControl from './BaseSegmentedControl.vue'
import BaseSelect from './BaseSelect.vue'
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

describe('BaseSelect', () => {
  it('syncs the selected value and forwards aria attributes to the native select', async () => {
    const wrapper = mount(BaseSelect, {
      props: {
        modelValue: 'a',
        'aria-labelledby': 'resolver-label',
      },
      slots: {
        default: '<option value="a">A</option><option value="b">B</option>',
      },
    })

    await wrapper.find('select').setValue('b')

    expect(wrapper.emitted('update:modelValue')).toEqual([['b']])
    expect(wrapper.find('select').attributes('aria-labelledby')).toBe('resolver-label')
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
