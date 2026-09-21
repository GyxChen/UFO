import type { FloatingLayout } from '@/lib/floating-layout'

/**
 * 待办组件的格子需求函数：恒定返回 2 格。
 *
 * 三面小旗（高 / 中 / 低各自的未完成数）本身不占地方，但「旗 + 数字」成组排布要一条更长的轴：
 * 方形 / 四角 / 横条里三组横向铺开，竖条里三行上下叠放。只占 1 格时同级共享窗口的份额太窄 ——
 * 横条那边只有一格宽（数字被降级掉，只剩三面光旗）、竖条那边只有一格高（三行挤不进一格）。
 * ⚠️「总格子数」还有个下限：窗口级汇总会把各组件需求之和夹在
 * `[DEFAULT_WINDOW_CELLS(4), 该形态上限]` 里，所以单独启用它时贴边窗口仍是 4 格；
 * 2 格主要在同其他组件同级时体现为更宽裕的份额（按需求比例切分窗口）。
 * ⚠️ 纯函数：只认形态，不读宿主分配了多少格（读 `allocatedCells` 会形成尺寸回路）。
 *
 * @param _layout 当前贴边形态（square | corner | horizontal | vertical），本组件四形态同需求
 * @returns 恒定返回 2
 */
export function todoDemand(_layout: FloatingLayout): number {
  return 2
}
