import { ABS_MAX_CELLS, DEFAULT_WINDOW_CELLS, cellsGrid, clampCells, type FloatingLayout } from '@/lib/floating-layout'

/**
 * Agent 状态灯模块自己的布局模型：
 * 「会话数 + 形态」→ 上报给宿主的格子需求，以及把会话切成可见灯 + `+N` 溢出格。
 * 与模块无关的形态 / 格子换算在 `floating-layout`。
 */

/**
 * 模块的格子需求：会话数不超过默认容量时保持默认尺寸；
 * 超过后按会话数放大，夹在 [默认, 该形态上限] 之间。
 */
export function windowCellsForSessions(sessionCount: number, layoutMax: number): number {
  const max = clampCells(layoutMax, DEFAULT_WINDOW_CELLS, ABS_MAX_CELLS)
  if (sessionCount <= DEFAULT_WINDOW_CELLS) return DEFAULT_WINDOW_CELLS
  return clampCells(sessionCount, DEFAULT_WINDOW_CELLS, max)
}

/**
 * 当前形态下分到的格子能放下几盏灯（含溢出格位）。
 * - 边条：一格一盏，等于分到的格子数；
 * - 方形/四角：等于网格 side²（5 格就是 3×3），溢出格能占满最后一格不会空着。
 */
export function indicatorCapacity(layout: FloatingLayout, cells: number = ABS_MAX_CELLS): number {
  const budget = clampCells(cells, 1, ABS_MAX_CELLS)
  if (layout === 'horizontal' || layout === 'vertical') {
    return budget
  }
  const grid = cellsGrid(layout, budget)
  return grid.cols * grid.rows
}

/**
 * 按容量切分会话：放得下则全显；超出时留出最后一格给 `+N`。
 */
export function partitionIndicatorSessions<T>(
  sessions: readonly T[],
  layout: FloatingLayout,
  cells: number = DEFAULT_WINDOW_CELLS
): { visible: T[]; overflow: number } {
  const capacity = indicatorCapacity(layout, cells)
  if (sessions.length <= capacity) {
    return { visible: [...sessions], overflow: 0 }
  }
  const visibleCount = Math.max(capacity - 1, 0)
  return {
    visible: sessions.slice(0, visibleCount),
    overflow: sessions.length - visibleCount,
  }
}
