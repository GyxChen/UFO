/**
 * `reference/plugin-template/CAPABILITIES.md` 的新鲜度护栏。
 *
 * 那份文档是 `/create-plugin-template` 从宿主源码整理出来的，它最大的坏法是**宿主改了能力面而文档没重生成**：
 * 作者照文档写代码，装上是装上了，用的是一个宿主不认识的名字 —— 那句话发生在运行时，作者先怀疑自己。
 *
 * 所以这里全是**双向集合**断言，不做「文档里有就放过」的单向检查：
 * - 少写（宿主加了能力 / 方法 / 入口字段，文档没跟上）→ 红；
 * - 多写（文档手改出一个宿主没有的名字）→ 也红。
 *
 * 判据的来源都是现成的第二份抄本（`plugin-capabilities.ts` / `plugin-limits.ts`），它们自己
 * 已被各自的 spec 与 Rust 对撞过，所以这里只负责「文档 ↔ 宿主」这一格。
 */

import { describe, expect, it } from 'vitest'
import docRaw from '../../reference/plugin-template/CAPABILITIES.md?raw'
import manifestRaw from '../../reference/plugin-template/public/plugin.json?raw'
import hostRaw from './plugin-host.ts?raw'
import loaderRaw from './plugin-loader.ts?raw'
import { PLUGIN_LIMITS } from './plugin-limits'
import { PLUGIN_PERMISSIONS, PLUGIN_SURFACES } from './plugin-capabilities'

/** 归一行尾：这份仓库里两种行尾都有，正则的 `^ {4}` 撞上 CRLF 会整段解析不出来。 */
const norm = (text: string) => text.replace(/\r\n/g, '\n')

const doc = norm(docRaw)
const loader = norm(loaderRaw)

/**
 * 只留代码。
 *
 * ⚠️ 必须剥注释：宿主源码的文档注释里满是「`host.http`」「`entry.tooltip`」这种写法，
 * 不剥就会拿散文当契约证据（同 `plugin-template.spec.ts` 的 `codeOf`）。
 */
function codeOf(text: string): string {
  return text
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/^[ \t]*\/\/[^\n]*$/gm, '')
}

/** 取一段文本里某个标记之后的内容（解析不到就抛，静默解析出 0 条等于假绿）。 */
function section(text: string, marker: string, endMarker: string): string {
  const at = text.indexOf(marker)
  if (at < 0) throw new Error(`找不到标记：${marker}`)
  const end = text.indexOf(endMarker, at)
  if (end < 0) throw new Error(`找不到段落结尾：${endMarker}`)
  return text.slice(at, end)
}

/** 文档某张表第一列反引号里的名字（按出现顺序）。 */
function tableColumn(title: string): string[] {
  const block = section(doc, `## ${title}`, '\n## ')
  return [...block.matchAll(/^\| `([^`]+)` \|/gm)].map(m => m[1])
}

/** `PluginHost` 接口的命名空间名（`storage: {` 这种，含 `readonly plugin: {`）。 */
function hostNamespaces(): string[] {
  const body = section(norm(hostRaw), 'export interface PluginHost {', '\n}')
  return [...body.matchAll(/^ {2}(?:readonly )?(\w+): \{$/gm)].map(m => m[1])
}

/** 接口里的顶层方法（只有 `log`：它不是命名空间，直接 `host.log(...)`）。 */
function hostTopLevelMethods(): string[] {
  const body = section(norm(hostRaw), 'export interface PluginHost {', '\n}')
  return [...body.matchAll(/^ {2}(?:readonly )?(\w+)\(/gm)].map(m => m[1])
}

/** `PluginHost` 接口里能被作者调的一切：命名空间成员 + 顶层方法。 */
function hostMembers(): string[] {
  const body = section(norm(hostRaw), 'export interface PluginHost {', '\n}')
  const nested = [...body.matchAll(/^ {4}(?:readonly )?(\w+)[(:]/gm)].map(m => m[1])
  return [...new Set([...nested, ...hostTopLevelMethods()])].sort()
}

/** 文档里逐项列出 API 的那些条目行（`- \`签名\``），取每条的起始名字。 */
function docListedApis(): string[] {
  return [...doc.matchAll(/^- `(\w+)[(:]/gm)].map(m => m[1])
}

/** 宿主从包的默认导出里实际读的字段名。 */
function loaderEntryFields(): string[] {
  return [...new Set([...codeOf(loader).matchAll(/\bentry\??\.(\w+)/g)].map(m => m[1]))].sort()
}

describe('CAPABILITIES.md 与宿主能力面同源', () => {
  it('文档头部写的版本就是宿主当前的那两个数', () => {
    expect(
      doc.includes(`API **v${PLUGIN_LIMITS.PLUGIN_API_VERSION}**`),
      `文档里的 API 版本不是 v${PLUGIN_LIMITS.PLUGIN_API_VERSION}：重跑 /create-plugin-template`
    ).toBe(true)
    expect(
      doc.includes(`清单格式 **v${PLUGIN_LIMITS.MANIFEST_VERSION}**`),
      `文档里的清单格式版本不是 v${PLUGIN_LIMITS.MANIFEST_VERSION}`
    ).toBe(true)
  })

  /** 双向：宿主登记的能力少写一条、或多写一条，都红。 */
  it('能力表与宿主的权限登记表一字不差', () => {
    expect(tableColumn('清单 `permissions` 能写哪些名字').sort(), '文档的能力表与 capability.rs 的登记表不一致').toEqual(
      PLUGIN_PERMISSIONS.map(item => item.id).sort()
    )
  })

  it('挂载点表与宿主的挂载点登记表一字不差', () => {
    expect(tableColumn('挂载点').sort(), '文档的挂载点表与 capability.rs 的登记表不一致').toEqual(
      PLUGIN_SURFACES.map(item => item.id).sort()
    )
  })

  it('每个 host 命名空间都有一节，且没有凭空多出来的节', () => {
    const sections = [...new Set([...doc.matchAll(/^### `host\.(\w+)/gm)].map(m => m[1]))].sort()
    const expected = [...new Set([...hostNamespaces(), ...hostTopLevelMethods()])].sort()
    expect(sections, '文档的 host 小节与 PluginHost 接口不一致').toEqual(expected)
  })

  /** 这条是「宿主加了个方法而文档没重生成」的直接抓手。 */
  it('文档逐项列出的 API 就是接口里能被调的那一批', () => {
    const listed = [...new Set(docListedApis())].sort()
    expect(listed, '文档列出的 API 名与 PluginHost 接口不一致').toEqual(hostMembers())
  })

  it('入口字段表就是宿主 plugin-loader 实际读的那些名字', () => {
    expect(tableColumn('入口：宿主会从你的默认导出里读这些名字').sort(), '文档的入口字段表与 plugin-loader.ts 的实读字段不一致').toEqual(
      loaderEntryFields()
    )
  })

  /** 「本模板演示了」这一列不能是拍脑袋写的：它必须等于清单真的声明了的那些。 */
  it('标注为已演示的能力，正是模板清单声明的那几项', () => {
    const block = section(doc, '## 清单 `permissions` 能写哪些名字', '\n## ')
    const demoed = [...block.matchAll(/^\| `([^`]+)` \|[^|]*\|[^|]*\| (是|否) \|$/gm)]
      .filter(m => m[2] === '是')
      .map(m => m[1])
    const manifest = JSON.parse(norm(manifestRaw)) as { permissions: string[] }
    expect(demoed.sort(), '文档说演示了这几项，与 plugin.json 的 permissions 不一致').toEqual(
      [...manifest.permissions].sort()
    )
  })
})
