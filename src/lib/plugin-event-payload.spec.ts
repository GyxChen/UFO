/**
 * 需求 1 的原话是「订阅信息包括（状态灯转灯, endpoint, 提问, id 和时间等必要信息）」，
 * 而这件事的证据只能是：**Rust 那条事件与本仓库作者看到的那条形状逐字段相等**。
 *
 * 为什么单独一条：`AgentEvent`（Rust，serde camelCase 出去）与 `PluginAgentEvent`（TS，手写镜像）
 * 之间没有任何编译期联系 —— TS 类型在运行时被擦掉，Rust 改个字段名的表现是
 * 「作者那边永远是 undefined」，而构建、类型检查、其余单测全绿（组件包不在本仓编译）。
 * 同一条也挡住反向：TS 侧漏改一个字段，界面会照抄一份读不到的键。
 *
 * ⚠️ 两侧的字段都是从**源码文本**解析的，所以先剥注释：文档式注释里满是 `endpoint：…`
 * 这种形状，不剥就会把散文当证据（同 `plugin-template.spec.ts` 的 `codeOf`）。
 */

import { describe, expect, it } from 'vitest'
import eventsRaw from '../../src-tauri/src/plugin/events.rs?raw'
import hostRaw from './plugin-host.ts?raw'

/** 归一行尾 + 剥掉块注释与行注释（`?raw` 在 Windows 上是 CRLF）。 */
function codeOf(raw: string): string {
  return raw
    .replace(/\r\n/g, '\n')
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/\/\/[^\n]*/g, '')
}

/** snake_case → camelCase（与 serde 的 `rename_all = "camelCase"` 同规则）。 */
function camel(name: string): string {
  return name.replace(/_([a-z0-9])/g, (_all, letter: string) => letter.toUpperCase())
}

/** 取 `pub struct NAME { … }` 的字段名（只认 `pub 名字:` 那几行）。 */
function rustFields(name: string): string[] {
  const source = codeOf(eventsRaw)
  const start = source.indexOf(`pub struct ${name} {`)
  expect(start, `events.rs 里找不到 struct ${name}`).toBeGreaterThan(-1)
  const end = source.indexOf('\n}', start)
  expect(end, `struct ${name} 没有以 \n} 收尾`).toBeGreaterThan(start)
  const fields = source
    .slice(start, end)
    .split('\n')
    .map(line => line.trim())
    .filter(line => /^pub\s+[A-Za-z_][A-Za-z0-9_]*\s*:/.test(line))
    .map(line => camel(line.replace(/^pub\s+/, '').split(':')[0].trim()))
  expect(fields.length, `struct ${name} 一个字段都没解析到`).toBeGreaterThan(0)
  return fields
}

/** 取 `export interface NAME { … }` 的字段名（只认 `名字: 类型` 那几行）。 */
function tsFields(name: string): string[] {
  const source = codeOf(hostRaw)
  const start = source.indexOf(`export interface ${name} {`)
  expect(start, `plugin-host.ts 里找不到 interface ${name}`).toBeGreaterThan(-1)
  const end = source.indexOf('\n}', start)
  expect(end, `interface ${name} 没有以 \n} 收尾`).toBeGreaterThan(start)
  const fields = source
    .slice(start, end)
    .split('\n')
    .map(line => line.trim())
    .filter(line => /^[a-zA-Z][A-Za-z0-9_]*\s*:/.test(line))
    .map(line => line.split(':')[0].trim())
  expect(fields.length, `interface ${name} 一个字段都没解析到`).toBeGreaterThan(0)
  return fields
}

describe('灯态事件的载荷同源（需求 1 逐项）', () => {
  const rust = rustFields('AgentEvent')
  const ts = tsFields('PluginAgentEvent')

  it('Rust 那条事件与本仓作者看到的那份形状逐字段相等', () => {
    expect(sorted(ts), 'TS 的 PluginAgentEvent 与 Rust 的 AgentEvent 漂移了').toEqual(sorted(rust))
  })

  /** 需求点名的四类信息：不是「大概有」，是每类都要指到具体字段。 */
  it('点名要的字段都在：转灯前后、endpoint、提问、会话 id、时间', () => {
    const required = [
      // 状态灯转灯：新态 + 变前态 + 主题。
      'phase',
      'previousPhase',
      'topic',
      // endpoint（上游优先，没解析到时是 null）。
      'endpoint',
      // 提问原文：敏感字段，靠清单声明 `events:agent` 才拿得到。
      'prompt',
      // id：会话 id 与稳定的 key、平台名。
      'sessionId',
      'key',
      'provider',
      // 时间：事件时刻 + 本轮开始 + 最后一次看到。
      'ts',
      'startedAtMs',
      'lastSeenAtMs',
    ]
    for (const field of required) {
      expect(rust, `Rust 的 AgentEvent 没有 ${field}`).toContain(field)
      expect(ts, `作者侧的 PluginAgentEvent 没有 ${field}`).toContain(field)
    }
  })

  it('主题名与 Rust 的常量一致（作者按字符串订阅，名字漂了就永远收不到）', () => {
    const source = codeOf(eventsRaw)
    for (const topic of ['light_changed', 'prompt_updated', 'session_removed']) {
      expect(source, `events.rs 里没有主题常量 "${topic}"`).toContain(`"${topic}"`)
    }
  })
})

const sorted = (values: string[]) => [...values].sort()
