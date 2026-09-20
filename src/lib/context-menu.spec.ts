// @vitest-environment node

/**
 * 右键菜单摆放规则的护栏。
 *
 * 规则只有一条：**贴着光标，但绝不越出视口**（越界的菜单会被窗口边缘切掉一截，
 * 项都点不到）。三件事容易改回去，逐条钉住：
 * 1. 摆得下就原样贴着光标（不能一律居中 / 一律贴边）；
 * 2. 越界时夹回视口内，并留出 [`MENU_EDGE_GAP`] 的空隙；
 * 3. 菜单比视口还大（极端窄窗）时不能算出负坐标 —— 宁可右下溢出，左上角也必须在视口内。
 */
import { describe, expect, it } from 'vitest'
import { MENU_EDGE_GAP, placeContextMenu, type ContextMenuPoint } from './context-menu'

/** 一个 1000 × 800 的视口。 */
const VIEWPORT = { width: 1000, height: 800 }
/** 一个 180 × 90 的菜单。 */
const MENU = { width: 180, height: 90 }

/** 断言两个坐标相等（失败信息里带上两边的数值，好定位）。 */
function expectPoint(actual: ContextMenuPoint, expected: ContextMenuPoint) {
  expect(actual).toEqual(expected)
}

describe('placeContextMenu', () => {
  it('sticks to the cursor when the menu fits', () => {
    expectPoint(placeContextMenu({ x: 300, y: 200 }, MENU, VIEWPORT), { x: 300, y: 200 })
  })

  it('clamps back inside the viewport near the right and bottom edges', () => {
    // 光标在右下角：菜单右 / 下都会溢出，应被夹到「视口尺寸 − 菜单尺寸 − 空隙」
    expectPoint(placeContextMenu({ x: 980, y: 790 }, MENU, VIEWPORT), {
      x: VIEWPORT.width - MENU.width - MENU_EDGE_GAP,
      y: VIEWPORT.height - MENU.height - MENU_EDGE_GAP,
    })
  })

  it('keeps the edge gap when the cursor sits in the very corner', () => {
    // 0 会贴着（甚至压住）窗口边框，所以要抬到空隙处
    expectPoint(placeContextMenu({ x: 0, y: 0 }, MENU, VIEWPORT), { x: MENU_EDGE_GAP, y: MENU_EDGE_GAP })
  })

  it('never returns negative coordinates when the menu is larger than the viewport', () => {
    const huge = { width: 1400, height: 900 }
    const point = placeContextMenu({ x: 500, y: 400 }, huge, VIEWPORT)

    // 夹取的下限是空隙而不是 0：至少让左上角（菜单的第一项）留在视口里
    expectPoint(point, { x: MENU_EDGE_GAP, y: MENU_EDGE_GAP })
  })

  it('honours a custom edge gap', () => {
    expectPoint(placeContextMenu({ x: 0, y: 0 }, MENU, VIEWPORT, 24), { x: 24, y: 24 })
  })
})
