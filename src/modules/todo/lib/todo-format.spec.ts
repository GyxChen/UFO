import { describe, expect, it } from 'vitest'
import {
  countByPriority,
  dueBucket,
  dueTone,
  formatClockTime,
  formatDue,
  formatMonthDay,
  isOverdue,
  localDayStartMs,
  localUtcOffsetMinutes,
  priorityBucket,
  priorityGroups,
} from './todo-format'

/** 参照时刻：本地时间 2026-09-19 10:00（用本地构造，测试不依赖运行机器时区）。 */
const NOW = new Date(2026, 8, 19, 10, 0, 0).getTime()
/** 造一个本地时间戳：相对参照日偏移 `dayOffset` 天、`hour:minute` 时刻。 */
function at(dayOffset: number, hour: number, minute = 0): number {
  return new Date(2026, 8, 19 + dayOffset, hour, minute, 0, 0).getTime()
}

describe('priorityBucket', () => {
  it('5 / 3 归高 / 中，1 与 0（无优先级）都归低', () => {
    expect(priorityBucket(5)).toBe('high')
    expect(priorityBucket(3)).toBe('medium')
    expect(priorityBucket(1)).toBe('low')
    // 「无优先级」必须有归属：界面只给三面小旗，否则这些任务会凭空消失
    expect(priorityBucket(0)).toBe('low')
  })

  it('按区间判断：以后滴答补档（4 / 2）也不会掉队', () => {
    expect(priorityBucket(4)).toBe('medium')
    expect(priorityBucket(2)).toBe('low')
    expect(priorityBucket(9)).toBe('high')
  })
})

describe('priorityGroups / countByPriority', () => {
  it('分三档、顺序恒为高 → 中 → 低，标签与颜色成对', () => {
    const groups = priorityGroups([{ priority: 5 }, { priority: 0 }, { priority: 3 }])

    expect(groups.map(group => group.bucket)).toEqual(['high', 'medium', 'low'])
    expect(groups.map(group => group.label)).toEqual(['高', '中', '低'])
    expect(groups.map(group => group.tone)).toEqual(['red', 'yellow', 'blue'])
    expect(groups.map(group => group.tasks.length)).toEqual([1, 1, 1])
  })

  it('空档也占位（信息卡骨架不跳）', () => {
    const groups = priorityGroups([{ priority: 5 }])
    expect(groups.map(group => group.tasks.length)).toEqual([1, 0, 0])
  })

  it('三档计数之和 = 任务总数（无优先级不会漏掉）', () => {
    const tasks = [{ priority: 5 }, { priority: 5 }, { priority: 0 }, { priority: 1 }, { priority: 3 }]
    const counts = countByPriority(tasks)

    expect(counts).toEqual({ high: 2, medium: 1, low: 2 })
    expect(counts.high + counts.medium + counts.low).toBe(tasks.length)
  })

  it('空列表：三档全是 0', () => {
    expect(countByPriority([])).toEqual({ high: 0, medium: 0, low: 0 })
    expect(priorityGroups([]).map(group => group.tasks.length)).toEqual([0, 0, 0])
  })
})

describe('新建任务用的本机日期换算', () => {
  it('localDayStartMs 取当地当天零点', () => {
    expect(localDayStartMs(NOW)).toBe(new Date(2026, 8, 19, 0, 0, 0, 0).getTime())
  })

  it('localUtcOffsetMinutes 是「本地 = UTC + N」，与 getTimezoneOffset 反号', () => {
    expect(localUtcOffsetMinutes(NOW)).toBe(-new Date(NOW).getTimezoneOffset())
  })

  it('formatMonthDay 给出中文月日', () => {
    expect(formatMonthDay(at(0, 13))).toBe('9月19日')
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

describe('formatClockTime', () => {
  it('格式化为 HH:MM:SS 并补零', () => {
    expect(formatClockTime(new Date(2026, 8, 19, 7, 5, 3).getTime())).toBe('07:05:03')
  })
})
