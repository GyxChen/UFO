import type { FloatingLayout } from '@/lib/floating-layout'

/**
 * 待办组件的格子需求函数：恒定返回 2 格。
 *
 * 为什么要 2 格而不是 1 格：徽标（图标 + 未完成数）之外还要放下最近一条任务，
 * 贴边成横条 / 竖条时才有展开的余地。方形（未贴边）固定 1 格、尺寸与格子数无关，
 * 所以 2 格只在贴角 / 贴边时体现为更大的窗口，不会把默认窗口撑大。
 *
 * ⚠️ 纯函数：只认形态，不读宿主分配了多少格（读 `allocatedCells` 会形成尺寸回路）。
 *
 * @param _layout 当前贴边形态（square | corner | horizontal | vertical），本组件四形态同需求
 * @returns 恒定返回 2
 */
export function todoDemand(_layout: FloatingLayout): number {
  return 2
}
