import { describe, expect, it } from 'vitest'
import capabilityRaw from '../../src-tauri/src/plugin/capability.rs?raw'
import netRaw from '../../src-tauri/src/plugin/net.rs?raw'
import confRaw from '../../src-tauri/tauri.conf.json?raw'
import { PLUGIN_CAPABILITIES, PLUGIN_PERMISSIONS, PLUGIN_SURFACES, findCapability } from './plugin-capabilities'
import { PLUGIN_CAPABILITY_ICONS } from './plugin-icons'

/**
 * 能力 / 挂载点注册表的跨语言护栏。
 *
 * 这张表决定三件事：徽标怎么画、确认弹窗披露什么、装载时注入哪些全局。
 * 所以「Rust 加了一条而前端不知道」和「前端文案与 Rust 不一致」都必须红，
 * 而不是界面上少一枚徽标、弹窗里少一句承诺（后者是用户没被告知就放了权）。
 */

/** 归一行尾（CRLF 会藏进捕获组，见 plugin-limits.spec.ts 同一坑）。 */
const source = capabilityRaw.replace(/\r\n/g, '\n')

/** 取 `pub const NAME … = &[…]` 那个数组的正文（按中括号配平）。 */
function constArrayBody(text: string, marker: string): string {
  const start = text.indexOf(marker)
  if (start < 0) throw new Error(`找不到 ${marker}`)
  // 必须从 `=` 之后找 `[`：类型写法 `&[Capability]` 里也有一个中括号，
  // 从 marker 直接找会先把类型当成数组，解析出 1 个「Capability」了事。
  const assign = text.indexOf('=', start)
  if (assign < 0) throw new Error(`${marker} 后面没有 =`)
  const open = text.indexOf('[', assign)
  if (open < 0) throw new Error(`${marker} 的 = 后面没有 [`)
  let depth = 0
  for (let i = open; i < text.length; i += 1) {
    if (text[i] === '[') depth += 1
    else if (text[i] === ']') {
      depth -= 1
      if (depth === 0) return text.slice(open + 1, i)
    }
  }
  throw new Error(`${marker} 的中括号没有配平`)
}

/**
 * 取块体里 `字段: 值` 的值（按行找，避免 `id:` 误命中 `kind:` 这类前缀）。
 *
 * ⚠️ 这里刻意不用 `new RegExp` 拼字段名：模板字符串里的转义会被二次求值，
 * 失配时表现为「解析到 0 条」而不报错（仓库里踩过）。
 */
function fieldValue(body: string, field: string): string {
  const prefix = `${field}: `
  for (const line of body.split('\n')) {
    const trimmed = line.trim()
    if (!trimmed.startsWith(prefix)) continue
    const value = trimmed.slice(prefix.length).replace(/,+$/, '')
    if (value.startsWith('"')) {
      const end = value.indexOf('"', 1)
      if (end < 0) throw new Error(`字段 ${field} 的字符串没有闭合`)
      return value.slice(1, end)
    }
    const name = value.match(/^[A-Z_]+/)
    if (!name) throw new Error(`字段 ${field} 既不是字符串也不是常量`)
    return name[0]
  }
  throw new Error(`块里没有字段 ${field}`)
}

/** 把 Rust 的 `Capability { … }` 块逐个解析成行。 */
function parseRows(body: string, expectConst: string): Array<{
  id: string
  kind: string
  label: string
  icon: string
  tone: string
  disclosure: string
}> {
  const rows: ReturnType<typeof parseRows> = []
  let cursor = 0
  for (;;) {
    const start = body.indexOf('Capability {', cursor)
    if (start < 0) break
    const open = body.indexOf('{', start)
    let depth = 0
    let end = -1
    for (let i = open; i < body.length; i += 1) {
      if (body[i] === '{') depth += 1
      else if (body[i] === '}') {
        depth -= 1
        if (depth === 0) {
          end = i
          break
        }
      }
    }
    if (end < 0) throw new Error('Capability 块的大括号没有配平')
    const block = body.slice(open, end)
    const kind = fieldValue(block, 'kind')
    if (kind !== expectConst) throw new Error(`块里 kind 写的是 ${kind}，应为 ${expectConst}`)
    rows.push({
      id: fieldValue(block, 'id'),
      kind,
      label: fieldValue(block, 'label'),
      icon: fieldValue(block, 'icon'),
      // `TONE_DANGER` → 'danger'：三档名字与前端 tone 直接对齐。
      tone: fieldValue(block, 'tone').replace('TONE_', '').toLowerCase(),
      disclosure: fieldValue(block, 'disclosure'),
    })
    cursor = end
  }
  if (rows.length === 0) throw new Error('一条 Capability 都没解析到（写法变了要同步改这里的解析）')
  return rows
}

/** 断言 Rust 里 `TONE_*` 的取值就是前端那三个字符串（不然 tone 会悄悄错档）。 */
function toneValues(): Record<string, string> {
  const found: Record<string, string> = {}
  for (const match of source.matchAll(/pub const (TONE_[A-Z]+): &str = "([a-z]+)";/g)) {
    found[match[1]] = match[2]
  }
  expect(Object.keys(found)).toEqual(['TONE_DANGER', 'TONE_CAUTION', 'TONE_LOCAL'])
  return {
    danger: found.TONE_DANGER,
    caution: found.TONE_CAUTION,
    local: found.TONE_LOCAL,
  }
}

const rustPermissions = parseRows(constArrayBody(source, 'pub const PERMISSIONS'), 'KIND_PERMISSION')
const rustSurfaces = parseRows(constArrayBody(source, 'pub const SURFACES'), 'KIND_SURFACE')

/** 把前端表投成与 Rust 解析结果同形的对象，才能整表 deep-equal。 */
function projection(rows: typeof PLUGIN_PERMISSIONS) {
  return rows.map(row => ({
    id: row.id,
    kind: row.kind === 'permission' ? 'KIND_PERMISSION' : 'KIND_SURFACE',
    label: row.label,
    icon: row.icon,
    tone: row.tone,
    disclosure: row.disclosure,
  }))
}

describe('能力注册表的跨语言对撞', () => {
  it('宿主能力表逐条同源（名字 / 徽标文字 / 图标 / 配色档 / 披露原文）', () => {
    expect(projection(PLUGIN_PERMISSIONS)).toEqual(rustPermissions)
  })

  it('挂载点表逐条同源', () => {
    expect(projection(PLUGIN_SURFACES)).toEqual(rustSurfaces)
  })

  it('tone 的三档在两边是同一组字符串', () => {
    expect(toneValues()).toEqual({ danger: 'danger', caution: 'caution', local: 'local' })
  })

  it('Rust 用到的每个图标键都有路径', () => {
    const keys = [...rustPermissions, ...rustSurfaces].map(row => row.icon)
    for (const key of keys) {
      expect(PLUGIN_CAPABILITY_ICONS[key], `图标 ${key} 没有路径`).toBeTruthy()
    }
  })

  it('危险度的档位符合口径：能读提问原文与读文件是 danger，出网/外设是 caution', () => {
    const toneOf = (id: string) => findCapability(id)?.tone
    expect(toneOf('events:agent')).toBe('danger')
    expect(toneOf('fs')).toBe('danger')
    expect(toneOf('http')).toBe('caution')
    expect(toneOf('secrets')).toBe('caution')
    expect(toneOf('ble')).toBe('caution')
    expect(toneOf('serial')).toBe('caution')
    expect(toneOf('frame')).toBe('caution')
    expect(toneOf('storage')).toBe('local')
    // 四个挂载点都不授权，一律 local 档。
    for (const row of PLUGIN_SURFACES) expect(row.tone).toBe('local')
  })

  it('`ufo.log` 是需求点名的特例：它有 API 但不进这张表（不出徽标）', () => {
    expect(findCapability('log')).toBeUndefined()
    expect(rustPermissions.some(row => row.id === 'log')).toBe(false)
  })

  it('合并表的顺序 = 能力在前、挂载点在后，且 id 不重复', () => {
    const ids = PLUGIN_CAPABILITIES.map(row => row.id)
    expect(PLUGIN_CAPABILITIES).toEqual([...PLUGIN_PERMISSIONS, ...PLUGIN_SURFACES])
    expect(new Set(ids).size).toBe(ids.length)
  })

  it('每条披露都写满一句「会碰到什么」', () => {
    for (const row of PLUGIN_CAPABILITIES) {
      // 下限 16 字：短到这个数以下基本只剩复读 label，用户看不出会碰什么。
      expect(row.disclosure.length, `${row.id} 的披露太短`).toBeGreaterThanOrEqual(16)
      expect(row.disclosure, `${row.id} 的披露没收尾`).toMatch(/。$/)
    }
  })
})

/**
 * CSP 必须跟能力面同向。
 *
 * 这是「只有真窗才知道」那一类缺陷的护栏：能力实装齐了、命令也通了，
 * 但 `frame-src` 没放行，表现是组件里的 `<iframe>` 一片空白 + 一行控制台 CSP 报错，
 * 而作者先看的是宿主文档，最容易归因错。JSON 里不能写注释，所以理由写在这里。
 */
describe('CSP 与能力面同向', () => {
  /** `"csp"` 对象里的 frame-src。 */
  const csp = (JSON.parse(confRaw.replace(/\r\n/g, '\n')) as { app: { security: { csp: Record<string, string> } } })
    .app.security.csp

  it('frame-src 放行组件产物源（容器成员直链要靠 iframe 装）', () => {
    const frameSrc = csp['frame-src'] ?? ''
    // `fs` 的用法之一是「把 epub 里的一页挂进 iframe」，那条 URL 是协议呈递出来的，
    // Windows 是 host 形式、macOS/Linux 是 scheme 形式，两边都得在名单里。
    expect(frameSrc, '组件产物不能当 iframe 装：容器成员直链会白屏').toContain('http://ufo-plugin.localhost')
    expect(frameSrc).toContain('ufo-plugin:')
  })

  it('frame-src 放行 https（`frame` 能力的承诺就是「能嵌网页」）', () => {
    expect(findCapability('frame'), 'frame 这条能力还在表里').toBeTruthy()
    // 外站能不能显示由对方的 X-Frame-Options 决定，披露里已经写明「显示不出来是正常现象」；
    // 宿主这边只保证 CSP 不替它们把门关死。
    expect(csp['frame-src'] ?? '').toMatch(/\bhttps:/)
  })

  it('frame 的闸与 CSP 同向：两边都不收明文 http', () => {
    const netSource = netRaw.replace(/\r\n/g, '\n')
    // 只认代码里那句判据 —— net.rs 的注释也写了「只放 https」，对散文做 toContain 等于没护栏。
    expect(netSource, 'Rust 的 frame 闸不再要求 https：放行的明文页会被 CSP 挡成白屏').toContain(
      'starts_with("https://")'
    )
    // `http://ufo-plugin.localhost` 这种带 host 的条目里也含 "http"，所以要按空格拆 token 逐个数。
    const bare = (csp['frame-src'] ?? '').split(/\s+/).filter(token => token === 'http:')
    expect(bare, 'frame-src 整段放行了明文：与 host.frame 的判据相反了').toEqual([])
  })

  it('组件产物仍在 script-src 里（多 chunk 的 ESM 要跨源取）', () => {
    expect(csp['script-src'] ?? '').toContain('http://ufo-plugin.localhost')
  })
})
