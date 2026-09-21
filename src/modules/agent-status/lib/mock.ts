import type { Provider, SessionPhase, SessionSnapshot, StatusSnapshot, TokenTotals } from './agent-status'

/**
 * 开发期假会话工具：从 `.env` 读数量，造一批字段口径与真实后端一致的假会话，
 * 让「没有真实 CLI 会话时也能调试悬浮窗多灯布局 / 信息卡内容」成为可能。
 *
 * 只服务 Agent 状态灯模块自己，因此整块收在模块目录里，外壳不参与。
 * 生产构建（`vite build`）下 `import.meta.env.DEV` 为 false，`shouldInjectMockSessions`
 * 恒为 false，不会误把假会话带进正式包。
 */

/** 开发期假会话的构造选项。 */
export interface MockStatusOptions {
  /** 假会话是否带闪烁（映射 `.env` 的 `VITE_AGENT_STATUS_MOCK_FLASHING`，默认 true）。 */
  flashing?: boolean
  /** 生成基准时间戳；测试里固定它可保证输出稳定，默认当前时间。 */
  nowMs?: number
}

/** 单份假会话数量的硬上限：防止误配一个巨大值把窗口 / 快照撑爆。 */
const MAX_MOCK_SESSIONS = 64

/** 轮换的阶段：覆盖绿(执行) / 黄(等待) / 红(出错) / 未知 / 完成(绿闪) 几种灯态。 */
const PHASE_CYCLE: SessionPhase[] = ['running', 'waiting', 'completed', 'error', 'running', 'unknown']
/** 轮换的 provider：Claude / Codex 交替，两种图标都能看到。 */
const PROVIDER_CYCLE: Provider[] = ['claude', 'codex']
/** 轮换的上游 endpoint：让信息卡的 Endpoint 行有内容（命中代理解析时显示的地址）。 */
const UPSTREAM_CYCLE = [
  'https://api.anthropic.com',
  'https://api.stepfun.com/v1',
  'https://openrouter.ai/api/v1',
]
/** 轮换的本地代理地址：配合上游一起造，让 Endpoint 行的「上游 / 回落」两种分支都有数据。 */
const BASE_CYCLE = ['http://127.0.0.1:8317', 'http://127.0.0.1:8080']
/** 轮换的用户提问：信息卡最底部的 prompt 行。 */
const PROMPT_CYCLE = [
  '帮我把这个 React 组件重构成 Composition API',
  '为什么这段 Rust 代码编译不过？',
  '总结一下今天的提交记录',
]

/** 造一份带缓存读占大头的假 token 用量（贴近 Anthropic 真实分布），供信息卡 / 单位格式化调试。 */
function mockTokens(index: number): TokenTotals {
  const base = 20_000 + index * 15_000
  return {
    input_tokens: base,
    cache_creation_input_tokens: base * 2,
    cache_read_input_tokens: base * 30,
    output_tokens: 4_000 + index * 800,
  }
}

/** 按 phase 汇总计数，字段口径与真实 `StatusSnapshot.counts` 一致。 */
function countMockSessions(sessions: SessionSnapshot[]): StatusSnapshot['counts'] {
  const counts: StatusSnapshot['counts'] = { running: 0, waiting: 0, error: 0, unknown: 0, completed: 0 }
  for (const session of sessions) counts[session.phase] += 1
  return counts
}

/**
 * 造 `count` 个假会话并打包成一份合法 `StatusSnapshot`。
 *
 * 纯函数 + 确定性输出（按 index 轮换字段，不用随机），便于单测锁定；
 * 产出的快照能直接过 `isStatusSnapshot` 校验，字段口径与真实后端一致。
 */
export function buildMockStatusSnapshot(count: number, options: MockStatusOptions = {}): StatusSnapshot {
  const flashing = options.flashing ?? true
  const now = options.nowMs ?? Date.now()
  const sessions: SessionSnapshot[] = []

  for (let index = 0; index < count; index += 1) {
    const provider = PROVIDER_CYCLE[index % PROVIDER_CYCLE.length]
    const phase = PHASE_CYCLE[index % PHASE_CYCLE.length]
    sessions.push({
      key: `${provider}:mock-${index}`,
      provider,
      session_id: `mock-session-${index}`,
      phase,
      source: 'hook',
      confidence: 100,
      // 越靠后的会话越“新”开始：与真实快照「按 started 升序排灯位」一致，灯位稳定可见。
      started_at_ms: now - (count - index) * 60_000,
      last_seen_at_ms: now,
      // 只让「完成」会话闪（Stop / SessionEnd 后的绿闪）；等待留黄、执行留绿、出错留红，
      // 这样几种灯态都不被闪盖住，一屏能看全。
      completed_at_ms: phase === 'completed' ? now - 5_000 : null,
      remove_at_ms: null,
      is_flashing: flashing && phase === 'completed',
      pid: 40_000 + index,
      error_code: phase === 'error' ? 'api_error' : null,
      base_url: BASE_CYCLE[index % BASE_CYCLE.length],
      upstream_url: UPSTREAM_CYCLE[index % UPSTREAM_CYCLE.length],
      prompt: PROMPT_CYCLE[index % PROMPT_CYCLE.length],
      waiting_reason: phase === 'waiting' ? '等待你确认命令执行' : null,
      tokens: mockTokens(index),
      last_sequence: index,
      termination_confirmed: false,
    })
  }

  return {
    schema_version: 1,
    revision: 1,
    generated_at_ms: now,
    sessions,
    counts: countMockSessions(sessions),
    source_health: 'healthy',
  }
}

/**
 * 从构建期环境变量读假会话数量：`>0` 才注入，夹在 `[0, MAX_MOCK_SESSIONS]`。
 * 未设置 / 非数字 / `<=0` 一律按 0（不注入）。
 */
export function mockSessionCount(): number {
  const parsed = Number.parseInt(String(import.meta.env.VITE_AGENT_STATUS_MOCK_COUNT ?? ''), 10)
  if (!Number.isFinite(parsed) || parsed <= 0) return 0
  return Math.min(parsed, MAX_MOCK_SESSIONS)
}

/**
 * 假会话是否带闪烁；未设置或空值默认 true，只有显式写成 `false` / `0` / `no` 才关。
 * 与 `.env.template` 里「默认 true」的说明一致。
 */
export function mockSessionsFlash(): boolean {
  const raw = import.meta.env.VITE_AGENT_STATUS_MOCK_FLASHING
  if (raw === undefined || raw === null || raw === '') return true
  const normalized = String(raw).trim().toLowerCase()
  return !(normalized === 'false' || normalized === '0' || normalized === 'no')
}

/**
 * 是否启用假会话注入：**仅开发构建**（`import.meta.env.DEV`）且数量 `>0`。
 * 生产构建下 DEV 为 false，恒不注入 —— 与 `VITE_DEVTOOLS` 的既有用法一致。
 */
export function shouldInjectMockSessions(): boolean {
  return import.meta.env.DEV && mockSessionCount() > 0
}
