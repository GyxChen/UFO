/**
 * 转灯日志的纯逻辑口径：色调、被拒判定、灯态串、行重建。
 *
 * 这几条都直接影响「用户能不能从一行字里看出灯为什么没动」，所以按语义断言而不是快照。
 */
import { describe, expect, it } from 'vitest'
import {
  STATUS_LOG_EVENT,
  formatLogClock,
  formatLogClockMillis,
  isRejected,
  logDetailNote,
  logHeadline,
  logPhaseChange,
  logTone,
  logWireLine,
  reasonExplain,
  type LogEntry,
} from './status-log'

/** 造一条最小记录：只填被测字段。 */
function entry(patch: Partial<LogEntry> = {}): LogEntry {
  return { ts_ms: 1_790_000_000_000, writer: 'monitor', stage: 'kernel_verdict', ...patch }
}

describe('转灯日志的行语义', () => {
  it('灯变与拒绝各归自己的色调，不靠颜色单独表达', () => {
    expect(logTone(entry({ stage: 'light_transition', reason: 'phase_changed' }))).toBe('ok')
    expect(logTone(entry({ stage: 'kernel_verdict', reason: 'duplicate_in_seen_window' }))).toBe('warn')
    // 摘灯是「灯凭空消失」那条线，走红；解析失败也走红（写失败 = 线索断了）。
    expect(logTone(entry({ stage: 'session_reaped', reason: 'running_timeout' }))).toBe('err')
    expect(logTone(entry({ stage: 'log_maintenance', reason: 'log_write_failed' }))).toBe('err')
    // 上游反查这类旁路记录不属于任何一次裁决：不给彩条，免得看成「出事了」。
    expect(logTone(entry({ stage: 'upstream_resolved', reason: 'resolver_hit' }))).toBe('plain')
  })

  it('「只看被拒」捞的是没落成灯变化的那些，灯变本身不算被拒', () => {
    expect(isRejected(entry({ stage: 'compat_verdict', reason: 'adapter_ignored_event' }))).toBe(true)
    expect(isRejected(entry({ stage: 'kernel_verdict', reason: 'stale_sequence' }))).toBe(true)
    // 同一环节里的例外：采纳且真变了灯的（phase_changed / flash_settled）不该被过滤掉。
    expect(isRejected(entry({ stage: 'kernel_verdict', reason: 'phase_changed' }))).toBe(false)
    expect(isRejected(entry({ stage: 'light_transition', reason: 'phase_changed' }))).toBe(false)
    expect(isRejected(entry({ stage: 'hook_attempt', reason: 'hook_completed' }))).toBe(false)
  })

  it('灯态串缺任一端就不硬凑箭头', () => {
    expect(logPhaseChange(entry({ phase_from: 'running', phase_to: 'waiting' }))).toBe('绿→黄')
    expect(logPhaseChange(entry({ phase_to: 'failed' }))).toBe('—→红')
    expect(logPhaseChange(entry())).toBe('')
    // 认不出的灯态原样显示：猜一个颜色会把「不知道」伪装成「知道」。
    expect(logPhaseChange(entry({ phase_from: 'weird', phase_to: 'running' }))).toBe('weird→绿')
  })

  it('detail 只拼标量，嵌套对象不进摘要', () => {
    const note = logDetailNote(entry({ detail: { new_bytes: 41208, records: 3, nested: { a: 1 } } }))
    expect(note).toBe('new_bytes=41208 · records=3')
    expect(logDetailNote(entry({ detail: { nested: { a: 1 } } }))).toBe('')
    expect(logDetailNote(entry({ kind: 'turn_finished' }))).toBe('kind=turn_finished')
  })

  it('重建的线协议一行不含内存游标 seq，也不含缺省字段', () => {
    const line = logWireLine(entry({ stage: 'rollout_activity', reason: 'activity_implies_running' }))
    const parsed = JSON.parse(line)
    expect(Object.keys(parsed)).toEqual(['ts_ms', 'writer', 'stage', 'reason'])
    // ⚠️ `seq` 是本进程的环形游标，磁盘上没有它 —— 文案不能说成「逐字原文」。
    expect(parsed.seq).toBeUndefined()
    expect(JSON.parse(logWireLine(entry({ trace_id: 'c4-1' })) as string).trace_id).toBe('c4-1')
  })

  it('没有理由时头句退回环节说明，而不是渲染成空串', () => {
    const head = logHeadline(entry({ stage: 'inbox_batch' }))
    expect(head).toBe('主进程读了一批 inbox（含跳过与清扫）')
    expect(reasonExplain('duplicate_in_seen_window')).toBe('去重窗口内已见过同一个 event_id')
    // 纯记录性条目没有理由：这里必须返回空串，由调用方决定省略这段还是换环节说明。
    expect(reasonExplain(undefined)).toBe('')
  })

  it('时间列按本地时区，毫秒列补三位', () => {
    // 固定偏移下断言格式而不是具体时刻：CI 的时区不该决定测试结论。
    const clock = formatLogClock(1_790_000_000_000)
    expect(clock).toMatch(/^\d{2}:\d{2}:\d{2}$/)
    expect(formatLogClockMillis(1_790_000_000_000)).toMatch(/^\d{2}:\d{2}:\d{2}\.\d{3}$/)
    expect(formatLogClock(Number.NaN)).toBe('—')
  })

  it('事件名常量与 Rust 的 STATUS_LOG_EVENT 同名（完整同源检查在 reasons spec）', () => {
    expect(STATUS_LOG_EVENT).toBe('agent-status-log')
  })
})
