import type { DockedEdges } from './snap'

/**
 * 悬浮窗形态，由贴边状态推出，窗口尺寸随之变化：
 * - `square`：未贴边，默认 45×45（整窗就一格）；
 * - `corner`：贴到四角，放大成方形（边长 = ceil(sqrt(格子数))）；
 * - `horizontal`：上下贴边，横条（宽 = 单格宽 × 格子数）；
 * - `vertical`：左右贴边，竖条（高 = 单格高 × 格子数）。
 *
 * 所有模块都必须同时适配这四种形态，布局不得写死像素。
 */
export type FloatingLayout = 'square' | 'corner' | 'horizontal' | 'vertical'

/** 悬浮窗默认格子数：等于 2×2 的方形容量 / 4×1 的边条容量。 */
export const DEFAULT_WINDOW_CELLS = 4
/** 用户可配格子上限的下限：不低于默认尺寸。 */
export const ABS_MIN_CELLS = 4
/** 用户可配格子上限的绝对上限。 */
export const ABS_MAX_CELLS = 8

/** 把任意数值取整并夹进 [min, max]。 */
export function clampCells(value: number, min: number, max: number): number {
  return Math.min(Math.max(Math.floor(value), min), max)
}

/** 由贴边状态推出悬浮窗形态；四边都未贴边（含未知状态）时为方形。 */
export function layoutFromDocked(docked: DockedEdges | null): FloatingLayout {
  if (!docked) return 'square'
  const vertical = docked.top || docked.bottom
  const horizontal = docked.left || docked.right
  if (vertical && horizontal) return 'corner'
  if (vertical) return 'horizontal'
  if (horizontal) return 'vertical'
  return 'square'
}

/**
 * 一格边长的**默认值**（物理像素）：与 tauri.conf.json 里 floating 窗口的 45×45 初值一致。
 * ⚠️ 改这个数必须同步改 tauri.conf.json 的窗口初值，否则启动瞬间的尺寸与吸附口径对不上。
 * ⚠️ 它只是「用户没配过」时的兜底；实际用哪一格见 `configCellPx()`（配置里的 `cell_px` 优先）。
 */
export const FLOATING_BASE_CELL_PX = 45

/** 一格边长的可选下限（物理像素）：再小灯点与图标就糊成一团了。 */
export const MIN_CELL_PX = 32
/** 一格边长的可选上限（物理像素）：8 格的横条按它算最长 768px，再大就撑满半屏。 */
export const MAX_CELL_PX = 96

/** 把任意数值取整并夹进 [MIN_CELL_PX, MAX_CELL_PX]；非有限值回落默认格边长。 */
export function clampCellPx(value: number): number {
  if (!Number.isFinite(value)) return FLOATING_BASE_CELL_PX
  return Math.min(Math.max(Math.round(value), MIN_CELL_PX), MAX_CELL_PX)
}

/** 一格的像素尺寸（宽高分开，便于将来非正方形基础格）。 */
export interface FloatingCellSize {
  width: number
  height: number
}

/** 一格在窗口里的默认像素尺寸。 */
export const FLOATING_CELL_SIZE: FloatingCellSize = {
  width: FLOATING_BASE_CELL_PX,
  height: FLOATING_BASE_CELL_PX,
}

/**
 * 把「一格多少像素」包成宽高相同的 `FloatingCellSize`（尺寸换算与预览都吃这个结构），
 * 顺便夹取范围 —— 调用方不需要自己 clamp。
 */
export function cellSizeOf(px: number): FloatingCellSize {
  const side = clampCellPx(px)
  return { width: side, height: side }
}

/**
 * 某形态 + 格子数下的悬浮窗像素尺寸（贴边尺寸与主窗口预览共用这一处口径）：
 * - 方形（未贴边）恒为 1×1 格，与格子数无关；
 * - 四角保持正方形，边长 = ceil(√cells) 格；
 * - 横条 cells×1 格（上下贴边）；
 * - 竖条 1×cells 格（左右贴边）。
 */
export function floatingWindowSize(
  layout: FloatingLayout,
  cells: number,
  cell: FloatingCellSize = FLOATING_CELL_SIZE
): FloatingCellSize {
  const n = clampCells(cells, 1, ABS_MAX_CELLS)
  switch (layout) {
    case 'horizontal':
      return { width: cell.width * n, height: cell.height }
    case 'vertical':
      return { width: cell.width, height: cell.height * n }
    case 'corner': {
      const side = squareSideForCells(n)
      return { width: cell.width * side, height: cell.height * side }
    }
    case 'square':
    default:
      return { width: cell.width, height: cell.height }
  }
}

/** 角/方形的边长倍率：保持正方形，side = ceil(sqrt(cells))。 */
export function squareSideForCells(cells: number): number {
  const n = clampCells(cells, 1, ABS_MAX_CELLS)
  return Math.ceil(Math.sqrt(n))
}

/** 网格列行数：横条 1 行、竖条 1 列、方形/四角为 side × side。 */
export function cellsGrid(
  layout: FloatingLayout,
  cells: number = DEFAULT_WINDOW_CELLS
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
