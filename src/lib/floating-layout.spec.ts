import { describe, expect, it } from 'vitest'
import {
  ABS_MAX_CELLS,
  ABS_MIN_CELLS,
  DEFAULT_WINDOW_CELLS,
  cellsGrid,
  clampCells,
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

  it('clamps cells into the absolute range', () => {
    expect(clampCells(0, ABS_MIN_CELLS, ABS_MAX_CELLS)).toBe(ABS_MIN_CELLS)
    expect(clampCells(3.7, ABS_MIN_CELLS, ABS_MAX_CELLS)).toBe(ABS_MIN_CELLS)
    expect(clampCells(99, ABS_MIN_CELLS, ABS_MAX_CELLS)).toBe(ABS_MAX_CELLS)
    expect(cellsGrid('vertical', 99)).toEqual({ cols: 1, rows: ABS_MAX_CELLS })
    expect(cellsGrid('vertical', 0)).toEqual({ cols: 1, rows: 1 })
  })
})
