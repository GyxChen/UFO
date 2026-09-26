// @vitest-environment happy-dom

/**
 * 设置页说明角标的护栏。
 *
 * 重点是「收放」这一对语义：面板**始终挂在 body 上**、靠 class 切可见性 —— 因为
 * 1. `aria-describedby` 必须能解析到一个存在的节点，读屏用户聚焦到图标就听得到全文；
 * 2. 隐藏走 `visibility: hidden`（盒子还在，能量尺寸）而不是 `display: none`（量出来是 0），
 *    位置才能一次算准，不必「先显示再量再挪」那种闪动写法。
 * 所以这里断言的是 class 与 aria 状态，不是 CSS 计算值（happy-dom 不注入 scoped 样式）。
 *
 * 延时用真实时钟等：组件的到点展开是 `setTimeout`，但 Vue 的渲染队列靠微任务，
 * 把定时器整个 fake 掉容易连 `flushPromises` 一起冻住，得不偿失。
 */
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import SettingsHint from './SettingsHint.vue'

/** 悬停到点 / 离开到点收起的延时，与组件里的 `OPEN_DELAY_MS` / `CLOSE_DELAY_MS` 一致。 */
const DELAY = 120

/** 等过延时并让渲染跑完。 */
async function waitMs(ms = DELAY + 20): Promise<void> {
  await new Promise(resolve => setTimeout(resolve, ms))
  await flushPromises()
}

/** 面板在 body 上（Teleport 出去了，`wrapper.find` 够不到），取法同 `base-context-menu.spec.ts`。 */
function panel(): HTMLElement {
  const el = document.querySelector<HTMLElement>('.settings-hint__pop')
  expect(el, '说明面板没挂到 body 上').not.toBeNull()
  return el as HTMLElement
}

/** 展开与否（收起态只靠这个 class + aria-hidden 表达）。 */
function isOpen(): boolean {
  return panel().classList.contains('settings-hint__pop--open')
}

/** 挂一个角标到 body；默认带正文。 */
async function mountHint(text = '绿灯超过该时长无新事件会被移除'): Promise<VueWrapper> {
  const wrapper = mount(SettingsHint, { props: { text }, attachTo: document.body })
  await flushPromises()
  return wrapper
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('SettingsHint', () => {
  it('renders a focusable question trigger wired to the teleported description', async () => {
    const wrapper = await mountHint()
    const trigger = wrapper.get('button')

    expect(trigger.attributes('type')).toBe('button')
    expect(trigger.attributes('aria-label')).toBe('说明')
    expect(trigger.attributes('aria-expanded')).toBe('false')
    // 图标没有可见文字：名称与描述引用都必须齐，否则键盘用户一无所知
    expect(trigger.text()).toBe('')
    expect(trigger.attributes('aria-describedby')).toBe(panel().id)
  })

  it('starts collapsed and only carries the text in the DOM', async () => {
    await mountHint()
    const pop = panel()

    expect(isOpen()).toBe(false)
    expect(pop.getAttribute('aria-hidden')).toBe('true')
    // 文案常驻 DOM（读屏靠它取描述），但页面上看不见
    expect(pop.textContent).toContain('无新事件会被移除')
  })

  it('opens on hover only after a short delay, so sweeping the mouse flashes nothing', async () => {
    const wrapper = await mountHint()
    await wrapper.get('button').trigger('pointerenter')
    expect(isOpen()).toBe(false)

    await waitMs()
    expect(isOpen()).toBe(true)
    expect(panel().getAttribute('aria-hidden')).toBe('false')
    expect(wrapper.get('button').attributes('aria-expanded')).toBe('true')
  })

  it('opens immediately on keyboard focus', async () => {
    const wrapper = await mountHint()
    await wrapper.get('button').trigger('focus')
    expect(isOpen()).toBe(true)
  })

  it('stays open when the pointer comes back before the delayed close lands', async () => {
    const wrapper = await mountHint()
    const trigger = wrapper.get('button')
    await trigger.trigger('pointerenter')
    await waitMs()

    await trigger.trigger('pointerleave')
    await trigger.trigger('pointerenter')
    await waitMs()
    expect(isOpen()).toBe(true)
  })

  it('stays open while the pointer moves from the trigger onto the panel', async () => {
    const wrapper = await mountHint()
    await wrapper.get('button').trigger('pointerenter')
    await waitMs()

    await wrapper.get('button').trigger('pointerleave')
    panel().dispatchEvent(new PointerEvent('pointerenter', { bubbles: true }))
    await waitMs()
    expect(isOpen()).toBe(true)

    // 真的离开面板才收
    panel().dispatchEvent(new PointerEvent('pointerleave', { bubbles: true }))
    await waitMs()
    expect(isOpen()).toBe(false)
  })

  it('toggles from a click for input devices without hover', async () => {
    const wrapper = await mountHint()
    const trigger = wrapper.get('button')

    await trigger.trigger('click')
    expect(isOpen()).toBe(true)
    await trigger.trigger('click')
    expect(isOpen()).toBe(false)
  })

  it('closes on Escape and keeps focus on the trigger', async () => {
    const wrapper = await mountHint()
    const button = wrapper.get('button').element
    button.focus()
    await flushPromises()
    expect(isOpen()).toBe(true)

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await flushPromises()

    expect(isOpen()).toBe(false)
    expect(document.activeElement).toBe(button)
  })

  it('closes on a pointer landing outside, but not on one inside the panel', async () => {
    const wrapper = await mountHint()
    await wrapper.get('button').trigger('focus')

    panel().dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }))
    await flushPromises()
    expect(isOpen()).toBe(true)

    // 真实点击的目标总是某个元素（捕获阶段的监听因此会先跑）；class 要等一轮渲染才落到 DOM
    document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }))
    await flushPromises()
    expect(isOpen()).toBe(false)
  })

  it('closes when anything scrolls, because a fixed panel would be left behind', async () => {
    const wrapper = await mountHint()
    await wrapper.get('button').trigger('focus')

    document.dispatchEvent(new Event('scroll', { bubbles: true }))
    await flushPromises()
    expect(isOpen()).toBe(false)
  })

  it('renders nothing when there is no text to explain', async () => {
    const wrapper = mount(SettingsHint, { props: { text: '' }, attachTo: document.body })
    await flushPromises()

    expect(wrapper.find('button').exists()).toBe(false)
    expect(document.querySelector('.settings-hint__pop')).toBeNull()
  })

  it('leaves the panel on body so no card can clip it', async () => {
    // 主窗从 `.main-shell` 到各分区面板多层 `overflow: hidden` / 内滚：
    // 面板只要留在字段里面就会被裁掉（这类坑本项目已经咬过几次），所以它必须是 body 的直接子节点。
    const wrapper = await mountHint()

    expect(panel().parentElement).toBe(document.body)
    expect(wrapper.element.contains(panel())).toBe(false)
  })
})
