import { describe, expect, it } from 'vitest'
import { ABS_MAX_CELLS, ABS_MIN_CELLS, DEFAULT_WINDOW_CELLS } from './floating-layout'
import {
  allocatedCells,
  defaultMaxCellsByLayout,
  enabledModuleDefinitions,
  isModuleEnabled,
  moduleCellShares,
  resolveWindowCells,
  type FloatingModuleConfig,
  type FloatingModuleDefinition,
} from './floating-modules'

const config: FloatingModuleConfig = {
  schema_version: 1,
  revision: 3,
  modules: [
    { id: 'agent-status', enabled: true, order: 0 },
    { id: 'future-mod', enabled: false, order: 1 },
  ],
  completed_retention_ms: 30_000,
  running_timeout_ms: 60_000,
  max_cells_by_layout: {
    square: 8,
    corner: 8,
    horizontal: 8,
    vertical: 8,
  },
  token_unit: 'tokens',
  proxy_resolver: 'none',
}

const definitions: FloatingModuleDefinition[] = [
  {
    id: 'agent-status',
    label: 'Agent 状态灯',
    description: '会话指示灯',
    version: 1,
    defaultEnabled: true,
    supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
    component: {} as never,
  },
  {
    id: 'future-mod',
    label: 'Future',
    description: '占位模块',
    version: 1,
    defaultEnabled: false,
    supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
    component: {} as never,
  },
]

describe('floating module helpers', () => {
  it('isModuleEnabled returns true for enabled module', () => {
    expect(isModuleEnabled(config, 'agent-status')).toBe(true)
  })

  it('isModuleEnabled returns false for disabled module', () => {
    expect(isModuleEnabled(config, 'future-mod')).toBe(false)
  })

  it('isModuleEnabled falls back to the definition default when config has no entry', () => {
    // 新增模块还没写进配置时：按定义里的默认值决定，不算被关掉。
    expect(isModuleEnabled(config, 'brand-new')).toBe(false)
    expect(isModuleEnabled(config, 'brand-new', true)).toBe(true)
    expect(isModuleEnabled(null, 'agent-status', true)).toBe(true)
  })

  it('enabledModuleDefinitions filters to enabled modules', () => {
    const result = enabledModuleDefinitions(config, definitions)
    expect(result).toHaveLength(1)
    expect(result[0].id).toBe('agent-status')
  })

  it('enabledModuleDefinitions sorts by config order', () => {
    const bothEnabled: FloatingModuleConfig = {
      ...config,
      modules: [
        { id: 'agent-status', enabled: true, order: 1 },
        { id: 'future-mod', enabled: true, order: 0 },
      ],
    }
    const result = enabledModuleDefinitions(bothEnabled, definitions)
    expect(result[0].id).toBe('future-mod')
    expect(result[1].id).toBe('agent-status')
  })

  it('enabledModuleDefinitions keeps default-enabled modules for a null config', () => {
    // 配置还没到手时按定义默认值挂载，避免悬浮窗白屏。
    const result = enabledModuleDefinitions(null, definitions)
    expect(result.map(definition => definition.id)).toEqual(['agent-status'])
  })

  it('enabledModuleDefinitions drops modules that do not support the layout', () => {
    const portraitOnly: FloatingModuleDefinition[] = [
      { ...definitions[0] },
      { ...definitions[1], id: 'vertical-only', defaultEnabled: true, supportedLayouts: ['vertical'] },
    ]
    expect(enabledModuleDefinitions(config, portraitOnly, 'vertical').map(item => item.id)).toEqual([
      'agent-status',
      'vertical-only',
    ])
    expect(enabledModuleDefinitions(config, portraitOnly, 'horizontal').map(item => item.id)).toEqual(['agent-status'])
  })

  it('enabledModuleDefinitions uses alphabetical for equal order', () => {
    const sameOrder: FloatingModuleConfig = {
      ...config,
      modules: [
        { id: 'agent-status', enabled: true, order: 0 },
        { id: 'aaa-mod', enabled: true, order: 0 },
      ],
    }
    const defs: FloatingModuleDefinition[] = [
      { ...definitions[0] },
      {
        id: 'aaa-mod',
        label: 'AAA',
        description: '同序模块',
        version: 1,
        defaultEnabled: true,
        supportedLayouts: ['square'],
        component: {} as never,
      },
    ]
    const result = enabledModuleDefinitions(sameOrder, defs)
    expect(result[0].id).toBe('aaa-mod')
    expect(result[1].id).toBe('agent-status')
  })
})

describe('floating window cell aggregation', () => {
  it('keeps a single module at its own demand', () => {
    expect(resolveWindowCells([4], 8)).toBe(4)
    expect(resolveWindowCells([6], 8)).toBe(6)
  })

  it('floors the window at the default capacity and caps it at the layout max', () => {
    expect(resolveWindowCells([], 8)).toBe(DEFAULT_WINDOW_CELLS)
    expect(resolveWindowCells([0], 8)).toBe(DEFAULT_WINDOW_CELLS)
    expect(resolveWindowCells([4], 3)).toBe(ABS_MIN_CELLS)
    expect(resolveWindowCells([8, 8], 8)).toBe(ABS_MAX_CELLS)
  })

  it('sums the demands of peer modules', () => {
    // 两个同级模块共享窗口：需求相加，各自按比例分到空间。
    expect(resolveWindowCells([4, 2], 8)).toBe(6)
    expect(resolveWindowCells([2, 2, 2], 8)).toBe(6)
  })

  it('treats non-finite or negative demands as one cell', () => {
    expect(resolveWindowCells([Number.NaN, 3], 8)).toBe(DEFAULT_WINDOW_CELLS)
    expect(resolveWindowCells([-5, -5, -5, -5], 8)).toBe(DEFAULT_WINDOW_CELLS)
  })

  it('derives integer shares that match the demands', () => {
    expect(moduleCellShares([4, 2])).toEqual([4, 2])
    expect(moduleCellShares([0, 1.9, Number.NaN])).toEqual([1, 1, 1])
  })

  it('splits the window between modules proportionally, never below one cell', () => {
    const shares = moduleCellShares([4, 2])
    // 6 格按 4:2 切 → 4 格 + 2 格。
    expect(allocatedCells(6, shares, 0)).toBe(4)
    expect(allocatedCells(6, shares, 1)).toBe(2)
    // 窗口被上限压小时仍然至少 1 格，不让模块消失。
    expect(allocatedCells(4, moduleCellShares([8, 8, 8]), 2)).toBe(1)
    // 没有任何权重信息时退回「整个窗口都给它」。
    expect(allocatedCells(5, [], 0)).toBe(5)
  })

  it('defaultMaxCellsByLayout clamps out-of-range values', () => {
    expect(defaultMaxCellsByLayout(0)).toEqual({
      square: ABS_MIN_CELLS,
      corner: ABS_MIN_CELLS,
      horizontal: ABS_MIN_CELLS,
      vertical: ABS_MIN_CELLS,
    })
    expect(defaultMaxCellsByLayout(99)).toEqual({
      square: ABS_MAX_CELLS,
      corner: ABS_MAX_CELLS,
      horizontal: ABS_MAX_CELLS,
      vertical: ABS_MAX_CELLS,
    })
    expect(defaultMaxCellsByLayout()).toEqual({
      square: ABS_MAX_CELLS,
      corner: ABS_MAX_CELLS,
      horizontal: ABS_MAX_CELLS,
      vertical: ABS_MAX_CELLS,
    })
  })
})
