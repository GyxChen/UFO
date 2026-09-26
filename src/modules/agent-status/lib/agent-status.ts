import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export const AGENT_STATUS_EVENT = 'agent-status'

/**
 * 事件来源平台。
 *
 * 列出已接入平台的字面量拿到补全，同时容纳尚未登记进
 * {@link providerMeta} 的新平台串（`(string & {})` 是"宽口径但仍列得出已知项"的写法）。
 * ⚠️ 展示名/图标一律走注册表，不要在这里加字面量后又在视图里写三元。
 */
export type Provider = 'claude' | 'codex' | 'qoder' | (string & {})
export type SessionPhase = 'running' | 'waiting' | 'error' | 'unknown' | 'completed'
export type StatusColor = 'green' | 'yellow' | 'red'

/** 一个会话累计消耗的 token，与 Rust 侧 `TokenTotals` 字段一一对应。 */
export interface TokenTotals {
  /** 新增输入 token（不含缓存部分）。 */
  input_tokens: number
  /** 写入缓存的 token。 */
  cache_creation_input_tokens: number
  /** 命中缓存读取的 token。 */
  cache_read_input_tokens: number
  /** 模型生成的 token（Rust 侧已含 thinking，此处不重复计入）。 */
  output_tokens: number
}

export interface SessionSnapshot {
  key: string
  provider: Provider
  session_id: string
  phase: SessionPhase
  source: 'hook' | 'process_fallback'
  confidence: number
  started_at_ms: number
  last_seen_at_ms: number
  completed_at_ms: number | null
  remove_at_ms: number | null
  is_flashing: boolean
  pid: number | null
  error_code: string | null
  /** 该会话使用的 API endpoint；未知时为 null（旧 hook 二进制不上报）。 */
  base_url: string | null
  /**
   * base_url 经所选代理解析器反查出的上游 endpoint（如 cc-switch 后的 https://api.b.ai）。
   * 未启用解析 / 未命中时为 null，此时 Endpoint 回落到 base_url。
   */
  upstream_url: string | null
  /** 用户最近一次提问内容；未知时为 null。 */
  prompt: string | null
  /** 「正在等你操作」的原因原文（如 Codex 的审批理由）；非等待态为 null。 */
  waiting_reason: string | null
  /** 会话累计消耗的 token；尚未统计到时为 null（旧 hook 二进制或未采集到 transcript）。 */
  tokens: TokenTotals | null
  last_sequence: number | null
  termination_confirmed: boolean
}

export interface StatusSnapshot {
  schema_version: number
  revision: number
  generated_at_ms: number
  sessions: SessionSnapshot[]
  counts: {
    running: number
    waiting: number
    error: number
    unknown: number
    completed: number
  }
  source_health: string
}

/**
 * 按会话阶段映射指示灯颜色：
 * - running → 绿常亮（正在执行）
 * - completed → 绿（配合 is_flashing 闪烁，表示对话已完成）
 * - waiting + is_flashing → 绿闪（一轮 Stop 刚完成，闪完后落黄）
 * - waiting / unknown → 黄（等待用户操作）
 * - error → 红（接口/不可抗力错误）
 */
export function colorForSession(session: SessionSnapshot): StatusColor {
  if (session.phase === 'error') return 'red'
  if (session.phase === 'running' || session.phase === 'completed') return 'green'
  // Stop 进入 Waiting 时会先绿闪；闪烁期间按绿色渲染。
  if (session.phase === 'waiting' && session.is_flashing) return 'green'
  return 'yellow'
}

/**
 * 会话阶段的中文短标签。
 *
 * 只给**溢出格的折叠清单**用：那里一行要同时读「阶段 + 提问」，英文枚举（`waiting`）在成段的行里
 * 读不出信息；单会话信息卡头部仍按原样显示枚举 —— 那是个大写字距的小徽章，枚举在那种形态下更像标签。
 * 未列出的阶段回落 `unknown` 的中文，不返回空串（读屏与省略号都不该出现空标签）。
 */
export function phaseLabel(phase: SessionPhase): string {
  switch (phase) {
    case 'running':
      return '执行中'
    case 'waiting':
      return '等待操作'
    case 'error':
      return '出错'
    case 'completed':
      return '已完成'
    default:
      return '状态未知'
  }
}

/**
 * 灯色的中文短标签（读屏文案用，如「还有 3 个会话：1 个出错、2 个等待操作」）。
 * 与 {@link colorForSession} 同一套语义；绿色涵盖「执行中」与「一轮刚跑完」两种阶段，
 * 统一按「执行中」读 —— 读屏文案里再区分闪烁只会更长。
 */
export function colorLabel(color: StatusColor): string {
  switch (color) {
    case 'red':
      return '出错'
    case 'yellow':
      return '等待操作'
    default:
      return '执行中'
  }
}

/**
 * 会话 Endpoint 的展示值：命中代理解析出的上游时优先显示上游，否则回落到 base_url。
 *
 * 两者都缺时返回 null（前端显示占位符）。
 */
export function displayEndpoint(session: SessionSnapshot): string | null {
  return session.upstream_url ?? session.base_url
}

/**
 * 会话累计 token 总量：输入 + 缓存写 + 缓存读 + 输出。
 *
 * 与 Rust 侧 `TokenTotals::total()` 保持同一口径；缓存读通常占比最大，
 * 漏掉会显著低估实际消耗。未统计到（null）时按 0 处理。
 */
export function tokenTotal(tokens: TokenTotals | null | undefined): number {
  if (!tokens) return 0
  return (
    tokens.input_tokens + tokens.cache_creation_input_tokens + tokens.cache_read_input_tokens + tokens.output_tokens
  )
}

/**
 * 千分位格式化整数，便于在小字号下读出位数。
 *
 * 不用 `toLocaleString`：其输出依赖运行环境的 ICU 数据，会让测试结果不稳定。
 * 非法值（NaN / Infinity）一律按 0 处理。
 */
export function formatTokenCount(value: number): string {
  const safe = Number.isFinite(value) ? Math.max(Math.trunc(value), 0) : 0
  return String(safe).replace(/\B(?=(\d{3})+(?!\d))/g, ',')
}

/**
 * 以百万（M tokens）为单位格式化，便于快速读出量级。
 *
 * 保留两位小数再去掉末尾多余的 0（`1.00M` → `1M`，`12.60M` → `12.6M`）；
 * 不足 0.01M 时显示 `<0.01M` —— 否则小数位不足以表达，会退化成没有信息量的 `0M`。
 */
export function formatTokenMillions(value: number): string {
  const safe = Number.isFinite(value) ? Math.max(value, 0) : 0
  const millions = safe / 1_000_000
  if (millions > 0 && millions < 0.01) return '<0.01M'
  return `${millions.toFixed(2).replace(/\.?0+$/, '')}M`
}

export function isStatusSnapshot(value: unknown): value is StatusSnapshot {
  if (!value || typeof value !== 'object') return false
  const snapshot = value as Partial<StatusSnapshot>
  return (
    // ⚠️ schema_version 是**刻意**的前后端同步 tripwire（内核恒发 1），与早先的 provider 白名单
    // 性质不同：那条白名单会把新平台的每一帧整帧丢弃（首帧走 invoke 不经此校验，
    // 症状是灯冻结在首帧、永不更新），故只按"是不是个能看的串"校验，展示交给注册表降级。
    snapshot.schema_version === 1 &&
    Number.isSafeInteger(snapshot.revision) &&
    snapshot.revision >= 0 &&
    typeof snapshot.generated_at_ms === 'number' &&
    Array.isArray(snapshot.sessions) &&
    snapshot.sessions.length <= 1000 &&
    snapshot.sessions.every(
      session =>
        session &&
        typeof session.key === 'string' &&
        typeof session.provider === 'string' &&
        session.provider.length > 0 &&
        session.provider.length <= 64 &&
        typeof session.session_id === 'string' &&
        typeof session.phase === 'string' &&
        typeof session.termination_confirmed === 'boolean'
    ) &&
    typeof snapshot.counts === 'object' &&
    snapshot.counts !== null
  )
}

export async function listenToAgentStatus(onSnapshot: (snapshot: StatusSnapshot) => void): Promise<UnlistenFn> {
  return listen<StatusSnapshot>(AGENT_STATUS_EVENT, event => {
    if (isStatusSnapshot(event.payload)) onSnapshot(event.payload)
  })
}

export function getAgentStatusSnapshot(): Promise<StatusSnapshot> {
  return invoke<StatusSnapshot>('get_status_snapshot')
}
