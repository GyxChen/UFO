import { describe, expect, it } from 'vitest'
// ?raw 读 tauri.conf.json：测试环境把 node:fs polyfill 掉了，走 vite 的原始文本导入最稳。
import tauriConfRaw from '../../src-tauri/tauri.conf.json?raw'
import {
  ABS_MAX_CELLS,
  ABS_MIN_CELLS,
  DEFAULT_WINDOW_CELLS,
  FLOATING_BASE_CELL_PX,
  FLOATING_CELL_SIZE,
  MAX_CELL_PX,
  MIN_CELL_PX,
  cellSizeOf,
  cellsGrid,
  clampCellPx,
  clampCells,
  floatingWindowSize,
  layoutFromDocked,
  snapToStep,
  windowGrid,
} from './floating-layout'

describe('floating layout primitives', () => {
  it('derives each floating layout from docked edges', () => {
    expect(layoutFromDocked(null)).toBe('square')
    expect(layoutFromDocked({ left: false, top: false, right: false, bottom: false })).toBe('square')
    expect(layoutFromDocked({ left: false, top: true, right: false, bottom: false })).toBe('horizontal')
    expect(layoutFromDocked({ left: true, top: false, right: false, bottom: false })).toBe('vertical')
    expect(layoutFromDocked({ left: true, top: false, right: false, bottom: true })).toBe('corner')
  })

  it('maps each layout to its grid for a cell count', () => {
    expect(cellsGrid('square', 4)).toEqual({ cols: 2, rows: 2 })
    expect(cellsGrid('corner', 4)).toEqual({ cols: 2, rows: 2 })
    expect(cellsGrid('horizontal', 6)).toEqual({ cols: 6, rows: 1 })
    expect(cellsGrid('vertical', 6)).toEqual({ cols: 1, rows: 6 })
  })

  it('snaps square / corner cells onto the discrete step ladder', () => {
    // {1,2,3,4,6,9,12,16} → 1×1 / 1×2 / 1×3 / 2×2 / 2×3 / 3×3 / 3×4 / 4×4（行×列）
    expect(cellsGrid('corner', 1)).toEqual({ cols: 1, rows: 1 })
    expect(cellsGrid('corner', 2)).toEqual({ cols: 2, rows: 1 })
    expect(cellsGrid('corner', 4)).toEqual({ cols: 2, rows: 2 })
    expect(cellsGrid('corner', 6)).toEqual({ cols: 3, rows: 2 })
    expect(cellsGrid('corner', 9)).toEqual({ cols: 3, rows: 3 })
    expect(cellsGrid('corner', 12)).toEqual({ cols: 4, rows: 3 })
    expect(cellsGrid('corner', 16)).toEqual({ cols: 4, rows: 4 })
    // 非档位向上夹到最近档
    expect(snapToStep(5)).toBe(6)
    expect(snapToStep(10)).toBe(12)
    expect(snapToStep(13)).toBe(16)
    expect(cellsGrid('corner', 5)).toEqual({ cols: 3, rows: 2 })
    // 边条不走阶梯，按个数精确
    expect(cellsGrid('horizontal', 5)).toEqual({ cols: 5, rows: 1 })
    expect(cellsGrid('vertical', 7)).toEqual({ cols: 1, rows: 7 })
  })

  it('defaults the grid to the default window capacity', () => {
    expect(cellsGrid('square')).toEqual(cellsGrid('square', DEFAULT_WINDOW_CELLS))
  })

  it('keeps the floating window initial size in sync with one cell', () => {
    // 启动瞬间的窗口尺寸与吸附口径靠这一个数对齐：tauri.conf.json 的 floating 初值必须等于一格，
    // 否则未贴边时窗口与贴边计算会互相打架（见 FLOATING_BASE_CELL_PX 的注释）。
    const conf = JSON.parse(tauriConfRaw) as {
      app: { windows: { label: string; width: number; height: number }[] }
    }
    const floating = conf.app.windows.find(window => window.label === 'floating')
    expect(floating?.width).toBe(FLOATING_CELL_SIZE.width)
    expect(floating?.height).toBe(FLOATING_CELL_SIZE.height)
  })

  it('clamps cells into the absolute range', () => {
    expect(clampCells(0, ABS_MIN_CELLS, ABS_MAX_CELLS)).toBe(ABS_MIN_CELLS)
    expect(clampCells(-5, ABS_MIN_CELLS, ABS_MAX_CELLS)).toBe(ABS_MIN_CELLS)
    // 落在 [1, 9] 内的值取整后保留（不再被抬到旧的下限 4）。
    expect(clampCells(3.7, ABS_MIN_CELLS, ABS_MAX_CELLS)).toBe(3)
    expect(clampCells(99, ABS_MIN_CELLS, ABS_MAX_CELLS)).toBe(ABS_MAX_CELLS)
    expect(cellsGrid('vertical', 99)).toEqual({ cols: 1, rows: ABS_MAX_CELLS })
    expect(cellsGrid('vertical', 0)).toEqual({ cols: 1, rows: 1 })
  })
})

describe('floating cell size', () => {
  it('clamps the cell size into the configurable range', () => {
    expect(clampCellPx(MIN_CELL_PX - 10)).toBe(MIN_CELL_PX)
    expect(clampCellPx(MAX_CELL_PX + 10)).toBe(MAX_CELL_PX)
    // 非整数取整；非有限值回落默认格边长（用户没配过时的口径）。
    expect(clampCellPx(45.6)).toBe(46)
    expect(clampCellPx(Number.NaN)).toBe(FLOATING_BASE_CELL_PX)
    expect(clampCellPx(Number.POSITIVE_INFINITY)).toBe(FLOATING_BASE_CELL_PX)
  })

  it('builds a square cell size from a pixel value', () => {
    expect(cellSizeOf(64)).toEqual({ width: 64, height: 64 })
    expect(cellSizeOf(0)).toEqual({ width: MIN_CELL_PX, height: MIN_CELL_PX })
  })

  it('scales every layout by the configured cell size', () => {
    // 方形恒 1 格；横条按宽度、竖条按高度成倍放大；四角逐模块竖向堆叠（单模块 2×2 / 2行×3列）。
    expect(floatingWindowSize('square', [4], cellSizeOf(64))).toEqual({ width: 64, height: 64 })
    expect(floatingWindowSize('horizontal', [4], cellSizeOf(64))).toEqual({ width: 256, height: 64 })
    expect(floatingWindowSize('vertical', [8], cellSizeOf(50))).toEqual({ width: 50, height: 400 })
    expect(floatingWindowSize('corner', [4], cellSizeOf(50))).toEqual({ width: 100, height: 100 })
    expect(floatingWindowSize('corner', [6], cellSizeOf(50))).toEqual({ width: 150, height: 100 })
  })
})

describe('window grid from per-module demands', () => {
  it('derives rows and cols for the edge strips and the undocked square', () => {
    expect(windowGrid('horizontal', [4, 2])).toEqual({ rows: 1, cols: 6 })
    expect(windowGrid('vertical', [4, 2])).toEqual({ rows: 6, cols: 1 })
    // 未贴边的方形恒一格，与需求无关。
    expect(windowGrid('square', [4, 2])).toEqual({ rows: 1, cols: 1 })
  })

  it('stacks corner modules as full-width rows, not a packed square', () => {
    // 三个各申请 2 格的模块（在四角各占一整行）：3 行 × 2 列，而不是把总和 6 归档成 2×3。
    expect(windowGrid('corner', [2, 2, 2])).toEqual({ rows: 3, cols: 2 })
    // 单个 2×2 模块（agent 4 会话）仍是它自己的 2×2。
    expect(windowGrid('corner', [4])).toEqual({ rows: 2, cols: 2 })
    // agent 2×2 + 两个 1×2：行数 2+1+1 = 4，最宽 2 列 → 4 行 × 2 列。
    expect(windowGrid('corner', [4, 2, 2])).toEqual({ rows: 4, cols: 2 })
    // agent 2×3（更宽）带一个 1×2：行数 2+1 = 3，最宽 3 列 → 3 行 × 3 列。
    expect(windowGrid('corner', [6, 2])).toEqual({ rows: 3, cols: 3 })
  })

  it('keeps the corner grid within the absolute cell cap', () => {
    // 很多 1×2 模块竖向堆叠：行数封顶到 ⌊16 / 2⌋ = 8，不撑爆窗口。
    const grid = windowGrid('corner', [2, 2, 2, 2, 2, 2, 2, 2, 2, 2])
    expect(grid.cols).toBe(2)
    expect(grid.rows).toBe(8)
  })

  it('treats empty or invalid demands as a single cell', () => {
    expect(windowGrid('horizontal', [])).toEqual({ rows: 1, cols: 1 })
    expect(windowGrid('corner', [])).toEqual({ rows: 1, cols: 1 })
    expect(floatingWindowSize('corner', [], cellSizeOf(45))).toEqual({ width: 45, height: 45 })
  })
})
