import type { FloatingLayout } from '@/lib/floating-modules'
import type { DockedEdges } from '@/lib/snap'

/** 悬浮窗默认格子数：角 2×2、边 4×1 / 1×N。 */
export const DEFAULT_WINDOW_CELLS = 4
/** 客户可配上限的下限（不低于默认尺寸）。 */
export const ABS_MIN_CELLS = 4
/** 客户可配上限的绝对上限。 */
export const ABS_MAX_CELLS = 8

export function layoutFromDocked(docked: DockedEdges | null): FloatingLayout {
  if (!docked) return 'square'
  const vertical = docked.top || docked.bottom
  const horizontal = docked.left || docked.right
  if (vertical && horizontal) return 'corner'
  if (vertical) return 'horizontal'
  if (horizontal) return 'vertical'
  return 'square'
}

function clampCells(value: number, min: number, max: number): number {
  return Math.min(Math.max(Math.floor(value), min), max)
}

/** 角/方形边长倍率：保持正方形，side = ceil(sqrt(cells))。 */
export function squareSideForCells(cells: number): number {
  const n = clampCells(cells, 1, ABS_MAX_CELLS)
  return Math.ceil(Math.sqrt(n))
}

/**
 * 壳应采用的物理格子数。
 * Agent 数不超过默认时保持默认尺寸；超过后再放大到 layoutMax，且不低于默认。
 */
export function windowCellsForSessions(sessionCount: number, layoutMax: number): number {
  const max = clampCells(layoutMax, ABS_MIN_CELLS, ABS_MAX_CELLS)
  if (sessionCount <= DEFAULT_WINDOW_CELLS) return DEFAULT_WINDOW_CELLS
  return clampCells(sessionCount, DEFAULT_WINDOW_CELLS, max)
}

/** 指示灯网格列行数，随贴边形态与格子数变化。 */
export function indicatorGrid(
  layout: FloatingLayout,
  cells: number = DEFAULT_WINDOW_CELLS,
): { cols: number; rows: number } {
  const n = clampCells(cells, 1, ABS_MAX_CELLS)
  switch (layout) {
    case 'horizontal':
      return { cols: n, rows: 1 }
    case 'vertical':
      return { cols: 1, rows: n }
    case 'square':
    case 'corner':
    default: {
      const side = squareSideForCells(n)
      return { cols: side, rows: side }
    }
  }
}

/**
 * 当前形态可放的指示灯格数（含溢出格位）。
 * - 边条：等于 layoutMax（与窗口格子一致）
 * - 方形/四角：等于网格 side²，这样 3×3 溢出时能占满 9 格，不会空一格
 */
export function indicatorCapacity(
  layout: FloatingLayout,
  layoutMax: number = ABS_MAX_CELLS,
  cells: number = layoutMax,
): number {
  const max = clampCells(layoutMax, ABS_MIN_CELLS, ABS_MAX_CELLS)
  if (layout === 'horizontal' || layout === 'vertical') {
    return max
  }
  const grid = indicatorGrid(layout, clampCells(cells, ABS_MIN_CELLS, ABS_MAX_CELLS))
  return grid.cols * grid.rows
}

/**
 * 按形态容量切分会话：放得下则全显；超出时留出最后一格给 +N。
 *
 * 方形/四角：未超 layoutMax 时按会话全显（3×3 可暂空）；
 * 一旦超过 layoutMax，容量改按网格 side²，让 +N 占满最后一格，避免空一格。
 */
export function partitionIndicatorSessions<T>(
  sessions: readonly T[],
  layout: FloatingLayout,
  layoutMax: number = ABS_MAX_CELLS,
  cells: number = layoutMax,
): { visible: T[]; overflow: number } {
  const max = clampCells(layoutMax, ABS_MIN_CELLS, ABS_MAX_CELLS)
  const isSquare = layout === 'square' || layout === 'corner'

  if (sessions.length <= max) {
    return { visible: [...sessions], overflow: 0 }
  }

  // 溢出：方形按网格占满；边条按 layoutMax。
  const capacity = isSquare ? indicatorCapacity(layout, max, cells) : max
  const visibleCount = Math.max(capacity - 1, 0)
  return {
    visible: sessions.slice(0, visibleCount),
    overflow: sessions.length - visibleCount,
  }
}
