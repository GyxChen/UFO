import { describe, expect, it } from 'vitest'
import manifestRaw from '../../src-tauri/src/plugin/manifest.rs?raw'
import { declaredSurfaceIds, tooltipSizeOf, RECENT_LOG_LINES } from './plugin-types'
import type { PluginManifestJson } from './plugin-types'

/**
 * 清单形状的跨语言护栏 + 挂载点推导表。
 *
 * `plugin-types.ts` 里的 interface 是 Rust 序列化结果的镜像，但 TS 类型在运行时会被擦掉，
 * 所以这里比对的是**手写的字段名清单**：Rust 改字段名 / 加字段 / 删字段都会让它红，
 * 提醒同一次改动里把 TS interface 与用到它的界面一起改掉（漏改的表现是「字段永远读不到」）。
 */

/** 归一行尾（与 plugin-limits.spec.ts 同一坑）。 */
const source = manifestRaw.replace(/\r\n/g, '\n')

/** 取 `pub struct NAME { … }` 里 `pub 字段:` 的字段名（按 snake→camel 换算，与 serde 的 rename_all 同规则）。 */
function structFields(name: string): string[] {
  const start = source.indexOf(`pub struct ${name} {`)
  if (start < 0) throw new Error(`找不到 struct ${name}`)
  const end = source.indexOf('\n}', start)
  if (end < 0) throw new Error(`struct ${name} 没有以 \\n} 收尾`)
  const body = source.slice(start, end)
  const fields = body
    .split('\n')
    .map(line => line.trim())
    // 只认真正的字段行（`pub 名字: 类型`）：`pub struct X {` 那一行也带 `pub ` 前缀。
    .filter(line => /^pub\s+[A-Za-z_][A-Za-z0-9_]*\s*:/.test(line))
    .map(line => line.replace(/^pub\s+/, '').split(':')[0].trim())
    .map(snake => snake.replace(/_([a-z])/g, (_all, letter: string) => letter.toUpperCase()))
  if (fields.length === 0) throw new Error(`struct ${name} 一个字段都没解析到`)
  return fields
}

/** 一份最小可用清单（其余字段按缺省走）。 */
function manifest(patch: Partial<PluginManifestJson> = {}): PluginManifestJson {
  return {
    id: 'balance-board',
    name: '余额看板',
    version: '1.2.0',
    description: '',
    entry: 'main.js',
    surfaces: { cell: 2, tooltip: { own: true, size: [340, 260] }, extension: { into: 'agent-status', maxRows: 3 }, settings: true },
    permissions: [],
    hosts: [],
    ...patch,
  }
}

describe('清单字段与 Rust 的镜像关系', () => {
  it('Manifest 的字段名逐个对上（snake→camel 后一致）', () => {
    expect(structFields('Manifest')).toEqual([
      'manifestVersion',
      'apiVersion',
      'id',
      'name',
      'version',
      'description',
      'entry',
      'surfaces',
      'permissions',
      'hosts',
    ])
  })

  it('Surfaces 的四个挂载点字段没变名', () => {
    expect(structFields('Surfaces')).toEqual(['cell', 'tooltip', 'extension', 'settings'])
  })

  it('独立信息卡与插入块的字段没变名', () => {
    expect(structFields('OwnTooltip')).toEqual(['own', 'size'])
    expect(structFields('Extension')).toEqual(['into', 'maxRows'])
  })
})

describe('挂载点推导', () => {
  it('四项全声明时按固定顺序产出', () => {
    expect(declaredSurfaceIds(manifest())).toEqual(['cell', 'tooltip-own', 'tooltip-extension', 'settings'])
  })

  it('cell 为 null 时不申请格子（只出现在信息卡 / 设置页的组件）', () => {
    expect(declaredSurfaceIds(manifest({ surfaces: { settings: true } }))).toEqual(['settings'])
  })

  it('写了 tooltip 但 own 为 false 不算声明独立卡', () => {
    expect(
      declaredSurfaceIds(
        manifest({ surfaces: { tooltip: { own: false, size: [340, 260] }, settings: true } })
      )
    ).toEqual(['settings'])
  })

  it('extension 存在即算插入块，与 maxRows 取值无关', () => {
    expect(
      declaredSurfaceIds(manifest({ surfaces: { extension: { into: 'agent-status', maxRows: 1 } } }))
    ).toEqual(['tooltip-extension'])
  })

  it('什么都没声明就是空表（纯后台包）', () => {
    expect(declaredSurfaceIds(manifest({ surfaces: {} }))).toEqual([])
  })

  it('调试卡的最近日志行数是界面预算，不跟随宿主上限变动', () => {
    expect(RECENT_LOG_LINES).toBe(3)
  })
})

describe('信息卡尺寸取值', () => {
  it('声明了 size 就换算成外壳用的对象', () => {
    expect(tooltipSizeOf(manifest())).toEqual({ width: 340, height: 260 })
  })

  it('没声明返回 null（外壳回落 tauri.conf.json 的默认尺寸）', () => {
    expect(tooltipSizeOf(manifest({ surfaces: { tooltip: { own: true } } }))).toBeNull()
    expect(tooltipSizeOf(manifest({ surfaces: {} }))).toBeNull()
  })
})
