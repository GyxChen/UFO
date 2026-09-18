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
  squareSideForCells,
} from './floating-layout'

describe('floating layout primitives', () => {
  it('derives each floating layout from docked edges', () => {
    expect(layoutFromDocked(null)).toBe('square')
    expect(layoutFromDocked({ left: false, top: false, right: false, bottom: false })).toBe('square')
    expect(layoutFromDocked({ left: false, top: true, right: false, bottom: false })).toBe('horizontal')
    expect(layoutFromDocked({ left: true, top: false, right: false, bottom: false })).toBe('vertical')
    expect(layoutFromDocked({ left: true, top: false, right: false, bottom: true })).toBe('corner')
  })

  it('computes square side as ceil(sqrt(cells))', () => {
    expect(squareSideForCells(1)).toBe(1)
    expect(squareSideForCells(4)).toBe(2)
    expect(squareSideForCells(5)).toBe(3)
    expect(squareSideForCells(8)).toBe(3)
  })

  it('maps each layout to its grid for a cell count', () => {
    expect(cellsGrid('square', 4)).toEqual({ cols: 2, rows: 2 })
    expect(cellsGrid('corner', 4)).toEqual({ cols: 2, rows: 2 })
    expect(cellsGrid('corner', 5)).toEqual({ cols: 3, rows: 3 })
    expect(cellsGrid('horizontal', 6)).toEqual({ cols: 6, rows: 1 })
    expect(cellsGrid('vertical', 6)).toEqual({ cols: 1, rows: 6 })
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
    expect(clampCells(3.7, ABS_MIN_CELLS, ABS_MAX_CELLS)).toBe(ABS_MIN_CELLS)
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
    // 方形恒 1 格；横条按宽度、竖条按高度成倍放大；四角保持正方形（side = ⌈√cells⌉）。
    expect(floatingWindowSize('square', 4, cellSizeOf(64))).toEqual({ width: 64, height: 64 })
    expect(floatingWindowSize('horizontal', 4, cellSizeOf(64))).toEqual({ width: 256, height: 64 })
    expect(floatingWindowSize('vertical', 8, cellSizeOf(50))).toEqual({ width: 50, height: 400 })
    expect(floatingWindowSize('corner', 8, cellSizeOf(50))).toEqual({ width: 150, height: 150 })
  })
})
