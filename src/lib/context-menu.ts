/**
 * 右键菜单的纯逻辑：菜单项的对外类型 + 「摆在哪儿」的计算。
 *
 * 拆出来是因为摆放规则是一条与 DOM 无关的几何规则，值得单测锁住；
 * 组件（`components/ui/BaseContextMenu.vue`）只负责把结果写进 style。
 */

/** 右键菜单里的一项。 */
export interface ContextMenuItem {
  /** 项 id：点选后原样回传给调用方（菜单本身不碰业务，不知道 id 是什么意思）。 */
  id: string
  /** 显示文案。 */
  label: string
  /**
   * 置灰不可点。
   * ⚠️ 保留一项置灰项、而不是干脆不渲染：用户能看出「是这一条不适用」，
   * 而不是「菜单坏了 / 少了一项」。
   */
  disabled?: boolean
  /** 语义色调：`danger` = 破坏性操作（红字，并与上面的普通项用分隔线隔开）。 */
  tone?: 'default' | 'danger'
}

/** 菜单贴着视口边缘时至少留出的空隙（px）。 */
export const MENU_EDGE_GAP = 8

/** 视口坐标（光标位置 / 菜单落点）。 */
export interface ContextMenuPoint {
  /** 距视口左边的像素 */
  x: number
  /** 距视口顶边的像素 */
  y: number
}

/** 一个矩形尺寸。 */
export interface ContextMenuSize {
  /** 宽 */
  width: number
  /** 高 */
  height: number
}

/**
 * 算出菜单该摆在哪儿：优先贴着光标，越出视口就夹回来。
 *
 * 用「夹取」而不是「翻到光标另一侧」的理由：菜单最宽也就一两百像素，夹回来之后光标
 * 依旧落在菜单上，观感与原生菜单的翻转几乎一样，规则却简单得多 —— 一条 clamp 就够，
 * 不必再为「翻到左侧之后又越界」写第二轮判断。
 *
 * @param anchor 光标位置（视口坐标，即 `clientX` / `clientY`）
 * @param menu 菜单的实际渲染尺寸
 * @param viewport 视口尺寸（`window.innerWidth` / `innerHeight`）
 * @param gap 与视口边缘的最小空隙，默认 [`MENU_EDGE_GAP`]
 * @returns 菜单左上角的视口坐标
 */
export function placeContextMenu(
  anchor: ContextMenuPoint,
  menu: ContextMenuSize,
  viewport: ContextMenuSize,
  gap: number = MENU_EDGE_GAP
): ContextMenuPoint {
  /*
   * 菜单比视口还大时（极端窄窗）max 会小于 gap，这里再夹一次下限：
   * 宁可右下溢出，也要保证左上角在视口内（配合组件的 max-height + 内部滚动）。
   */
  const maxX = Math.max(gap, viewport.width - menu.width - gap)
  const maxY = Math.max(gap, viewport.height - menu.height - gap)
  return {
    x: Math.min(Math.max(anchor.x, gap), maxX),
    y: Math.min(Math.max(anchor.y, gap), maxY),
  }
}
