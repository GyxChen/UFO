import { describe, expect, it } from 'vitest'
// ?raw 读 Rust 源：常量与判据的权威在那边，这里只允许「逐条字面量对上」。
import pathsRaw from '../../src-tauri/src/plugin/paths.rs?raw'
import { PLUGIN_ID_MAX_CHARS, PLUGIN_ID_PATTERN, PLUGIN_LIMITS, isSafePluginRelPath } from './plugin-limits'

/**
 * 体积 / 配额上限的跨语言护栏。
 *
 * 手法照 `push-providers.spec.ts`：`?raw` 读源、正则抠常量、与手抄表逐条比。
 * ⚠️ 表里的期望值**全部手写字面量**，绝不从 Rust 反推 —— 反推等于 `x === x`，是一条永远绿的假护栏。
 */

/** 归一行尾：仓库是 CRLF，`\r` 会钻进 `[^;]+` 里让值带上尾随回车（表现为「看着一样但不等」）。 */
const source = pathsRaw.replace(/\r\n/g, '\n')

/** 一行 `pub const NAME: T = VALUE;`。 */
const CONST_LINE = /^pub const ([A-Z0-9_]+): [^=\n]+ = ([^;]+);$/gm

/** 把 `64 * 1024` 这类乘积算成数字；字符串常量取内容；看不懂的就抛（宁可红也不静默漏）。 */
function evaluate(value: string): string | number {
  const text = value.trim()
  if (text.startsWith('"') && text.endsWith('"')) return text.slice(1, -1)
  // Rust 的数字字面量可以写下划线分组（`1_000`），比较前先去掉。
  const numeric = text.replace(/_/g, '')
  if (/^[0-9]+(?: \* [0-9]+)*$/.test(numeric)) {
    return numeric
      .split(' * ')
      .reduce((product, factor) => product * Number(factor), 1)
  }
  throw new Error(`看不懂的上限取值：${text}`)
}

/** 抠出 paths.rs 的全部常量。 */
function parseRustConsts(text: string): Record<string, string | number> {
  const consts: Record<string, string | number> = {}
  for (const match of text.matchAll(CONST_LINE)) {
    consts[match[1]] = evaluate(match[2])
  }
  if (Object.keys(consts).length === 0) throw new Error('paths.rs 里一条常量都没解析到')
  return consts
}

const rust = parseRustConsts(source)
const tsTable: Record<string, string | number> = { ...PLUGIN_LIMITS }

/** 与 Rust `is_valid_plugin_id` 同一批评语：两侧各自独立断言同一张表。 */
const ID_CASES: ReadonlyArray<readonly [string, boolean]> = [
  ['balance-board', true],
  ['a', true],
  ['clock24', true],
  ['ci-timing-pro', true],
  ['', false],
  ['Balance', false],
  ['my_plugin', false],
  ['a/b', false],
  ['..', false],
  ['-lead', false],
  ['trail-', false],
  ['double--dash', false],
  ['中文', false],
  ['a'.repeat(PLUGIN_ID_MAX_CHARS + 1), false],
]

/** 与 Rust `is_safe_rel_path` 同一批评语（越界的形状一个都不许只测 `../`）。 */
const PATH_CASES: ReadonlyArray<readonly [string, boolean]> = [
  ['main.js', true],
  ['assets/index-a1b2c3.js', true],
  ['chunk.v2.min.js', true],
  ['../secret.txt', false],
  ['assets/../../secret.txt', false],
  ['/etc/passwd', false],
  ['C:\\Windows\\win.ini', false],
  ['C:/Windows/win.ini', false],
  ['a\\b.js', false],
  ['a//b.js', false],
  ['./a.js', false],
  ['a<b.js', false],
  ['a'.repeat(PLUGIN_LIMITS.MAX_REL_PATH_CHARS + 1), false],
]

describe('插件上限的跨语言对撞', () => {
  it('Rust 里每条常量都在前端表里登记（新增帽没登记就红）', () => {
    const missing = Object.keys(rust).filter(name => !(name in tsTable))
    expect(missing).toEqual([])
  })

  it('前端表里没有凭空多出的条目（删了 Rust 常量也要删抄本）', () => {
    const extra = Object.keys(tsTable).filter(name => !(name in rust))
    expect(extra).toEqual([])
  })

  it('逐条取值相同', () => {
    // 一条一条比，好让失败信息直接点名是哪一项漂移。
    for (const [name, value] of Object.entries(rust)) {
      expect(tsTable[name], `上限 ${name}`).toBe(value)
    }
  })

  it('版本常量按当前契约取值', () => {
    expect(rust.PLUGIN_API_VERSION).toBe(1)
    expect(rust.MANIFEST_VERSION).toBe(1)
  })

  it('体积帽的口径是「放宽后」的量级，不是回到 2 MiB', () => {
    // 抬帽是本轮架构结论的一部分（大产物 + ESM 多 chunk）；被改回小帽要有意识。
    expect(PLUGIN_LIMITS.MAX_SCRIPT_BYTES).toBe(8 * 1024 * 1024)
    expect(PLUGIN_LIMITS.MAX_TOTAL_BYTES).toBe(48 * 1024 * 1024)
    expect(PLUGIN_LIMITS.MAX_FILES).toBe(256)
  })

  it('格子帽是物理约束：16 格不随其它帽一起被抬', () => {
    expect(PLUGIN_LIMITS.MIN_CELLS).toBe(1)
    expect(PLUGIN_LIMITS.MAX_CELLS).toBe(16)
    expect(PLUGIN_LIMITS.MAX_EXTENSION_ROWS).toBe(3)
  })
})

describe('id 与包内路径的判据', () => {
  it('id 判据与 Rust 的字符集规则逐条一致', () => {
    for (const [id, expected] of ID_CASES) {
      const actual = id.length <= PLUGIN_ID_MAX_CHARS && PLUGIN_ID_PATTERN.test(id)
      expect(actual, `id「${id}」`).toBe(expected)
    }
  })

  it('包内路径允许多段、挡住全部穿越形状', () => {
    for (const [path, expected] of PATH_CASES) {
      expect(isSafePluginRelPath(path), `路径「${path}」`).toBe(expected)
    }
  })

  it('NUL 与控制字符都在判据里被拒（Rust 侧同样先查）', () => {
    expect(isSafePluginRelPath('a\0.js')).toBe(false)
    expect(isSafePluginRelPath('a\nb.js')).toBe(false)
  })
})
