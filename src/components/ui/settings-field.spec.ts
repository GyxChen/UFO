// @vitest-environment happy-dom

import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import SettingsField from './SettingsField.vue'

describe('SettingsField', () => {
  it('renders a native label target inside the shared field layout', () => {
    const wrapper = mount(SettingsField, {
      props: {
        label: '代理解析器',
        labelFor: 'proxy-resolver',
        hint: '把本地代理地址反查成上游 endpoint',
      },
      slots: {
        default: '<button id="proxy-resolver">不解析</button>',
      },
    })

    expect(wrapper.get('label').attributes('for')).toBe('proxy-resolver')
    expect(wrapper.get('.settings-field__hint').text()).toContain('反查成上游')
  })

  it('supports aria-labelledby targets for custom controls', () => {
    const wrapper = mount(SettingsField, {
      props: {
        label: 'Token 单位',
        labelId: 'token-unit-label',
      },
      slots: {
        default: '<div aria-labelledby="token-unit-label">Tokens</div>',
      },
    })

    expect(wrapper.get('.settings-field__label').attributes('id')).toBe('token-unit-label')
  })
})
