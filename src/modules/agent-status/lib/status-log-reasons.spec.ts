/**
 * 护栏：转灯日志的环节 / 理由文案必须与 Rust 枚举同源。
 *
 * 手法照 `push-providers.spec.ts`：`?raw` 读 Rust 源，正则抠出变体名再比对。
 * 这里要挡的是两种真实的腐坏 ——
 * ① Rust 加了一个 stage/reason，前端查不到它是谁（日志里出现一个陌生英文词）；
 * ② 前端留着一个 Rust 已经删掉的键（那条理由永远不会出现，读代码的人却以为会）。
 * 线协议取值是 `#[serde(rename_all = "snake_case")]` 的结果，所以测试自己也按同一规则换算。
 */
import { describe, expect, it } from 'vitest'
import recordRaw from '../../../../src-tauri/src/modules/agent_status/status/log/record.rs?raw'
import logModRaw from '../../../../src-tauri/src/modules/agent_status/status/log/mod.rs?raw'
import { REASON_EXPLAIN, STAGE_EXPLAIN, STAGE_LABELS, STATUS_LOG_EVENT } from './status-log'

/** 抠出 `pub enum X { ... }` 块里的变体名（跳过注释与整行注释掉的分支）。 */
function rustVariants(source: string, enumName: string): string[] {
  const block = new RegExp(`pub enum ${enumName} \\{([\\s\\S]*?)\\n\\}`).exec(source)?.[1]
  if (!block) throw new Error(`没能在 record.rs 里找到 ${enumName} —— 它改名或挪文件了`)
  return block
    .split('\n')
    .map(line => line.trim())
    // 只认「标识符 + 可选逗号」这种变体行：注释、空行、属性行都不是。
    .filter(line => /^[A-Z][A-Za-z0-9]*,?$/.test(line))
    .map(line => line.replace(/,$/, ''))
}

/** Rust 变体名 → serde 的 snake_case 线协议取值。 */
function toWire(name: string): string {
  return name.replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase()
}

describe('转灯日志文案与 Rust 枚举同源', () => {
  it('环节清单两边一条不多一条不少', () => {
    const rust = rustVariants(recordRaw, 'LogStage').map(toWire)
    const ts = Object.keys(STAGE_LABELS)
    expect(ts).toHaveLength(rust.length)
    expect(new Set(ts)).toEqual(new Set(rust))
  })

  it('理由清单两边一条不多一条不少', () => {
    const rust = rustVariants(recordRaw, 'LogReason').map(toWire)
    const ts = Object.keys(REASON_EXPLAIN)
    // 这条断言同时也是「后端加了黑洞却没写解释」的探测器：漏一个就查不出灯为什么不动。
    expect(ts).toHaveLength(rust.length)
    expect(new Set(ts)).toEqual(new Set(rust))
  })

  it('每个环节都有短标签与整句说明，且都不为空', () => {
    for (const stage of Object.keys(STAGE_LABELS)) {
      expect(STAGE_LABELS[stage as keyof typeof STAGE_LABELS].trim(), `环节 ${stage} 缺短标签`).not.toBe('')
      expect(STAGE_EXPLAIN[stage as keyof typeof STAGE_EXPLAIN].trim(), `环节 ${stage} 缺说明`).not.toBe('')
    }
    for (const reason of Object.keys(REASON_EXPLAIN)) {
      expect(REASON_EXPLAIN[reason as keyof typeof REASON_EXPLAIN].trim(), `理由 ${reason} 缺解释`).not.toBe('')
    }
  })

  it('短标签互不重复：下拉里出现两个同名选项就没法选', () => {
    const labels = Object.values(STAGE_LABELS)
    expect(new Set(labels).size).toBe(labels.length)
  })

  it('广播事件名与 Rust 常量一致', () => {
    // Rust 侧：`pub const STATUS_LOG_EVENT: &str = "agent-status-log";`
    const rust = /STATUS_LOG_EVENT: &str = "([^"]+)"/.exec(logModRaw)?.[1]
    expect(rust, 'Rust 里的事件名常量被改名或删除').toBe(STATUS_LOG_EVENT)
  })
})
