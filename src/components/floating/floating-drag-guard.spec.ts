/**
 * 「按住窗口里任何地方都能拖动」的护栏。
 *
 * 拖动是外壳自己实现的：pointerdown 记起点 → 位移超 4px → `startDragging()`（`FloatingWindow.vue`）。
 * 但 `<img>` 在 Chromium / WebView2 里默认就能原生拖拽，一旦按下并移动，浏览器进入图片拖拽会话
 * 并给外壳派发 `pointercancel`，起点被清空 —— 表现就是「按住空态机器人鬼影或平台图标时窗口拖不动」。
 * 修复是 CSS（外壳一条规则，不认识任何模块 id），所以这里锁这条规则还在。
 *
 * ⚠️ `-webkit-user-drag` **不继承**：只写在 `.floating-launcher` 上等于没写，必须带上后代选择器。
 *
 * 第二件事：拖完必须把置顶补回来。`startDragging()` 走 Windows 的标题栏移动循环，循环收尾时系统
 * 会清掉 `WS_EX_TOPMOST`（实测拖一次掉一次），而补置顶要先 `false` 再 `true` 才有效（tao 按标志差异
 * 下发样式），并且这条 IPC 需要 capability 授权 —— 三处任一漏掉都是「静默不生效」。
 */
import { describe, expect, it } from 'vitest'
import css from '../../assets/main.css?raw'
import capability from '../../../src-tauri/capabilities/default.json?raw'
import shell from './FloatingWindow.vue?raw'

/** 那条护栏规则的选择器串（父 + 后代）。 */
const GUARD_SELECTOR = String.raw`\.floating-launcher\s*,\s*\.floating-launcher\s+\*`

describe('悬浮外壳拖动手势护栏', () => {
  it('turns off native drag and text selection for everything under the launcher', () => {
    expect(css, '缺少 .floating-launcher + 后代的护栏规则').toMatch(new RegExp(`${GUARD_SELECTOR}\\s*\\{`))

    const block = css.match(new RegExp(`${GUARD_SELECTOR}\\s*\\{([^}]*)\\}`))?.[1] ?? ''
    expect(block).toContain('-webkit-user-drag: none')
    expect(block).toContain('user-select: none')
  })
})

describe('拖完补置顶的护栏', () => {
  it('re-pins topmost after the drag loop, toggling off then on', () => {
    const drag = shell.indexOf('startDragging()')
    const off = shell.indexOf('setAlwaysOnTop(false)')
    const on = shell.indexOf('setAlwaysOnTop(true)')

    expect(drag).toBeGreaterThan(-1)
    expect(off, '拖完没有补置顶').toBeGreaterThan(drag)
    // ⚠️ 顺序不能反：tao 只在标志发生变化时才下发 SetWindowPos，先开后关等于没补。
    expect(on, '补置顶缺了 setAlwaysOnTop(true)').toBeGreaterThan(off)
  })

  it('keeps the capability that authorizes the re-pin IPC', () => {
    // 前端 IPC 走 capability 校验，没授权就是静默失败（只有 console.warn）。
    expect(capability).toContain('core:window:allow-set-always-on-top')
  })
})
