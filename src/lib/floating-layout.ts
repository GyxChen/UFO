import type { DockedEdges } from './snap'

/**
 * 悬浮窗形态，由贴边状态推出，窗口尺寸随之变化：
 * - `square`：未贴边，默认 45×45（整窗就一格）；
 * - `corner`：贴到四角，各模块竖向堆叠成「N 行 × 最宽模块列数」（见 {@link windowGrid}）；
 * - `horizontal`：上下贴边，横条（宽 = 单格宽 × 格子数）；
 * - `vertical`：左右贴边，竖条（高 = 单格高 × 格子数）。
 *
 * 所有模块都必须同时适配这四种形态，布局不得写死像素。
 */
export type FloatingLayout = 'square' | 'corner' | 'horizontal' | 'vertical'

/**
 * 悬浮窗默认格子数：宿主算出真实格子数前的兜底（= 2×2 方形容量）。
 * ⚠️ 窗口格子的**地板**在 `resolveWindowCells` 用 {@link ABS_MIN_CELLS}(1)，不在这里 ——
 * 否则边条按会话个数缩到 1 格会被抬回默认值。「贴角最少 1×2」由模块自己的容量函数保证。
 */
export const DEFAULT_WINDOW_CELLS = 4
/** 格子数的绝对下限：1 格（边条按会话个数可缩到 1）。 */
export const ABS_MIN_CELLS = 1
/** 格子数的绝对上限：阶梯最大档 4×4 = 16 格（窗口要能容下「Agent 3×3 + 若干同级组件」）。 */
export const ABS_MAX_CELLS = 16

/**
 * 悬浮窗格子数的**离散阶梯**（升序）：窗口按需求总和在这些档位间伸缩，
 * 分别对应方形 / 贴角网格 1×1 / 1×2 / 1×3 / 2×2 / 2×3 / 3×3 / 3×4 / 4×4（行×列）。
 * 边条（横条 / 竖条）不走阶梯，按个数精确取长度（见 `cellsGrid`）。
 *
 * ⚠️ 单个模块（如 Agent 状态灯）自己的上限只到 3×3（9）—— 更大的档位是给
 * 「一个模块吃满 3×3、再叠加同级组件」的整窗预留的，模块自己的滑块不暴露它们。
 */
export const CELL_STEPS: readonly number[] = [1, 2, 3, 4, 6, 9, 12, 16]

/** 每个档位对应的方形 / 贴角网格（行×列）。 */
const GRID_BY_STEP: Record<number, { rows: number; cols: number }> = {
  1: { rows: 1, cols: 1 },
  2: { rows: 1, cols: 2 },
  3: { rows: 1, cols: 3 },
  4: { rows: 2, cols: 2 },
  6: { rows: 2, cols: 3 },
  9: { rows: 3, cols: 3 },
  12: { rows: 3, cols: 4 },
  16: { rows: 4, cols: 4 },
}

/**
 * 把任意格子数向上夹到最近的阶梯档位：≥ cells 的最小档，封顶 {@link ABS_MAX_CELLS}、最低 1 档。
 * 非有限值按 1 档。供方形 / 贴角把任意格子数归一到有效网格（贴角因此是非正方形，如 6 格 = 2 行 3 列）。
 */
export function snapToStep(cells: number): number {
  const n = Number.isFinite(cells) ? Math.floor(cells) : 1
  for (const step of CELL_STEPS) {
    if (step >= n) return step
  }
  return ABS_MAX_CELLS
}

/** 取某个格子数（向上归档后）对应的方形 / 贴角网格；理论上一定有档，兜底 1×1 防越界。 */
function stepGrid(cells: number): { rows: number; cols: number } {
  return GRID_BY_STEP[snapToStep(cells)] ?? { rows: 1, cols: 1 }
}

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
 * 单个模块的格子权重：至少 1 格；非有限值（NaN / Infinity）按 1 格算。
 * 与外壳汇总（`moduleCellWeight`）同口径，这里内联一份以避免 floating-layout
 * 反向依赖 floating-modules（会形成循环依赖）。
 */
function cellWeight(demand: number): number {
  return Number.isFinite(demand) ? Math.max(1, Math.floor(demand)) : 1
}

/**
 * 某形态下、给定**各模块格子需求**时的窗口网格（行×列）。
 *
 * 宿主分槽、贴边尺寸、三形态预览共用这一份口径：
 * - 横条：1 行，列数 = 各模块需求之和（模块左右并排，各占自己需求那么宽）；
 * - 竖条：行数 = 需求之和，1 列（模块上下堆叠，各占自己需求那么高）；
 * - 方形（未贴边）：恒 1×1，与需求无关；
 * - 四角：每个模块按自己的需求归一到 corner 网格后**竖向堆叠** ——
 *   行数 = 各模块行数之和，列数 = 各模块列数的最大值。最宽的模块决定窗宽，
 *   其余模块各占满一整行：因此 agent 在四角是 1×2 时，todo / weather 也各是
 *   1×2，三模块整窗就是 3 行 × 2 列（3×2），而不是把需求总和 6 归档成 2×3 的方格。
 *   行数封顶到 ⌊ABS_MAX_CELLS / 列数⌋，模块过多时不撑爆窗口（到顶后下面按比例收缩）。
 *
 * ⚠️ 之所以要传入需求数组而不是单个总数：四角的行列无法只从「总格数」反推
 * （6 格既可是 2×3 也可是 3×2），必须知道是哪些模块、各要几格。
 */
export function windowGrid(layout: FloatingLayout, demands: readonly number[]): { rows: number; cols: number } {
  const weights = demands.map(cellWeight)
  const total = weights.reduce((sum, weight) => sum + weight, 0)
  switch (layout) {
    case 'horizontal':
      // 边条按个数精确取长度，不走阶梯（用户要「实际显示按个数」）。
      return { rows: 1, cols: clampCells(total, ABS_MIN_CELLS, ABS_MAX_CELLS) }
    case 'vertical':
      return { rows: clampCells(total, ABS_MIN_CELLS, ABS_MAX_CELLS), cols: 1 }
    case 'corner': {
      // 逐模块按自己的需求取 corner 网格（{1,2,3,4,6,9} → 1×1…3×3），再竖向堆叠。
      const grids = weights.map(weight => cellsGrid('corner', weight))
      const cols = Math.max(1, ...grids.map(grid => grid.cols))
      const rows = grids.reduce((sum, grid) => sum + grid.rows, 0)
      return { rows: clampCells(rows, 1, Math.max(1, Math.floor(ABS_MAX_CELLS / cols))), cols }
    }
    case 'square':
    default:
      // 未贴边的方形恒为一格，与模块需求无关。
      return { rows: 1, cols: 1 }
  }
}

/**
 * 某形态 + **各模块格子需求**下的悬浮窗像素尺寸（贴边尺寸与主窗口预览共用这一处口径）。
 * 传入的是需求数组而非单个总数：四角要按模块数分行，只有拿到需求才知道（见 {@link windowGrid}）。
 */
export function floatingWindowSize(
  layout: FloatingLayout,
  demands: readonly number[],
  cell: FloatingCellSize = FLOATING_CELL_SIZE
): FloatingCellSize {
  const { rows, cols } = windowGrid(layout, demands)
  return { width: cell.width * cols, height: cell.height * rows }
}

/** 网格列行数：横条 1 行、竖条 1 列、方形/四角按阶梯取行列。 */
export function cellsGrid(
  layout: FloatingLayout,
  cells: number = DEFAULT_WINDOW_CELLS
): { cols: number; rows: number } {
  const n = clampCells(cells, 1, ABS_MAX_CELLS)
  switch (layout) {
    case 'horizontal':
      // 边条按个数精确取长度，不走阶梯（用户要「实际显示按个数」）。
      return { cols: n, rows: 1 }
    case 'vertical':
      return { cols: 1, rows: n }
    case 'square':
    case 'corner':
    default:
      // 方形 / 贴角按阶梯取网格（贴角为非正方形，如 6 格 = 2 行 3 列）。
      return stepGrid(n)
  }
}
