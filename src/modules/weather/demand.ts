import type { FloatingLayout } from '@/lib/floating-layout'

/**
 * 和风天气组件的格子需求函数：恒定返回 2 格占用。
 *
 * 宿主在未挂载组件时即可依此汇总窗口尺寸，并在三形态预览中精确分槽。
 * 无论悬浮窗处于方形(square)、四角(corner)、横条(horizontal)还是竖条(vertical)，均占用 2 个单元格。
 *
 * @param _layout 当前贴边形态（square | corner | horizontal | vertical）
 * @returns 恒定返回 2
 */
export function weatherDemand(_layout: FloatingLayout): number {
  return 2
}
