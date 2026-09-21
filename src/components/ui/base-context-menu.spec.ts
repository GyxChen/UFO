// @vitest-environment happy-dom

/**
 * 通用右键菜单的护栏。菜单是**受控组件**（`open` 由调用方给），所以这里同时锁两件事：
 * 发不发事件、以及发完之后全局状态干不干净。
 *
 * 重点在「点空白处关闭」这一条 —— 它有两个半场，只做一半就是 bug：
 * 1. **关菜单**（显然的那半）；
 * 2. **不吃掉这一次手势**：pointerdown 之后浏览器还会照常派发 click，
 *    不拦的话这次点击会落到菜单底下刚露出来的按钮上（「关闭菜单」顺带点了别的）。
 *    所以这里既断言「底层元素收到了 pointerdown」，也断言「它没收到 click」。
 */
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { nextTick } from 'vue'
import { afterEach, describe, expect, it, vi } from 'vitest'
import BaseContextMenu from './BaseContextMenu.vue'
import type { ContextMenuItem } from '@/lib/context-menu'

/** 三项：一项置灰、一项普通、一项破坏性（顺带覆盖置灰与危险色两条渲染分支）。 */
const ITEMS: ContextMenuItem[] = [
  { id: 'postpone', label: '顺延到今天', disabled: true },
  { id: 'rename', label: '重命名' },
  { id: 'delete', label: '删除目标', tone: 'danger' },
]

/** 挂一个菜单；默认是打开状态。 */
async function mountMenu(open = true, items: ContextMenuItem[] = ITEMS) {
  const wrapper = mount(BaseContextMenu, {
    attachTo: document.body,
    props: { open, x: 120, y: 80, items, ariaLabel: '目标操作' },
  })
  await flushPromises()
  return wrapper
}

/** 取菜单面板（组件用 Teleport 挂到 body，wrapper.find 找不到）。 */
function panel(): HTMLElement {
  const el = document.querySelector<HTMLElement>('.base-context-menu')
  expect(el, '菜单面板没渲染出来').not.toBeNull()
  return el as HTMLElement
}

/** 取菜单里的项。 */
function items(): HTMLElement[] {
  return [...panel().querySelectorAll<HTMLElement>('.base-context-menu__item')]
}

/**
 * 造一个「菜单底下的按钮」：记录它收到的 pointerdown / click。
 *
 * @returns 元素本身与两个 spy
 */
function makeUnderlyingButton() {
  const button = document.createElement('button')
  const onPointerDown = vi.fn()
  const onClick = vi.fn()
  button.addEventListener('pointerdown', onPointerDown)
  button.addEventListener('click', onClick)
  document.body.appendChild(button)
  return { button, onPointerDown, onClick }
}

/** 派发一次左键 pointerdown，返回事件本身（好断言 defaultPrevented）。 */
function pressPointerDown(target: EventTarget, button = 0): PointerEvent {
  const event = new PointerEvent('pointerdown', { bubbles: true, cancelable: true, button })
  target.dispatchEvent(event)
  return event
}

/**
 * 派发一次**指针**点出来的 click。
 *
 * ⚠️ `detail` 必须是 1：浏览器给指针 click 的 `detail` 是点击次数，而键盘敲出来的 click
 * 是 0 —— 组件正是靠这个值区分两者（见 `swallowEvent`）。这里不写就成了「键盘 click」，
 * 会被放行，测出来的是假阳性。
 */
function pressClick(target: EventTarget): void {
  target.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 1 }))
}

/**
 * 在面板上按一个键。
 *
 * ⚠️ 要等一次 DOM 更新：`activeIndex` 变的是状态，高亮 class 是下一轮渲染才落到 DOM 上的。
 *
 * @param key 键名（如 `ArrowDown` / `Enter`）
 */
async function pressKey(key: string): Promise<void> {
  panel().dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
  await nextTick()
}

/** 某一项此刻是不是键盘高亮项。 */
function isActive(index: number): boolean {
  return items()[index].classList.contains('base-context-menu__item--active')
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('BaseContextMenu · 渲染与选择', () => {
  it('renders nothing while closed', () => {
    const wrapper = mount(BaseContextMenu, {
      props: { open: false, x: 0, y: 0, items: ITEMS },
    })

    expect(document.querySelector('.base-context-menu')).toBeNull()
    expect(wrapper.find('.base-context-menu').exists()).toBe(false)
  })

  it('renders the items with menu semantics and marks the disabled one', async () => {
    await mountMenu()

    const root = panel()
    expect(root.getAttribute('role')).toBe('menu')
    expect(root.getAttribute('aria-label')).toBe('目标操作')
    expect(items().map(item => item.textContent?.trim())).toEqual(['顺延到今天', '重命名', '删除目标'])

    // 置灰项用原生 disabled：键盘与读屏都据此跳过它
    expect(items()[0].hasAttribute('disabled')).toBe(true)
    expect(items()[2].classList.contains('base-context-menu__item--danger')).toBe(true)
  })

  it('emits select then closes, and ignores the disabled item', async () => {
    const wrapper = await mountMenu()

    items()[0].click()
    expect(wrapper.emitted('select')).toBeUndefined()

    items()[1].click()
    // ⚠️ 顺序：先 select 再关闭 —— 调用方要在 select 里读到「点的是哪一条」
    expect(wrapper.emitted('select')).toEqual([['rename']])
    expect(wrapper.emitted('update:open')).toEqual([[false]])
  })
})

describe('BaseContextMenu · 点空白处关闭且不误触底下的 UI', () => {
  it('closes on an outside press, swallows the gesture and does not touch the UI underneath', async () => {
    const wrapper = await mountMenu()
    const { button, onPointerDown, onClick } = makeUnderlyingButton()

    const down = pressPointerDown(button)

    // 关菜单
    expect(wrapper.emitted('update:open')).toEqual([[false]])
    // 这一次按下没到目标元素，默认行为（聚焦 / 起选）也被挡掉
    expect(onPointerDown).not.toHaveBeenCalled()
    expect(down.defaultPrevented).toBe(true)

    // 紧接着的 click 同样被吃掉：否则「点空白处」会顺带按下底下的按钮
    pressClick(button)
    expect(onClick).not.toHaveBeenCalled()

    // 反证：没有菜单拦着时，这个按钮本来是能点的
    wrapper.unmount()
    pressClick(button)
    expect(onClick).toHaveBeenCalledTimes(1)
  })

  it('leaves clicks inside the menu alone', async () => {
    const wrapper = await mountMenu()

    pressPointerDown(items()[1])
    expect(wrapper.emitted('update:open')).toBeUndefined()
  })

  it('leaves keyboard-generated clicks alone', async () => {
    const wrapper = await mountMenu()
    const { button, onClick } = makeUnderlyingButton()

    // 先把拦截挂上（模拟「上一次指针手势没等到 click 就结束了」）
    pressPointerDown(button)

    // 键盘按出来的 click（Enter / Space，detail 为 0）不是那一次手势的后半程：不该被吃掉
    button.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 0 }))

    expect(onClick).toHaveBeenCalledTimes(1)
    expect(wrapper.emitted('update:open')).toEqual([[false]])
  })

  it('closes on an outside right-click but lets the row underneath open its own menu', async () => {
    const wrapper = await mountMenu()

    const row = document.createElement('div')
    const onRowContextMenu = vi.fn((event: Event) => event.preventDefault())
    row.addEventListener('contextmenu', onRowContextMenu)
    document.body.appendChild(row)

    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
    row.dispatchEvent(event)

    expect(wrapper.emitted('update:open')).toEqual([[false]])
    // 不拦这一次右键：底下的行要能照常弹自己的菜单（一次右键换菜单，而不是两下）
    expect(onRowContextMenu).toHaveBeenCalledTimes(1)
  })

  it('closes on wheel and on window resize, but not when the wheel is inside the menu', async () => {
    const wrapper = await mountMenu()

    panel().dispatchEvent(new WheelEvent('wheel', { bubbles: true }))
    expect(wrapper.emitted('update:open')).toBeUndefined()

    document.body.dispatchEvent(new WheelEvent('wheel', { bubbles: true }))
    expect(wrapper.emitted('update:open')).toEqual([[false]])

    await wrapper.setProps({ open: true })
    await flushPromises()
    window.dispatchEvent(new Event('resize'))
    expect(wrapper.emitted('update:open')).toEqual([[false], [false]])
  })
})

describe('BaseContextMenu · 键盘与焦点', () => {
  it('closes on Escape and returns focus to where it was', async () => {
    const trigger = document.createElement('button')
    document.body.appendChild(trigger)
    trigger.focus()
    expect(document.activeElement).toBe(trigger)

    const wrapper = await mountMenu()
    // 打开时焦点收到菜单上，键盘才用得了
    expect(document.activeElement).toBe(panel())

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))

    expect(wrapper.emitted('update:open')).toEqual([[false]])

    // 调用方据此把 open 置回 false：焦点要还给打开菜单前那个元素，键盘用户才接得上
    await wrapper.setProps({ open: false })
    await nextTick()
    expect(document.activeElement).toBe(trigger)
  })

  it('moves the highlight with the arrow keys, skipping disabled items', async () => {
    await mountMenu()

    // 打开时高亮落在第一个**可点**的项（第一项置灰，应跳过）
    expect(isActive(1)).toBe(true)

    await pressKey('ArrowDown')
    expect(isActive(2)).toBe(true)

    // 循环回第一项可点的那一项（不是置灰的 0）
    await pressKey('ArrowDown')
    expect(isActive(1)).toBe(true)

    await pressKey('ArrowUp')
    expect(isActive(2)).toBe(true)

    await pressKey('Home')
    expect(isActive(1)).toBe(true)

    await pressKey('End')
    expect(isActive(2)).toBe(true)
  })

  it('selects the highlighted item with Enter', async () => {
    const wrapper = await mountMenu()

    await pressKey('ArrowDown')
    await pressKey('Enter')

    expect(wrapper.emitted('select')).toEqual([['delete']])
  })
})

describe('BaseContextMenu · 定位', () => {
  it('anchors the panel at the cursor position', async () => {
    await mountMenu()

    // 测试环境没有排版（offsetWidth 为 0），夹取不会生效，落点就是光标处
    expect(panel().style.left).toBe('120px')
    expect(panel().style.top).toBe('80px')
  })

  it('re-anchors when the caller opens it at a new cursor position', async () => {
    const wrapper: VueWrapper = await mountMenu()

    await wrapper.setProps({ x: 400, y: 300 })
    await flushPromises()

    expect(panel().style.left).toBe('400px')
    expect(panel().style.top).toBe('300px')
  })
})
