import { invoke } from '@tauri-apps/api/core'
import type { EndpointProfileList } from '@/lib/endpoint-profiles'

/** 后端广播流程脚本结果的事件名，与 Rust `FLOW_RESULTS_EVENT` 一致。 */
export const FLOW_RESULTS_EVENT = 'flow-results'

/** 拓展字段的语义色；与状态灯同一套命名（muted 表示不强调）。 */
export type FlowColor = 'green' | 'yellow' | 'red' | 'muted'

/** 脚本返回的单个拓展字段（后端 `flow::FlowField`）。 */
export interface FlowField {
  /** 展示标签（后端截断到上限）。 */
  label: string
  /** 展示值（后端截断到上限，可能是数字/布尔的文本形态）。 */
  value: string
  /** 语义色；非法值后端已清洗为 null。 */
  color: FlowColor | null
}

/** 一个 endpoint 最近一次运行的产物（后端 `flow::FlowOutcome`）。 */
export interface FlowOutcome {
  /** 清洗后的字段（成功时有值；失败时为空）。 */
  fields: FlowField[]
  /** 上次失败的简短原因；成功时 null。 */
  error: string | null
  /** 本次结果的生成时刻（毫秒）。 */
  updated_at_ms: number
  /** 上次运行是否成功。 */
  ok: boolean
}

/**
 * 一个会话应展示的拓展字段 + 新鲜度（后端 `flow::FlowSessionResult`）。
 *
 * 只给字段的话用户无从判断屏上的余额/用量是刚跑的还是旧值，因此结果额外带生成时刻，
 * tooltip 据此标注「更新于 HH:MM:SS」；`interval_sec` 仍走协议 + 严格校验，
 * 但仅作后端调度/退避用，前端已不展示（按需求移除「每 Ns 刷新」）。
 */
export interface FlowSessionResult {
  /** 清洗后的拓展字段。 */
  fields: FlowField[]
  /** 本批字段的生成时刻（毫秒）。 */
  updated_at_ms: number
  /** 配置的轮询间隔（秒）；退避只拉长实际间隔，标注取配置原值。 */
  interval_sec: number
}

/**
 * 广播快照：按**会话 key** 索引该会话 endpoint 的字段与新鲜度（后端 `flow::FlowSnapshot`）。
 *
 * 用会话 key 而非 endpoint id 做键，让 tooltip 零归一逻辑（它手里只有会话 key）；
 * endpoint 归一口径只在后端算一次。
 */
export interface FlowSnapshot {
  /** 每次内容变化递增，供前端丢弃乱序事件。 */
  revision: number
  /** 会话 key → 该会话 endpoint 的拓展结果（无字段/未启用的会话不进表）。 */
  sessions: Record<string, FlowSessionResult>
}

/**
 * 主窗口保存的流程配置（后端 `flow::FlowConfig`）。
 *
 * ⚠️ 字段走 snake_case：外层命令参数由 Tauri 做 camelCase→snake_case，但**嵌套结构体**的内层字段
 * 是 serde 直接按 Rust 字段名序列化的，必须与后端一致写 snake_case（`interval_sec` 而非 `intervalSec`）。
 * ⚠️ 只带客户端该管的四项：`secret_names` / `script_hash` 由服务端权威维护（保存路径会覆盖它们），
 * 前端不传，避免漏传把凭据记账清空。
 */
export interface FlowConfigPayload {
  enabled: boolean
  interval_sec: number
  timeout_ms: number
  script: string
}

/** 「运行测试」的同步产物（后端 `flow::FlowTestResult`）。 */
export interface FlowTestResult {
  fields: FlowField[]
  logs: string[]
  error: string | null
}

/** 空快照，供后端不可用时兜底。 */
export function emptyFlowSnapshot(): FlowSnapshot {
  return { revision: 0, sessions: {} }
}

/** 读某 endpoint 的运行产物（未跑过时后端返回 null）。 */
export function fetchFlowResults(endpointId: string): Promise<FlowOutcome | null> {
  return invoke<FlowOutcome | null>('get_flow_results', { endpointId })
}

/** 读当前「会话 key → 字段」映射（tooltip 初次挂载时拉一次，之后靠事件）。 */
export function fetchFlowSnapshot(): Promise<FlowSnapshot> {
  return invoke<FlowSnapshot>('get_flow_snapshot')
}

/** 保存某 endpoint 的流程配置（仅主窗口）；返回新的档案列表。 */
export function saveEndpointFlow(
  endpointId: string,
  config: FlowConfigPayload,
): Promise<EndpointProfileList> {
  return invoke<EndpointProfileList>('save_endpoint_flow', { endpointId, config })
}

/** 同步跑一次脚本供预览（仅主窗口）。 */
export function runFlowTest(
  endpointId: string,
  script: string,
  timeoutMs?: number,
): Promise<FlowTestResult> {
  return invoke<FlowTestResult>('run_flow_test', { endpointId, script, timeoutMs })
}

/** 写一个密钥到系统凭据管理器（仅主窗口）。值不落盘、不回显。 */
export function setFlowSecret(endpointId: string, name: string, value: string): Promise<void> {
  return invoke('set_flow_secret', { endpointId, name, value })
}

/** 删一个密钥（仅主窗口）。 */
export function deleteFlowSecret(endpointId: string, name: string): Promise<void> {
  return invoke('delete_flow_secret', { endpointId, name })
}

/**
 * 校验 `get_flow_snapshot` / 事件 payload 的形态。
 *
 * `flow-results` 是新通道，没有旧 hook 兼容包袱，因此**整份严格校验**：任一字段非法即判整份无效，
 * 保持上一次已知状态（与 `isSessionRecordList` 同一策略，比 `isStatusSnapshot` 的可选字段宽松更严）。
 */
export function isFlowSnapshot(value: unknown): value is FlowSnapshot {
  if (!value || typeof value !== 'object') return false
  const snapshot = value as Partial<FlowSnapshot>
  if (!Number.isSafeInteger(snapshot.revision) || (snapshot.revision ?? 0) < 0) return false
  if (!snapshot.sessions || typeof snapshot.sessions !== 'object') return false
  return Object.values(snapshot.sessions).every(
    (result: unknown) =>
      !!result &&
      typeof result === 'object' &&
      Array.isArray((result as FlowSessionResult).fields) &&
      (result as FlowSessionResult).fields.every((field) => isFlowField(field ?? undefined)) &&
      Number.isSafeInteger((result as FlowSessionResult).updated_at_ms) &&
      Number.isFinite((result as FlowSessionResult).interval_sec),
  )
}

/** 单个字段的结构守卫（label/value 必须是字符串，color 允许 null 或已知枚举）。 */
export function isFlowField(value: unknown): value is FlowField {
  if (!value || typeof value !== 'object') return false
  const field = value as FlowField
  return (
    typeof field.label === 'string' &&
    typeof field.value === 'string' &&
    (field.color === null ||
      field.color === undefined ||
      field.color === 'green' ||
      field.color === 'yellow' ||
      field.color === 'red' ||
      field.color === 'muted')
  )
}

/** 一条 endpoint 档案的 extensions 里 flow 键的持久化形态（与 Rust `FlowConfig` 字段一一对应）。 */
export interface FlowConfigStored {
  enabled: boolean
  interval_sec: number
  timeout_ms: number
  script: string
  script_hash: number
  secret_names: string[]
}

/**
 * 从档案 extensions 里读出 flow 配置；缺失/结构不符返回 null（编辑器按默认值起步）。
 *
 * 只认 `flow` 键下形状正确的对象，其余字段用默认值兜底，避免手改坏配置卡住 UI。
 */
export function flowFromExtensions(
  extensions: Record<string, unknown> | undefined,
): FlowConfigStored | null {
  const raw = extensions?.flow
  if (!raw || typeof raw !== 'object') return null
  const value = raw as Partial<FlowConfigStored>
  if (typeof value.enabled !== 'boolean') return null
  return {
    enabled: value.enabled,
    interval_sec: Number.isFinite(value.interval_sec) ? Number(value.interval_sec) : 60,
    timeout_ms: Number.isFinite(value.timeout_ms) ? Number(value.timeout_ms) : 8000,
    script: typeof value.script === 'string' ? value.script : '',
    script_hash: Number.isFinite(value.script_hash) ? Number(value.script_hash) : 0,
    secret_names: Array.isArray(value.secret_names)
      ? value.secret_names.filter((name): name is string => typeof name === 'string')
      : [],
  }
}

/**
 * 把结果生成时刻（epoch 毫秒）写成 `HH:MM:SS`；无效值回落空串。
 *
 * tooltip 只在悬停那几秒可见，用「当天时刻」而非相对时间，避免用户还要换算。
 */
export function formatFlowUpdatedAt(ms: number | null | undefined): string {
  if (!Number.isFinite(ms) || !ms || ms <= 0) return ''
  const date = new Date(ms)
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

/**
 * 把字段色映射到 CSS 修饰类后缀；未知/缺失回落 `muted`。
 *
 * 与状态灯色共用变量名，保证「红色=异常」在信息卡各处一致。
 */
export function flowColorClass(color: FlowColor | null | undefined): FlowColor {
  return color === 'green' || color === 'yellow' || color === 'red' ? color : 'muted'
}
