/**
 * 待办组件的纯逻辑：优先级三档（高 / 中 / 低）、截止时间的相对描述与语义色，
 * 以及新建任务要用的本机时区日期换算。
 *
 * 与视图 / store 完全解耦，便于单测。所有「今天 / 明天」的判断都落在**本机时区**，
 * 因此刻意放在前端：Rust 侧只把滴答的时间戳转成绝对的 UTC 毫秒（与时区无关），
 * 相对日期由浏览器按用户所在时区算 —— 后端不猜用户时区，也就不会猜错。
 */

/** 滴答 OpenAPI 的优先级取值：5 高 / 3 中 / 1 低 / 0 无。 */
export const PRIORITY_HIGH = 5
export const PRIORITY_MEDIUM = 3
export const PRIORITY_LOW = 1

/** 截止时间的紧迫程度分档。 */
export type DueBucket = 'overdue' | 'today' | 'tomorrow' | 'later' | 'none'

/** 语义色名：与 `todo.css` 里的 `--red / --yellow / --muted` 修饰类一一对应。 */
export type DueTone = 'red' | 'yellow' | 'muted'

/** 取某个毫秒时间戳所在「本地日」的 00:00 时刻，用于按天比较。 */
function startOfLocalDay(ms: number): number {
  const date = new Date(ms)
  date.setHours(0, 0, 0, 0)
  return date.getTime()
}

/**
 * 该任务此刻是否已逾期。
 *
 * 逾期口径按任务粒度分两种（两处视图共用，不许各算一套 —— 否则会出现
 * 「徽标变红但信息卡写着 0 项逾期」）：
 * - 带时刻的任务：截止时刻已过即逾期（`今天 09:00` 到了 10:00 就是欠账）；
 * - 全天任务：滴答给的截止是当天 00:00，按时刻比较会让它从凌晨起整日标红，
 *   所以当天一整天都不算逾期，过了那一天才算。
 *
 * @param dueAtMs 截止毫秒时间戳；null = 未设置截止（不算逾期）
 * @param nowMs 参考时刻
 * @param hasTime 是否有具体时刻（false = 只到天粒度的全天任务）
 * @returns 已逾期时为 true
 */
export function isOverdue(dueAtMs: number | null, nowMs: number, hasTime: boolean): boolean {
  if (dueAtMs == null) return false
  if (hasTime) return dueAtMs < nowMs
  return startOfLocalDay(dueAtMs) < startOfLocalDay(nowMs)
}

/**
 * 把截止时间分档：已逾期 / 今天 / 明天 / 更远 / 未设置。
 *
 * 逾期一律走 [`isOverdue`]，分档（决定颜色与文案）与计数共用同一套口径。
 *
 * @param dueAtMs 截止毫秒时间戳；null = 该任务没有截止时间
 * @param nowMs 参考时刻（用快照的 `updated_at_ms`，保证同一次渲染里所有任务口径一致）
 * @param hasTime 是否有具体时刻（口径见 [`isOverdue`]）
 * @returns 紧迫程度分档
 */
export function dueBucket(dueAtMs: number | null, nowMs: number, hasTime: boolean): DueBucket {
  if (dueAtMs == null) return 'none'
  if (isOverdue(dueAtMs, nowMs, hasTime)) return 'overdue'
  const dayDiff = Math.round((startOfLocalDay(dueAtMs) - startOfLocalDay(nowMs)) / 86_400_000)
  if (dayDiff === 0) return 'today'
  if (dayDiff === 1) return 'tomorrow'
  return 'later'
}

/**
 * 分档 → 语义色。逾期红、今天黄、其余中性 —— 与状态灯语义保持一致。
 *
 * @param bucket `dueBucket` 的分档结果
 * @returns 语义色名
 */
export function dueTone(bucket: DueBucket): DueTone {
  if (bucket === 'overdue') return 'red'
  if (bucket === 'today') return 'yellow'
  return 'muted'
}

/** 把数字补成两位（9 → "09"）。 */
function pad2(value: number): string {
  return value < 10 ? `0${value}` : String(value)
}

/**
 * 把截止时间格式化成中文相对日期（可带时刻）。
 *
 * 规则（按本地时区的「天差」算）：
 * - 已逾期 → `已逾期` / `已逾期 08:00`
 * - 今天 → `今天` / `今天 09:00`
 * - 明天 → `明天` / `明天 09:00`
 * - 更远 → `MM-DD` / `MM-DD 09:00`
 *
 * @param dueAtMs 截止毫秒时间戳；null = 未设置截止，返回空串
 * @param nowMs 参考时刻
 * @param hasTime 该任务是否有具体时刻（否则只到天粒度，不显示时分）
 * @returns 可直接展示的中文文案（未设置截止时为空串）
 */
export function formatDue(dueAtMs: number | null, nowMs: number, hasTime: boolean): string {
  if (dueAtMs == null) return ''
  const due = new Date(dueAtMs)
  const time = hasTime ? ` ${pad2(due.getHours())}:${pad2(due.getMinutes())}` : ''
  switch (dueBucket(dueAtMs, nowMs, hasTime)) {
    case 'overdue':
      return `已逾期${time}`
    case 'today':
      return `今天${time}`
    case 'tomorrow':
      return `明天${time}`
    default:
      return `${pad2(due.getMonth() + 1)}-${pad2(due.getDate())}${time}`
  }
}

/**
 * 把毫秒时间戳格式化成 `HH:MM:SS`，用于信息卡的「更新时间」标注。
 *
 * @param ms 毫秒时间戳
 * @returns 24 小时制的时钟文本
 */
export function formatClockTime(ms: number): string {
  const date = new Date(ms)
  return `${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(date.getSeconds())}`
}

/** 优先级三档：界面（悬浮窗小旗 / 信息卡）只分这三档。 */
export type PriorityBucket = 'high' | 'medium' | 'low'

/** 三档的展示顺序：高 → 中 → 低（悬浮窗小旗与信息卡里的勾选框配色都按它排）。 */
export const PRIORITY_BUCKETS: readonly PriorityBucket[] = ['high', 'medium', 'low']

/** 优先级语义色名：与 `todo.css` 里的 `--red / --yellow / --blue` 修饰类一一对应。 */
export type PriorityTone = 'red' | 'yellow' | 'blue'

/**
 * 优先级数值 → 三档（高 / 中 / 低）。
 *
 * 滴答的取值是 5 高 / 3 中 / 1 低 / 0 无。**「无优先级」归入「低」**：界面只给三面
 * 小旗，必须让三档计数之和等于未完成总数 —— 否则没标优先级的任务会在这块视图里
 * 凭空消失（用户看到「一个任务都没有」，实际是有的）。取值用区间判断而不是等值匹配，
 * 将来滴答新增档位（如 4 / 2）也不会掉出去。
 *
 * @param priority 滴答的优先级数值（0 / 1 / 3 / 5）
 * @returns 所属档位
 */
export function priorityBucket(priority: number): PriorityBucket {
  if (priority >= PRIORITY_HIGH) return 'high'
  if (priority >= PRIORITY_MEDIUM) return 'medium'
  return 'low'
}

/**
 * 三档 → 中文标签。
 *
 * @param bucket 优先级档位
 * @returns 「高 / 中 / 低」
 */
export function priorityBucketLabel(bucket: PriorityBucket): string {
  switch (bucket) {
    case 'high':
      return '高'
    case 'medium':
      return '中'
    default:
      return '低'
  }
}

/**
 * 三档 → 语义色名（高 = 红 / 中 = 黄 / 低 = 蓝）。
 *
 * @param bucket 优先级档位
 * @returns 语义色名，供视图拼 `--red / --yellow / --blue` 修饰类
 */
export function priorityTone(bucket: PriorityBucket): PriorityTone {
  switch (bucket) {
    case 'high':
      return 'red'
    case 'medium':
      return 'yellow'
    default:
      return 'blue'
  }
}

/**
 * 按三档统计未完成任务数。
 *
 * @param tasks 任务列表（只读 `priority` 字段）
 * @returns 高 / 中 / 低三档各自的条数（三数之和 = 传入任务总数）
 */
export function countByPriority(tasks: readonly { priority: number }[]): Record<PriorityBucket, number> {
  const counts: Record<PriorityBucket, number> = { high: 0, medium: 0, low: 0 }
  for (const task of tasks) counts[priorityBucket(task.priority)] += 1
  return counts
}

/** 一档优先级的分组视图模型：档位 + 文案 + 颜色 + 该档任务。 */
export interface PriorityGroup<T> {
  /** 档位 */
  bucket: PriorityBucket
  /** 中文标签（高 / 中 / 低） */
  label: string
  /** 语义色名 */
  tone: PriorityTone
  /** 该档的任务（保持传入顺序） */
  tasks: T[]
}

/**
 * 把任务按三档分组，顺序固定为高 → 中 → 低。
 *
 * 空档也会返回（信息卡要显示「高 0 项」这种一致的骨架，列表才不会跳）。
 *
 * @param tasks 任务列表（只读 `priority` 字段）
 * @returns 三档分组，长度恒为 3
 */
export function priorityGroups<T extends { priority: number }>(tasks: readonly T[]): PriorityGroup<T>[] {
  const buckets: Record<PriorityBucket, T[]> = { high: [], medium: [], low: [] }
  for (const task of tasks) buckets[priorityBucket(task.priority)].push(task)

  return PRIORITY_BUCKETS.map(bucket => ({
    bucket,
    label: priorityBucketLabel(bucket),
    tone: priorityTone(bucket),
    tasks: buckets[bucket],
  }))
}

/**
 * 本机时区「今天 00:00」的毫秒时间戳（新建任务的默认截止）。
 *
 * @param nowMs 参考时刻，默认当前
 * @returns 当地当天零点的毫秒时间戳
 */
export function localDayStartMs(nowMs: number = Date.now()): number {
  return startOfLocalDay(nowMs)
}

/**
 * 本机时区相对 UTC 的分钟偏移（北京 +0800 → 480）。
 *
 * `Date.getTimezoneOffset()` 返回的是「UTC − 本地」（+0800 时给 -480），这里取反成
 * 「本地 = UTC + N 分钟」的口径，后端按它把当地零点写成带偏移的日期串
 * （滴答要 `2026-09-20T00:00:00+0800` 这种形态，写错时区会让「今天」变成昨天 / 明天）。
 *
 * @param nowMs 参考时刻，默认当前
 * @returns 分钟偏移（东八区为 480）
 */
export function localUtcOffsetMinutes(nowMs: number = Date.now()): number {
  return -new Date(nowMs).getTimezoneOffset()
}

/**
 * 把毫秒时间戳格式化成「M月D日」，用于设置页标注新建任务的默认截止。
 *
 * @param ms 毫秒时间戳
 * @returns 中文月日文案
 */
export function formatMonthDay(ms: number): string {
  const date = new Date(ms)
  return `${date.getMonth() + 1}月${date.getDate()}日`
}
