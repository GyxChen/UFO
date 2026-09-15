import { invoke } from '@tauri-apps/api/core'

/** 后端广播档案变更的事件名，与 Rust `ENDPOINT_PROFILES_EVENT` 一致。 */
export const ENDPOINT_PROFILES_EVENT = 'endpoint-profiles'

/**
 * 一条 endpoint 档案（后端 `endpoint_profiles.rs` 的 `EndpointProfile`）。
 * 是「信息卡拓展」左侧列表的行，也是拓展字段的主键载体。
 */
export interface EndpointProfile {
  /** 去重键：`host:port`（URL 归一化后的结果），删除时要回传给后端。 */
  id: string
  /** 首次登记时的原始地址，列表直接展示这一串。 */
  url: string
  /** 首次发现时间（毫秒时间戳），列表按它升序。 */
  first_seen_at_ms: number
  /** 拓展字段容器；本轮恒为空对象，后续按 endpoint 配置 tooltip 字段时填充。 */
  extensions: Record<string, unknown>
}

/** 档案集合；`revision` 用于丢弃乱序到达的事件。 */
export interface EndpointProfileList {
  schema_version: number
  revision: number
  profiles: EndpointProfile[]
}

/** 空集合，供后端不可用时兜底，避免 UI 因 null 分支崩溃。 */
export function emptyProfileList(): EndpointProfileList {
  return { schema_version: 1, revision: 0, profiles: [] }
}

/** 读取已登记的 endpoint 档案。 */
export function fetchEndpointProfiles(): Promise<EndpointProfileList> {
  return invoke<EndpointProfileList>('get_endpoint_profiles')
}

/** 手动登记一条 endpoint；后端拒绝（本地代理地址 / 非法）时抛错，由调用方提示。 */
export function addEndpointProfile(url: string): Promise<EndpointProfileList> {
  return invoke<EndpointProfileList>('add_endpoint_profile', { url })
}

/** 删除一条档案。 */
export function deleteEndpointProfile(id: string): Promise<EndpointProfileList> {
  return invoke<EndpointProfileList>('delete_endpoint_profile', { id })
}

/**
 * 校验 `get_endpoint_profiles` / 事件 payload 的形态。
 *
 * 只做结构守卫（字段类型、数组元素合法），不校验 URL —— 归一化口径在后端，
 * 前端重复解析只会出现两套规则。非法 payload 一律忽略，保持上一次已知状态。
 */
export function isEndpointProfileList(value: unknown): value is EndpointProfileList {
  if (!value || typeof value !== 'object') return false
  const list = value as Partial<EndpointProfileList>
  if (!Number.isSafeInteger(list.revision) || (list.revision ?? 0) < 0) return false
  if (!Array.isArray(list.profiles)) return false
  return list.profiles.every(
    (profile: unknown) =>
      !!profile &&
      typeof profile === 'object' &&
      typeof (profile as EndpointProfile).id === 'string' &&
      (profile as EndpointProfile).id.length > 0 &&
      typeof (profile as EndpointProfile).url === 'string' &&
      typeof (profile as EndpointProfile).first_seen_at_ms === 'number',
  )
}

/**
 * 把毫秒时间戳格式化成列表右侧的紧凑时间（`MM-DD HH:mm`）。
 *
 * 不写年份：档案数量少、都是近期发现的，年份只会挤占窄列。
 * 非法/缺失值返回空串，由调用方省略该列。
 */
export function formatProfileTime(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return ''
  const date = new Date(ms)
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`
}
