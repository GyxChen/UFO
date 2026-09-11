import { describe, expect, it } from 'vitest'
import {
  enabledModuleDefinitions,
  isModuleEnabled,
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
  max_cells_by_layout: {
    square: 8,
    corner: 8,
    horizontal: 8,
    vertical: 8,
  },
}

const definitions: FloatingModuleDefinition[] = [
  {
    id: 'agent-status',
    label: 'Agent 状态',
    version: 1,
    defaultEnabled: true,
    supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
    component: {} as any,
  },
  {
    id: 'future-mod',
    label: 'Future',
    version: 1,
    defaultEnabled: false,
    supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
    component: {} as any,
  },
]

describe('floating module helpers', () => {
  it('isModuleEnabled returns true for enabled module', () => {
    expect(isModuleEnabled(config, 'agent-status')).toBe(true)
  })

  it('isModuleEnabled returns false for disabled module', () => {
    expect(isModuleEnabled(config, 'future-mod')).toBe(false)
  })

  it('isModuleEnabled returns false for missing module', () => {
    expect(isModuleEnabled(config, 'nonexistent')).toBe(false)
  })

  it('isModuleEnabled returns false for null config', () => {
    expect(isModuleEnabled(null, 'agent-status')).toBe(false)
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

  it('enabledModuleDefinitions returns empty for null config', () => {
    expect(enabledModuleDefinitions(null, definitions)).toHaveLength(0)
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
      { id: 'aaa-mod', label: 'AAA', version: 1, defaultEnabled: true, supportedLayouts: ['square'], component: {} as any },
    ]
    const result = enabledModuleDefinitions(sameOrder, defs)
    expect(result[0].id).toBe('aaa-mod')
    expect(result[1].id).toBe('agent-status')
  })
})
