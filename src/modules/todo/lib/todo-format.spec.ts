import { describe, expect, it } from 'vitest'
import {
  dueBucket,
  dueTone,
  formatClockTime,
  formatDue,
  formatDueCompact,
  isOverdue,
  priorityLabel,
} from './todo-format'

/** 参照时刻：本地时间 2026-09-19 10:00（用本地构造，测试不依赖运行机器时区）。 */
const NOW = new Date(2026, 8, 19, 10, 0, 0).getTime()
/** 造一个本地时间戳：相对参照日偏移 `dayOffset` 天、`hour:minute` 时刻。 */
function at(dayOffset: number, hour: number, minute = 0): number {
  return new Date(2026, 8, 19 + dayOffset, hour, minute, 0, 0).getTime()
}

describe('priorityLabel', () => {
  it('按滴答的 5 / 3 / 1 取值给出中文标签，其余为空串', () => {
    expect(priorityLabel(5)).toBe('高')
    expect(priorityLabel(3)).toBe('中')
    expect(priorityLabel(1)).toBe('低')
    expect(priorityLabel(0)).toBe('')
    expect(priorityLabel(2)).toBe('')
  })
})

describe('isOverdue', () => {
  it('带时刻的任务：时刻已过即逾期', () => {
    expect(isOverdue(at(0, 9), NOW, true)).toBe(true)
    expect(isOverdue(at(0, 11), NOW, true)).toBe(false)
    expect(isOverdue(at(-1, 23), NOW, true)).toBe(true)
  })

  it('全天任务：当天不算逾期，过了那天才算（否则一过 00:00 就整日标红）', () => {
    expect(isOverdue(at(0, 0), NOW, false)).toBe(false)
    expect(isOverdue(at(0, 23), NOW, false)).toBe(false)
    expect(isOverdue(at(-1, 23), NOW, false)).toBe(true)
  })

  it('未设置截止永远不算逾期', () => {
    expect(isOverdue(null, NOW, true)).toBe(false)
    expect(isOverdue(null, NOW, false)).toBe(false)
  })
})

describe('dueBucket', () => {
  it('未设置截止时为 none', () => {
    expect(dueBucket(null, NOW, true)).toBe('none')
    expect(dueBucket(null, NOW, false)).toBe('none')
  })

  it('带时刻：当天更早的时刻算逾期，之后的算今天', () => {
    expect(dueBucket(at(-1, 23), NOW, true)).toBe('overdue')
    expect(dueBucket(at(0, 1), NOW, true)).toBe('overdue')
    expect(dueBucket(at(0, 23), NOW, true)).toBe('today')
    expect(dueBucket(at(1, 9), NOW, true)).toBe('tomorrow')
    expect(dueBucket(at(5, 9), NOW, true)).toBe('later')
  })

  it('全天任务：当天整天都算今天', () => {
    expect(dueBucket(at(0, 0), NOW, false)).toBe('today')
    expect(dueBucket(at(0, 23), NOW, false)).toBe('today')
    expect(dueBucket(at(-1, 23), NOW, false)).toBe('overdue')
    expect(dueBucket(at(1, 0), NOW, false)).toBe('tomorrow')
  })

  it('分档与计数同口径：逾期分档 ⇔ isOverdue', () => {
    for (const hasTime of [true, false]) {
      for (const offset of [-2, -1, 0, 1, 3]) {
        const due = at(offset, 9)
        expect(dueBucket(due, NOW, hasTime) === 'overdue').toBe(isOverdue(due, NOW, hasTime))
      }
    }
  })
})

describe('dueTone', () => {
  it('逾期红 / 今天黄 / 其余中性', () => {
    expect(dueTone('overdue')).toBe('red')
    expect(dueTone('today')).toBe('yellow')
    expect(dueTone('tomorrow')).toBe('muted')
    expect(dueTone('later')).toBe('muted')
    expect(dueTone('none')).toBe('muted')
  })
})

describe('formatDue', () => {
  it('未设置截止时为空串', () => {
    expect(formatDue(null, NOW, false)).toBe('')
  })

  it('带时刻时附加 HH:MM', () => {
    expect(formatDue(at(-2, 8, 5), NOW, true)).toBe('已逾期 08:05')
    expect(formatDue(at(0, 9, 30), NOW, true)).toBe('已逾期 09:30')
    expect(formatDue(at(0, 23, 30), NOW, true)).toBe('今天 23:30')
    expect(formatDue(at(1, 9), NOW, true)).toBe('明天 09:00')
  })

  it('只到天粒度时不写时分', () => {
    expect(formatDue(at(0, 23), NOW, false)).toBe('今天')
    expect(formatDue(at(3, 0), NOW, false)).toBe('09-22')
  })
})

describe('formatDueCompact', () => {
  it('悬浮窗用的紧凑文案不带时刻', () => {
    expect(formatDueCompact(at(-1, 23), NOW, true)).toBe('逾期')
    expect(formatDueCompact(at(0, 9), NOW, true)).toBe('逾期')
    expect(formatDueCompact(at(0, 23), NOW, true)).toBe('今天')
    expect(formatDueCompact(at(0, 0), NOW, false)).toBe('今天')
    expect(formatDueCompact(at(1, 1), NOW, true)).toBe('明天')
    expect(formatDueCompact(at(9, 1), NOW, true)).toBe('09-28')
  })
})

describe('formatClockTime', () => {
  it('格式化为 HH:MM:SS 并补零', () => {
    expect(formatClockTime(new Date(2026, 8, 19, 7, 5, 3).getTime())).toBe('07:05:03')
  })
})
