/**
 * 消息推送的纯逻辑：类型、事件清单、地址校验与快照守卫。
 *
 * 这里是前端一侧「认识推送」的唯一位置：面板与 store 都从本文件取类型与判据，
 * 后端对应的解释器是 `src-tauri/src/modules/agent_status/push/config.rs`，两边口径必须一致
 * （`push.spec.ts` 锁住这一点）。规范全文见 `reference/doc/guide/agent-status-push-guide.md`。
 *
 * 配置存放在外壳的 `module_settings['agent-status'].push` 里：外壳只按 id 整块透传，
 * 不认识里面任何字段，所以加一个推送设置项不需要动外壳。
 *
 * ⚠️ 主题串（含 Bark 的设备密钥）**按用户要求明文存配置、明文显示**，不是密钥：
 * 不要在这里补脱敏，也不要把它挪回凭据管理器。
 */
import type { FloatingModuleConfig } from '@/lib/floating-modules'
import { AGENT_STATUS_MODULE_ID } from '../settings'

/** 后端广播推送面板数据的事件名（与 Rust `PUSH_STATUS_EVENT` 同值）。 */
export const PUSH_STATUS_EVENT = 'push-status'

/** 可推送的事件语义，取值是线协议字符串（与 Rust `PushEvent` 的 serde 写法一致）。 */
export type PushEventName =
  | 'turn_finished'
  | 'waiting_for_user'
  | 'failed'
  | 'session_finished'

/** 事件勾选条目的展示信息：颜色点 + 文字（⚠️ 不只靠颜色传达语义）。 */
export const PUSH_EVENT_OPTIONS: ReadonlyArray<{
  value: PushEventName
  label: string
  /** 灯色，与悬浮窗上的指示灯同一套语义。 */
  dot: 'green' | 'yellow' | 'red' | 'grey'
  /** 一句话说明这一条什么时候会触发。 */
  hint: string
}> = [
  { value: 'turn_finished', label: '一轮跑完，等你输入', dot: 'green', hint: '对应绿灯闪烁后落黄常亮' },
  { value: 'waiting_for_user', label: '在等你授权 / 回答', dot: 'yellow', hint: '对应黄灯常亮' },
  { value: 'failed', label: '本轮出错', dot: 'red', hint: '对应红灯（StopFailure）' },
  { value: 'session_finished', label: '会话结束', dot: 'grey', hint: '默认不勾：信息量低、噪音高' },
]

/** 可选的提醒内容项，取值是线协议字符串（与 Rust `PushContent` 一致）。 */
export type PushContentName = 'endpoint' | 'prompt' | 'script' | 'session_id'

/**
 * 提醒内容勾选项。
 *
 * ⚠️ 这里只列「额外带什么」—— agent 状态（哪个平台的哪类事件）是**固定发**的，没有开关：
 * 一条不含状态的推送没有意义。四项默认全不勾，勾哪一项正文里就多一行。
 */
export const PUSH_CONTENT_OPTIONS: ReadonlyArray<{
  value: PushContentName
  label: string
  hint: string
}> = [
  { value: 'endpoint', label: 'Endpoint', hint: '上游优先、回落本地地址（与信息卡同一口径）' },
  { value: 'prompt', label: '提问', hint: '最近一次用户提问原文，可能含私有代码' },
  { value: 'script', label: '脚本内容', hint: '信息卡拓展那几行的当前值' },
  { value: 'session_id', label: 'Session ID', hint: '完整会话标识（provider:session_id）' },
]

/** 服务器地址这一栏的文案（与 Rust `ServerSpec` 对应）：provider 自带，面板不认识具体服务。 */
export interface ServerSpec {
  /** 表单标签。 */
  label: string
  /** 官方默认地址；留空即用它的用户口径。 */
  default: string
  /** 标签下方的一行说明。 */
  help: string
}

/** 一个 provider 的视图：开关 + 服务器地址 + 地址栏文案。 */
export interface ProviderView {
  id: string
  label: string
  enabled: boolean
  /** 用户填的地址原样值；空串表示用 `spec.default`。 */
  server: string
  spec: ServerSpec
}

/** 一行主题（ntfy 的主题 / Bark 的设备密钥）：名字给左、主题串给右。 */
export interface ChannelView {
  id: string
  provider: string
  name: string
  topic: string
}

/** 一个主题的测试结果（统一测试逐行报成败）。 */
export interface PushTestOutcome {
  name: string
  ok: boolean
  /** HTTP 状态码；没发出去时为 0。 */
  status: number
  error: string | null
}

/** `get_push_status` / `push-status` 事件的载荷。 */
export interface PushSnapshot {
  config: PushConfig
  providers: ProviderView[]
  channels: ChannelView[]
}

/** 推送配置（与 Rust `PushConfig` 字段一一对应）。 */
export interface PushConfig {
  enabled: boolean
  events: PushEventName[]
  /** 正文额外带哪些内容（agent 状态固定发，这里只列「还要带什么」）。 */
  content: PushContentName[]
  /** 各服务的配置：只有开关与服务器地址。 */
  providers: Record<string, ProviderEntry>
  /** 主题清单（顺序即 UI 顺序）。 */
  channels: ChannelEntry[]
}

/** 配置里的一个服务（与后端 `ProviderConfig` 对应）。 */
export interface ProviderEntry {
  enabled: boolean
  server: string
}

/** 配置里的一行主题（与后端 `ChannelConfig` 对应）。 */
export interface ChannelEntry {
  id: string
  provider: string
  name: string
  topic: string
}

/** 默认配置：总开关关 —— 推送会往公网发内容，不能替用户默认打开。 */
export const DEFAULT_PUSH_CONFIG: PushConfig = {
  enabled: false,
  events: ['turn_finished', 'waiting_for_user', 'failed'],
  // 默认一条额外内容都不带：ntfy 主题公开可订阅，少外泄一寸是一寸。
  content: [],
  providers: {},
  channels: [],
}

/** 各字段的长度上限（与 Rust `push/config.rs` 的常量一致）。 */
const MAX_NAME_CHARS = 48
const MAX_TOPIC_CHARS = 256
const MAX_SERVER_CHARS = 4096

/**
 * 从外壳配置里读出推送配置块。
 *
 * 逐字段宽松取值（与 Rust `PushConfig::from_block` 同一口径）：某个值被手改坏了只回落它自己。
 * 后端送来的快照是权威值，本函数只服务「后端还没回话时先把界面摆对」。
 */
export function pushSettings(config: FloatingModuleConfig | null): PushConfig {
  const block = config?.module_settings?.[AGENT_STATUS_MODULE_ID] as Record<string, unknown> | undefined
  return parsePush(block?.push)
}

/** 解析 `push` 子块；非对象 / 缺字段都回落默认。 */
export function parsePush(raw: unknown): PushConfig {
  if (!isRecord(raw)) return { ...DEFAULT_PUSH_CONFIG }
  return {
    enabled: typeof raw.enabled === 'boolean' ? raw.enabled : DEFAULT_PUSH_CONFIG.enabled,
    // 事件与内容：**字段存在就照它走**（空数组是用户全勾掉的明确意图），只有缺字段才回落默认。
    events: Array.isArray(raw.events)
      ? (raw.events.filter(
          item => typeof item === 'string' && PUSH_EVENT_OPTIONS.some(option => option.value === item)
        ) as PushEventName[])
      : [...DEFAULT_PUSH_CONFIG.events],
    // 只认注册表里的四项：后端加了新项而这里没跟上时丢掉那一项，而不是整帧崩掉。
    content: Array.isArray(raw.content)
      ? (raw.content.filter(
          item => typeof item === 'string' && PUSH_CONTENT_OPTIONS.some(option => option.value === item)
        ) as PushContentName[])
      : [...DEFAULT_PUSH_CONFIG.content],
    providers: parseProviders(raw.providers),
    // 逐行规整（补默认名、截长度、读旧键），坏行只丢那一行 —— 与 Rust `read_channels` 同一口径。
    channels: (Array.isArray(raw.channels) ? raw.channels : [])
      .map(toChannelEntry)
      .filter((entry): entry is ChannelEntry => entry !== null),
  }
}

/** 解析服务表：单个服务坏了不影响别的服务。 */
function parseProviders(raw: unknown): PushConfig['providers'] {
  if (!isRecord(raw)) return {}
  const out: PushConfig['providers'] = {}
  for (const [id, value] of Object.entries(raw)) {
    if (!isRecord(value)) continue
    out[id] = {
      enabled: value.enabled === true,
      // 旧版本把地址存在通用 `fields` map 里（那时还没有「服务只有一个字段」这个口径）。
      server: clip(readString(value.server) ?? readLegacy(value, 'server') ?? '', MAX_SERVER_CHARS),
    }
  }
  return out
}

/**
 * 收一行主题：缺 id / provider 的行连该发给谁都不知道，直接丢。
 *
 * ⚠️ 名称为空要回落成服务名（与 Rust 同一口径）：列表左侧不能是空的。
 */
function isChannelEntry(value: unknown): value is ChannelEntry {
  if (!isRecord(value)) return false
  if (typeof value.id !== 'string' || !value.id) return false
  if (typeof value.provider !== 'string' || !value.provider) return false
  return true
}

/** 把一行原始配置规整成 `ChannelEntry`（截断长度、补默认名、读旧键）。 */
export function toChannelEntry(raw: unknown): ChannelEntry | null {
  if (!isChannelEntry(raw)) return null
  const name = readString(raw.name) ?? ''
  return {
    id: raw.id,
    provider: raw.provider,
    name: clip(name || raw.provider, MAX_NAME_CHARS),
    topic: clip(
      (readString(raw.topic) ?? readLegacy(raw, 'topic') ?? '').trim(),
      MAX_TOPIC_CHARS
    ),
  }
}

function readString(value: unknown): string | null {
  if (typeof value !== 'string') return null
  const trimmed = value.trim()
  return trimmed ? trimmed : null
}

/** 读旧版存在 `fields` 里的一个值。 */
function readLegacy(value: Record<string, unknown>, key: string): string | null {
  return isRecord(value.fields) ? readString(value.fields[key]) : null
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** 按字符数裁剪（中文不按码点切会截出半个字）。 */
function clip(value: string, max: number): string {
  return [...value].slice(0, max).join('')
}

/**
 * 生成主题行 id：`ch_` + 8 位十六进制。
 *
 * 只做行标识与增删定位 —— 它曾经是凭据 service 名的派生依据，那套已随密钥一起删掉。
 */
export function newChannelId(): string {
  const bytes = new Uint8Array(4)
  crypto.getRandomValues(bytes)
  return `ch_${[...bytes].map(byte => byte.toString(16).padStart(2, '0')).join('')}`
}

/**
 * 校验服务器地址，返回要报给用户的错误；合法返回 null。
 *
 * 判据与 Rust `providers::normalize_server` 一致（http(s) scheme + 形状），差别只在：
 * 空值在这里**算合法**（含义是「用官方默认地址」），到后端才回落。
 * ⚠️ 与流程脚本的 `validate_http_url` 刻意不同：这里的地址是用户自己显式填的，
 * 自建服务器落在局域网里是常见用法，挡掉私网等于砍功能。
 */
export function validateServer(value: string): string | null {
  const trimmed = value.trim()
  if (!trimmed) return null
  if ([...trimmed].length > MAX_SERVER_CHARS) return `服务器地址超过 ${MAX_SERVER_CHARS} 个字符`
  if (!/^https?:\/\//i.test(trimmed)) return '服务器地址必须以 http:// 或 https:// 开头'
  if (/[\s\u0000-\u001f]/.test(trimmed)) return '服务器地址含非法字符'
  return null
}

/** 主题串的唯一硬性要求：非空（太长由 `toChannelEntry` / 后端截断）。 */
export function validateTopic(value: string): string | null {
  return value.trim() ? null : '主题不能为空'
}

/**
 * 严格校验后端送来的快照。
 *
 * ⚠️ 只验到「形状对不对」，不按 provider 收窄 —— 后端加了新服务而前端注册表还没跟上时，
 * 面板应当降级显示而不是整帧丢弃（同一取舍见 `lib/agent-status.ts` 的 `isStatusSnapshot`）。
 */
export function isPushSnapshot(value: unknown): value is PushSnapshot {
  if (!isRecord(value)) return false
  if (!Array.isArray(value.providers) || !Array.isArray(value.channels)) return false
  return isRecord(value.config) && typeof value.config.enabled === 'boolean'
}

/**
 * 把 UI 里的配置改动拼成要写回外壳的那一块（外壳按浅合并整块落盘）。
 *
 * ⚠️ 整块替换：被删掉的旧字段（节流 / 静默 / 前缀 / 收件方开关）因此会在第一次保存时消失，
 * 不需要额外的迁移代码。
 */
export function pushPatch(config: PushConfig): { push: PushConfig } {
  return {
    push: {
      ...config,
      providers: Object.fromEntries(
        Object.entries(config.providers).map(([id, entry]) => [
          id,
          { enabled: entry.enabled === true, server: clip((entry.server ?? '').trim(), MAX_SERVER_CHARS) },
        ])
      ),
      channels: config.channels.map(channel => ({
        ...channel,
        name: clip(channel.name.trim() || channel.provider, MAX_NAME_CHARS),
        topic: clip(channel.topic.trim(), MAX_TOPIC_CHARS),
      })),
    },
  }
}
