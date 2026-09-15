import { invoke } from '@tauri-apps/api/core'
import type { Provider, TokenTotals } from '@/lib/agent-status'

/** 后端广播存档变更的事件名，与 Rust `SESSION_RECORDS_EVENT` 一致。 */
export const SESSION_RECORDS_EVENT = 'session-records'

/**
 * 一条会话存档（后端 `session_store.rs` 的 `SessionRecord`）。
 * 是「会话记录」页左侧列表的行；只含跨重启仍有意义的字段，不含灯色等存活态。
 */
export interface SessionRecord {
  /** 主键 = `provider:session_id`（如 `claude:<uuid>`），删除时回传给后端。 */
  key: string
  /** 来源 provider。 */
  provider: Provider
  /** 原始会话标识（不含 provider 前缀）。 */
  session_id: string
  /** 用户最近一次提问原文；可能为 null（尚未采集到）。⚠️ 明文长期驻留磁盘。 */
  prompt: string | null
  /** 会话使用的 API endpoint（存 base_url，不含反查上游）。 */
  base_url: string | null
  /** 会话最早创建时刻（毫秒）：回填它让指示灯排序不跳。 */
  started_at_ms: number
  /** 最后一次活动时刻（毫秒）：列表按它降序，淘汰与 30 天过期也按它算。 */
  last_seen_at_ms: number
  /** 累计 token；未统计到时为 null。 */
  tokens: TokenTotals | null
}

/** 存档集合；`revision` 用于丢弃乱序到达的事件。 */
export interface SessionRecordList {
  schema_version: number
  revision: number
  records: SessionRecord[]
}

/** 空集合，供后端不可用时兜底，避免 UI 因 null 分支崩溃。 */
export function emptyRecordList(): SessionRecordList {
  return { schema_version: 1, revision: 0, records: [] }
}

/** 读取持久化的会话存档列表。 */
export function fetchSessionRecords(): Promise<SessionRecordList> {
  return invoke<SessionRecordList>('get_session_records')
}

/** 删除一条会话存档；后端拒绝（未知 key）时抛错，由调用方提示。 */
export function deleteSessionRecord(key: string): Promise<SessionRecordList> {
  return invoke<SessionRecordList>('delete_session_record', { key })
}

/** 清空全部存档。⚠️ 只删归档、不动存活会话，运行中的会话会在下一个事件后重新出现。 */
export function clearSessionRecords(): Promise<SessionRecordList> {
  return invoke<SessionRecordList>('clear_session_records')
}

/**
 * 校验 `get_session_records` / 事件 payload 的形态。
 *
 * 只做结构守卫（字段类型、数组元素合法），不在前端重复 provider 归一 —— 口径在后端。
 * 非法 payload 一律忽略，保持上一次已知状态（与 `isEndpointProfileList` 同一策略）。
 */
export function isSessionRecordList(value: unknown): value is SessionRecordList {
  if (!value || typeof value !== 'object') return false
  const list = value as Partial<SessionRecordList>
  if (!Number.isSafeInteger(list.revision) || (list.revision ?? 0) < 0) return false
  if (!Array.isArray(list.records)) return false
  return list.records.every((record: unknown) => {
    if (!record || typeof record !== 'object') return false
    const item = record as SessionRecord
    return (
      typeof item.key === 'string' &&
      item.key.length > 0 &&
      (item.provider === 'claude' || item.provider === 'codex') &&
      typeof item.session_id === 'string' &&
      typeof item.started_at_ms === 'number' &&
      typeof item.last_seen_at_ms === 'number'
    )
  })
}

/**
 * 把毫秒时间戳格式化成列表右侧的紧凑时间（`MM-DD HH:mm`）。
 *
 * 复用信息卡拓展的同一读法：不写年份（都是近期），非法/缺失返回空串由调用方省略。
 */
export function formatRecordTime(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return ''
  const date = new Date(ms)
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`
}
