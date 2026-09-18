// @vitest-environment happy-dom

import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { afterEach, describe, expect, it } from 'vitest'
import ScriptEditor from './ScriptEditor.vue'

afterEach(() => {
  document.body.innerHTML = ''
})

describe('ScriptEditor', () => {
  it('opens the full-window editor and emits inline changes', async () => {
    const wrapper = mount(ScriptEditor, {
      attachTo: document.body,
      props: {
        modelValue: 'return 1',
        title: '编辑脚本 · https://api.example.com',
      },
    })

    expect(wrapper.find('.jse__input').exists()).toBe(true)

    await wrapper.get('.jse__expand').trigger('click')
    await nextTick()

    const modal = document.querySelector('.jse-modal')
    expect(modal).not.toBeNull()
    expect(modal?.querySelector('.jse-modal__title')?.textContent).toContain('api.example.com')

    const textarea = modal?.querySelector<HTMLTextAreaElement>('.jse__input')
    expect(textarea?.value).toBe('return 1')
    if (textarea) {
      textarea.value = 'return 2'
      textarea.dispatchEvent(new Event('input', { bubbles: true }))
    }
    await nextTick()

    expect(wrapper.emitted('update:modelValue')).toEqual([['return 2']])
    ;(modal?.querySelector('button') as HTMLButtonElement | null)?.click()
    await nextTick()

    expect(document.querySelector('.jse-modal')).toBeNull()
    wrapper.unmount()
  })
})
