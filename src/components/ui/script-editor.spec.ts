// @vitest-environment happy-dom

import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { afterEach, describe, expect, it } from 'vitest'
import ScriptEditor from './ScriptEditor.vue'

afterEach(() => {
  document.body.innerHTML = ''
})

/**
 * 摆出主窗的浮层宿主（`MainWindow.vue` 里的 `#main-content`）。
 *
 * 整窗弹层 `Teleport` 到它（不是 `body`），目标不存在时 Teleport 会直接抛错 ——
 * 所以这条既是测试夹具，也是「宿主改名/挪走就立刻红」的护栏。
 */
function appendOverlayHost(): HTMLElement {
  const host = document.createElement('div')
  host.id = 'main-content'
  host.className = 'main-content'
  document.body.append(host)
  return host
}

describe('ScriptEditor', () => {
  it('opens the full-window editor and emits inline changes', async () => {
    const host = appendOverlayHost()
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
    // 弹层必须落在宿主里（挂在 body 上时它按视口铺，会盖住标题栏并伸出窗口圆角）。
    expect(modal?.parentElement?.id, '整窗弹层的父节点必须是 #main-content').toBe('main-content')
    expect(host.contains(modal)).toBe(true)
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
