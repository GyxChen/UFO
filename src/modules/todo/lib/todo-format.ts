/**
 * 待办组件的纯格式化逻辑：优先级文案、截止时间的相对描述与语义色。
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

/**
 * 优先级 → 中文标签。
 *
 * @param priority 滴答的优先级数值（0 / 1 / 3 / 5）
 * @returns 「高 / 中 / 低」，无优先级返回空串（界面据此不渲染这个角标）
 */
export function priorityLabel(priority: number): string {
  switch (priority) {
    case PRIORITY_HIGH:
      return '高'
    case PRIORITY_MEDIUM:
      return '中'
    case PRIORITY_LOW:
      return '低'
    default:
      return ''
  }
}

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
 * 悬浮窗专用的紧凑截止文案（不带时刻）。
 *
 * 悬浮窗按 1 格宽 / 一格高排版，`已逾期 08:00` 这种带时刻的写法会把任务标题挤没，
 * 所以这里只保留「哪天」这一层信息；要精确到分钟就悬停看信息卡。
 *
 * @param dueAtMs 截止毫秒时间戳
 * @param nowMs 参考时刻
 * @param hasTime 是否有具体时刻（口径见 `isOverdue`）
 * @returns `逾期` / `今天` / `明天` / `MM-DD`
 */
export function formatDueCompact(dueAtMs: number, nowMs: number, hasTime: boolean): string {
  switch (dueBucket(dueAtMs, nowMs, hasTime)) {
    case 'overdue':
      return '逾期'
    case 'today':
      return '今天'
    case 'tomorrow':
      return '明天'
    default: {
      const due = new Date(dueAtMs)
      return `${pad2(due.getMonth() + 1)}-${pad2(due.getDate())}`
    }
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
