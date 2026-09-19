import { describe, expect, it } from 'vitest'
import { DEFAULT_TODO_SETTINGS, todoSettings } from './settings'

/** 造一份只有 todo 配置块的外壳配置（外壳不解释块里的字段）。 */
function configWith(block: Record<string, unknown>) {
  return {
    schema_version: 1,
    revision: 1,
    modules: [],
    max_cells_by_layout: { square: 4, corner: 4, horizontal: 4, vertical: 4 },
    module_settings: { todo: block },
  }
}

describe('todoSettings', () => {
  it('没有配置 / 配置块为空时回落默认值', () => {
    expect(todoSettings(null)).toEqual(DEFAULT_TODO_SETTINGS)
    expect(todoSettings(configWith({}))).toEqual(DEFAULT_TODO_SETTINGS)
  })

  it('解析各字段并去掉首尾空白', () => {
    const result = todoSettings(
      configWith({
        has_token: true,
        project_id: '  69a850f41c20d2030e148fdf  ',
        project_name: ' 工作 ',
        refresh_interval_min: 30,
      }),
    )

    expect(result.has_token).toBe(true)
    expect(result.project_id).toBe('69a850f41c20d2030e148fdf')
    expect(result.project_name).toBe('工作')
    expect(result.refresh_interval_min).toBe(30)
  })

  it('刷新周期夹在 5..=60', () => {
    expect(todoSettings(configWith({ refresh_interval_min: 1 })).refresh_interval_min).toBe(5)
    expect(todoSettings(configWith({ refresh_interval_min: 999 })).refresh_interval_min).toBe(60)
  })

  it('字段类型不对时只影响它自己（不整块失效）', () => {
    const result = todoSettings(
      configWith({ has_token: 'yes', project_id: 42, refresh_interval_min: 'soon' }),
    )

    expect(result.has_token).toBe(true) // 强转布尔：非空字符串为真
    expect(result.project_id).toBe('')
    expect(result.refresh_interval_min).toBe(DEFAULT_TODO_SETTINGS.refresh_interval_min)
  })
})
