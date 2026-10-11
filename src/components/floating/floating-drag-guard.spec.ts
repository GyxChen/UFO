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
 * 第二件事：拖完要把置顶补回来，而且只能在**松手且贴边落定之后**补。为什么是这个时机、为什么必须先关
 * 再开，唯一落点写在 `FloatingWindow.vue` 的 `repinTopmost()` 注释上，这里只锁调用点确实挂在那儿。
 * 补置顶走的 IPC 还要 capability 授权，漏授权同样是静默不生效。
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
  it('re-pins after the snap settles, not when the drag starts', () => {
    const snap = shell.indexOf('void snapWindow()')
    const repin = shell.indexOf('.then(repinTopmost)')

    expect(snap, '贴边重算的调用点找不到了').toBeGreaterThan(-1)
    expect(repin, '没有补置顶，或没挂在贴边重算之后').toBeGreaterThan(snap)
    // 补置顶排在 catch 之后：吸附失败那一次也得补，否则那条路径又把置顶空出来。
    expect(shell.slice(snap, repin), '补置顶被排进了只成功才走的分支').toContain('.catch(')

    // `startDragging()` 到拉起松手轮询之间 = 原生移动循环还没开始的那一段。
    const from = shell.indexOf('await window.startDragging()')
    const to = shell.indexOf('startDragReleasePolling()')
    expect(from, '拖拽启动点找不到了').toBeGreaterThan(-1)
    expect(to, '松手轮询的接线变了').toBeGreaterThan(from)
    expect(
      shell.slice(from, to),
      '补置顶跟在 startDragging() 后面 = 补在拖拽开始之前，是个空操作（b79fb65 就是这么漏的）'
    ).not.toMatch(/repinTopmost/)
  })

  it('toggles off then on, because a bare set-true is a no-op against tao flag diff', () => {
    const off = shell.indexOf('setAlwaysOnTop(false)')
    const on = shell.indexOf('setAlwaysOnTop(true)')

    expect(off, '拖完没有补置顶').toBeGreaterThan(-1)
    // ⚠️ 顺序不能反：tao 只在标志发生变化时才下发 SetWindowPos，先开后关等于没补。
    expect(on, '补置顶缺了 setAlwaysOnTop(true)').toBeGreaterThan(off)
    // 函数体里不许加 isAlwaysOnTop() 门控：它读的是 tao 的内部标志（样式位被系统清掉时仍为 true），
    // 一当门控这里就变回空操作。只查函数体，注释里那个警告名字不算。
    const body = shell.match(/function repinTopmost\(\)\s*\{[\s\S]*?\n\}/)?.[0] ?? ''
    expect(body, '补置顶被 isAlwaysOnTop() 门控了').not.toContain('isAlwaysOnTop')
  })

  it('keeps the capability that authorizes the re-pin IPC', () => {
    // 前端 IPC 走 capability 校验，没授权就是静默失败（只有 console.warn）。
    expect(capability).toContain('core:window:allow-set-always-on-top')
  })
})
