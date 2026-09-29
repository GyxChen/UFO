/**
 * 转灯日志的前端线协议层：类型、环节/理由的中文文案、行渲染与过滤口径。
 *
 * ⚠️ 这里的 `LogStageWire` / `LogReasonWire` 与标签表**必须**与 Rust
 * `src-tauri/src/modules/agent_status/status/log/record.rs` 的两个枚举同源。
 * 靠 `status-log-reasons.spec.ts` 直接读那份 Rust 源码来锁 —— 手抄一定会漂，
 * 而漂了的文案等于「日志里写着这个环节，界面上查不到它是谁」。
 *
 * 线协议取值 = 枚举的 `#[serde(rename_all = "snake_case")]` 结果，不要改拼写。
 */

/** 实时广播的事件名（Rust `log::STATUS_LOG_EVENT` 的另一份，两边必须一致）。 */
export const STATUS_LOG_EVENT = 'agent-status-log'

/**
 * 浮层左栏一行的行高（CSS 像素），虚拟列表用它估算总高。
 *
 * ⚠️ 必须与 `agent-status.css` 里 `--lg-row-h` 同值（`agent-status-css.spec.ts` 对撞）。
 * 两边漂了不会崩，只是滚动条长度与真实高度不符 —— 虚拟列表最典型的「拖到底还差一截」。
 */
export const LOG_ROW_HEIGHT_PX = 30

/** 视口上下各多渲染几行：滚动时少露白，快速拖动时也不会先看到一片空。 */
export const LOG_OVERSCAN_ROWS = 6

/** 环节（一次信号在链路上经过的那一站）。 */
export type LogStageWire =
  | 'hook_attempt'
  | 'compat_verdict'
  | 'inbox_write'
  | 'inbox_batch'
  | 'inbox_quarantine'
  | 'kernel_verdict'
  | 'light_transition'
  | 'light_unchanged'
  | 'subagent_alive_only'
  | 'session_reaped'
  | 'idle_lantern_cleared'
  | 'rollout_scan'
  | 'rollout_dropped'
  | 'rollout_seeded'
  | 'rollout_activity'
  | 'baseline_reflow'
  | 'upstream_resolved'
  | 'source_health'
  | 'flow_run'
  | 'push_signal'
  | 'config_applied'
  | 'log_maintenance'

/** 该环节给出这个结果的理由（纯记录性条目可以没有）。 */
export type LogReasonWire =
  | 'unknown_provider'
  | 'stdin_unreadable'
  | 'input_too_large'
  | 'payload_not_json'
  | 'no_hook_adapter'
  | 'adapter_rejected'
  | 'adapter_ignored_event'
  | 'inbox_dir_unwritable'
  | 'temp_create_failed'
  | 'temp_write_failed'
  | 'publish_rename_failed'
  | 'hook_completed'
  | 'not_json_extension'
  | 'stale_temp_swept'
  | 'parse_failed'
  | 'quarantine_rename_failed_file_lost'
  | 'inbox_unreadable'
  | 'schema_mismatch'
  | 'empty_event_id'
  | 'empty_session_id'
  | 'duplicate_in_seen_window'
  | 'stale_sequence'
  | 'completed_within_retention'
  | 'subagent_on_finished_session'
  | 'unknown_kind_inherits_timing'
  | 'phase_reduced_same'
  | 'phase_changed'
  | 'flash_settled'
  | 'seed_applied'
  | 'refresh_only'
  | 'retention_elapsed'
  | 'running_timeout'
  | 'waiting_timeout'
  | 'error_timeout'
  | 'process_dead'
  | 'cleared_by_new_turn_of_same_provider'
  | 'manually_dismissed'
  | 'cares_prefilter_skip'
  | 'unclassified_line'
  | 'same_kind_merged_in_batch'
  | 'same_as_last_round_baseline'
  | 'already_registered'
  | 'session_id_unparsable'
  | 'seeded_without_event'
  | 'sessions_dir_missing'
  | 'rollout_file_vanished'
  | 'activity_implies_running'
  | 'keepalive_running'
  | 'batch_counters_only'
  | 'resolver_hit'
  | 'resolver_miss'
  | 'resolver_error'
  | 'source_degraded'
  | 'source_recovered'
  | 'flow_ok'
  | 'flow_error'
  | 'flow_backoff'
  | 'push_disabled'
  | 'push_no_topics'
  | 'push_queued'
  | 'push_delivered'
  | 'push_failed'
  | 'config_reloaded'
  | 'log_opened'
  | 'log_rotated'
  | 'log_rotation_skipped_file_busy'
  | 'log_write_failed'
  | 'sink_disabled'

/**
 * 一条日志（`LogEntry` 的线协议形状）。
 *
 * 除前四个字段外全部 `skip_serializing_if = "Option::is_none"` —— 缺失就是「这一条没有这个坐标」，
 * 面板不许拿它当空字符串渲染。
 */
export interface LogEntry {
  /** 事件时刻（Unix 毫秒）；文件按 UTC 分片，这里按本地时区显示。 */
  ts_ms: number
  /** 写这一行的是哪个进程 / 线程。 */
  writer: 'hook' | 'monitor' | 'ui'
  /** 链路环节。 */
  stage: LogStageWire
  /** 该环节为什么给出这个结果。 */
  reason?: LogReasonWire
  /** 贯穿 id：一条信号从 hook 到内核共用它。 */
  trace_id?: string
  /** 事件去重 id（原样带上，便于和 inbox 内容对上）。 */
  event_id?: string
  /** 平台。 */
  provider?: string
  /** 会话 id。 */
  session_id?: string
  /** 会话 key（`provider:session_id`），按会话过滤走它。 */
  key?: string
  /** 执行者：主线程为空，子代理是它的 `agent_id`。 */
  actor?: string
  /** 兼容层归一出的事件语义。 */
  kind?: string
  /** 灯态：变化前。 */
  phase_from?: string
  /** 灯态：变化后，或被拒绝时「维持原样」的那个态。 */
  phase_to?: string
  /** 事件自带的序号（多数平台不带）。 */
  sequence?: number
  /** 内核当时为该执行者记的序号水位。 */
  watermark?: number
  /** 内核 revision：与广播出去的快照对得上。 */
  revision?: number
  /** 白名单标量细节（明细级才可能带用户文本的摘录）。 */
  detail?: Record<string, unknown>
}

/**
 * 面板 / 广播里的一条：盘上身份 + 展平的日志行。
 *
 * ⚠️ `:key`、合并去重、选中比对一律用 `id`（= `(日期, 文件序号, 起始字节)`）。
 * 线协议里已经没有进程内游标了：那种数重启归零，拿它翻页会串到别的行上。
 */
export type StatusLogEntry = { id: string } & LogEntry

/** 一条日志在盘上的坐标（与 Rust `LogPosition` 同名字段），既是行身份也是翻页锚点。 */
export interface LogPosition {
  /** UTC 日期（`YYYY-MM-DD`）。 */
  day: string
  /** 当日文件序号（文件名里那个）。 */
  file_seq: number
  /** 该行在文件里的起始字节偏移。 */
  offset: number
}

/** 查询条件（与 Rust `StatusLogQuery` 同名；全部可选）。 */
export interface StatusLogQuery {
  /** 上一页最后一条的盘上坐标：只取比它更旧的。缺省 = 从最新一条开始。 */
  before?: LogPosition | null
  /** 一页多少条（缺省 200）。⚠️ 只有分页读它：导出与计数忽略它。 */
  limit?: number
  /** 只看某个会话（`provider:session_id`）。 */
  key?: string | null
  /** 只看某条信号的完整链路。 */
  trace_id?: string | null
  /** 只看某个环节。 */
  stage?: string | null
  /** 只看某个理由。 */
  reason?: string | null
  /** 只看「没落成灯变化」的那些（被拒、被丢、被摘、只记账）。 */
  only_rejected?: boolean | null
  /** 只要这个时刻之后的。 */
  since_ms?: number | null
}

/** 日志目录的占用读数（字段与 Rust `LogDiskUsage` 同名同义）。 */
export interface LogDiskUsage {
  /** 认识的文件合计字节数。 */
  bytes: number
  /** 文件个数。 */
  files: number
  /** 最早一天的日期（`YYYY-MM-DD`）；目录空时为 null。 */
  oldest_day: string | null
  /** 本次运行内是否删过「还没到保留天数、只因超出体积上限」的整天文件。 */
  cut_by_size_cap: boolean
}

/** 缺省占用读数：后端还没答话时也要能渲染（面板不许判 undefined）。 */
export function emptyLogUsage(): LogDiskUsage {
  return { bytes: 0, files: 0, oldest_day: null, cut_by_size_cap: false }
}

/**
 * 一页日志（命令返回值，也是实时广播的载荷）。
 *
 * ⚠️ 这里**没有**「一共多少条」：那曾经是环形缓冲的条数，而它被当成会话条数写过文案
 * （「本会话 50+ 条」就是这么来的）。条数走 `count_status_log`，能不能继续翻走 `exhausted`。
 */
export interface StatusLogPage {
  /** 环形水位：只用来判断「这一轮有没有新行进过内存」，不参与身份、翻页与计数。 */
  head: number
  /** 日志目录：常驻显示它，用户才知道去哪翻历史。 */
  dir: string
  /** 目录占用读数：设置页靠它说清「这套账吃了多少盘、最早到哪天」。 */
  usage: LogDiskUsage
  /** 下一页的锚点（本页最后一条的坐标）；实时增量包里为 null（它不是从锚点翻出来的）。 */
  next_before: LogPosition | null
  /** 已经翻到目录头：据此收起「看更早的」，否则它会一直点、一直空。 */
  exhausted: boolean
  /** 为凑齐这一页读了多少字节。 */
  scanned_bytes: number
  /** 命中的条目，**新→旧**。 */
  entries: StatusLogEntry[]
}

/** 一次按盘精确计数的结果（与 Rust `StatusLogCount` 同名同义）。 */
export interface StatusLogCount {
  /** 命中总条数：精确值，不是下界 —— 界面因此再也不许出现「N+ / 约」。 */
  total: number
  /** 这次扫描的时刻：计数是瞬时快照，界面要写成「截至 HH:MM:SS 共 N 条」。 */
  as_of_ms: number
  /** 实际扫描字节（诊断「这个数为什么慢」用）。 */
  scanned_bytes: number
}

/** 字节数 → 人类可读。达到 1 MiB 用 MiB，否则 KiB，都只留一位小数（够判断量级）。 */
export function formatLogBytes(bytes: number): string {
  const mib = bytes / (1024 * 1024)
  if (mib >= 1) return `${mib.toFixed(1)} MiB`
  return `${(bytes / 1024).toFixed(1)} KiB`
}

/**
 * 占用读数文案：`占用 24.7 MiB · 9 个文件 · 最早 09-22`。
 *
 * ⚠️ `cut_by_size_cap` 那句必须留着：界面上只有「保留天数」一个旋钮，而容量那条线
 * 仍会在量特别大时削掉最旧的整天 —— 不说出来，「保留 N 天」就是个会被静默打破的承诺。
 * 它只反映本次运行（启动前删掉的那次无从知道），所以没置位时什么都不补，宁可少说。
 */
export function logUsageLabel(usage: LogDiskUsage): string {
  if (!usage.files) return '还没写过日志'
  const parts = [`占用 ${formatLogBytes(usage.bytes)}`, `${usage.files} 个文件`]
  if (usage.oldest_day) parts.push(`最早 ${usage.oldest_day.slice(5)}`)
  if (usage.cut_by_size_cap) parts.push('更早的已按容量上限回收')
  return parts.join(' · ')
}

/** 计数处于哪一步：四态各有文案，任何一档都不许出现「+」或「约」。 */
export type LogCountState = 'idle' | 'loading' | 'ready' | 'failed'

/**
 * 计数文案：`本会话 12 条 · 截至 14:22:31`。
 *
 * `as_of` 那个时刻是这条能写精确数的前提 —— 它把「一个会过期的精确值」变成
 * 「一句永远为真的历史事实」，于是既不需要「N+」这种含糊，也不用担心数字在眼前变了。
 *
 * @param prefix 「本会话」/「共」这类主语，两处调用只差主语与查询条件
 */
export function logCountLabel(
  prefix: string,
  count: StatusLogCount | null,
  state: LogCountState
): string {
  if (state === 'loading' || state === 'idle') return `${prefix}计数中…`
  if (state === 'failed') return `${prefix}计数暂时读不到`
  if (!count) return `${prefix}计数中…`
  return `${prefix} ${count.total} 条 · 截至 ${formatLogClock(count.as_of_ms)}`
}

/** 一次导出的结果。 */
export interface ExportedLog {
  /** 实际写到哪（用户选的路径，或未给路径时的日志目录内 `export/`）。 */
  path: string
  /** 带出去多少条 = **盘上命中总数**（不再等于「内存环形装得下的那一段」）。 */
  entries: number
  /** 为拿到这份结果读了多少字节（判「这次导出为什么慢」的唯一线索）。 */
  scanned_bytes: number
  /** 撞到扫描预算 ⇒ 这份**不完整**，界面必须说出来，不许只报条数。 */
  stopped_by_budget: boolean
}

/** 环节 → 界面短标签（徽标用，宽度有限，词要短）。 */
export const STAGE_LABELS: Record<LogStageWire, string> = {
  hook_attempt: 'hook',
  compat_verdict: '兼容层',
  inbox_write: '落盘',
  inbox_batch: '收件',
  inbox_quarantine: '隔离',
  kernel_verdict: '内核',
  light_transition: '灯变',
  light_unchanged: '采纳未转灯',
  subagent_alive_only: '子代理记账',
  session_reaped: '摘灯',
  idle_lantern_cleared: '清空闲灯',
  rollout_scan: '巡检聚合',
  rollout_dropped: '巡检丢弃',
  rollout_seeded: '巡检播种',
  rollout_activity: '巡检活动',
  baseline_reflow: '基线回灌',
  upstream_resolved: '上游',
  source_health: '事件源',
  flow_run: '脚本',
  push_signal: '推送',
  config_applied: '配置',
  log_maintenance: '日志自身',
}

/** 环节 → 一句话说明它站在链路的哪一段（悬浮释义与右侧详情用）。 */
export const STAGE_EXPLAIN: Record<LogStageWire, string> = {
  hook_attempt: 'hook 进程收到一次平台调用（含各种早退）',
  compat_verdict: '兼容层裁决：归一成哪种语义，或表明「这条我不关心」',
  inbox_write: 'hook 把规范事件原子写进 inbox',
  inbox_batch: '主进程读了一批 inbox（含跳过与清扫）',
  inbox_quarantine: '批内某个文件解析失败被隔离（或干脆消失）',
  kernel_verdict: '一条事件进内核后的裁决（采纳 / 拒绝及理由）',
  light_transition: '灯色真的变了',
  light_unchanged: '事件被采纳，但灯色没变',
  subagent_alive_only: '带 agent_id 的事件只维护后台集合，灯与计时原样保留',
  session_reaped: '会话被清理（超时 / 进程已死 / 手动）',
  idle_lantern_cleared: '别人的空闲黄灯被同平台的新一轮清掉',
  rollout_scan: 'Codex rollout 一轮巡检的聚合账',
  rollout_dropped: 'rollout 里某条记录被丢弃及原因',
  rollout_seeded: 'rollout 首次登记只播种，刻意不产出事件',
  rollout_activity: '「文件在长 = 有活动」推出的绿灯，或 30 秒保活',
  baseline_reflow: 'hook 事件把新灯态回灌给巡检器的去抖基线',
  upstream_resolved: 'endpoint 反查上游的结果',
  source_health: '事件源健康度变化（坏文件出现 / 清干净）',
  flow_run: '流程脚本一次执行的结果',
  push_signal: '灯变触发的推送信号与其归宿',
  config_applied: '配置热更新后的快照',
  log_maintenance: '日志系统自己的开关、轮转与写入失败',
}

/** 理由 → 它到底说明了什么（照 Rust 枚举的文档注释写，不许自己发挥语义）。 */
export const REASON_EXPLAIN: Record<LogReasonWire, string> = {
  unknown_provider: '--provider 拼错或未登记',
  stdin_unreadable: 'stdin 读不出来',
  input_too_large: 'payload 超过单次读取上限',
  payload_not_json: 'payload 不是合法 JSON',
  no_hook_adapter: '该平台没有兼容层适配器',
  adapter_rejected: '适配器判这条 payload 非法（缺 session id 等）',
  adapter_ignored_event: '适配器表明「该事件我不关心」：既不落盘也不报错',
  inbox_dir_unwritable: 'inbox 目录建不出来',
  temp_create_failed: '临时文件建不出来',
  temp_write_failed: '临时文件写入失败',
  publish_rename_failed: 'rename 发布失败（事件等于没送到）',
  hook_completed: 'hook 正常完成，事件已落盘',
  not_json_extension: '扩展名不是 .json，本轮跳过',
  stale_temp_swept: 'hook 崩溃遗留的半截 .tmp，扫描时清掉',
  parse_failed: '反序列化失败，移进隔离目录',
  quarantine_rename_failed_file_lost: '隔离也失败，只能删 —— 文件凭空消失',
  inbox_unreadable: 'inbox 目录读不出来（被删、权限、正被占用）',
  schema_mismatch: 'schema_version 与本二进制不符',
  empty_event_id: '事件 id 为空，无法去重',
  empty_session_id: '会话 id 为空，无法定位灯',
  duplicate_in_seen_window: '去重窗口内已见过同一个 event_id',
  stale_sequence: '序号不比该执行者的水位新（乱序迟到）',
  completed_within_retention: '会话已完成且仍在绿闪保留窗内',
  subagent_on_finished_session: '子代理事件落在已结束 / 已出错的会话上：只记账',
  unknown_kind_inherits_timing: 'Unknown 事件且状态未变：继承旧计时',
  phase_reduced_same: '事件被采纳，但推导出的灯态与原态相同',
  phase_changed: '灯态发生了变化',
  flash_settled: '绿闪落定（保留期到，闪绿收成常黄）',
  seed_applied: '用了跨重启存档的回填值',
  refresh_only: '采纳且没用到回填，灯态也未变（只刷新 last_seen）',
  retention_elapsed: '完成灯保留期已过',
  running_timeout: 'Running 超时仍无新事件',
  waiting_timeout: 'Waiting 兜底超时',
  error_timeout: 'Error 灯超时',
  process_dead: '上报的 pid 已经不在（进程被杀）',
  cleared_by_new_turn_of_same_provider: '同平台的新一轮把它的空闲黄灯清掉了',
  manually_dismissed: '用户手动移除',
  cares_prefilter_skip: '子串预筛没命中：该行只推进「文件在长」',
  unclassified_line: '命中预筛但分类不出语义（新记录类型还没登记）',
  same_kind_merged_in_batch: '同一批里连续同灯态，压成一条',
  same_as_last_round_baseline: '与上一轮灯态基线相同 → 丢弃（批准后卡黄就出在这）',
  already_registered: '该 rollout 文件上一轮已登记过',
  session_id_unparsable: '文件名解析不出 session id',
  seeded_without_event: '首次登记只播种，刻意不产出事件',
  sessions_dir_missing: '会话目录不存在（没跑过，或 CODEX_HOME 指错）',
  rollout_file_vanished: '归档导致读不到文件，判会话结束',
  activity_implies_running: '「文件在长」推出绿灯',
  keepalive_running: '长时间只写命令输出，30 秒补一条保活绿',
  batch_counters_only: '逐行记账已达本轮上限，之后只报聚合计数',
  resolver_hit: '代理解析器反查命中上游',
  resolver_miss: '解析器没有该地址对应的记录（没开代理或端口变了）',
  resolver_error: '解析器读自己的配置库失败',
  source_degraded: '事件源降级：inbox 里有解析失败的文件',
  source_recovered: '事件源恢复：坏文件已被清走',
  flow_ok: '脚本执行成功',
  flow_error: '脚本执行失败（语法、返回形状、HTTP）',
  flow_backoff: '脚本连续失败进入指数退避，本轮不发',
  push_disabled: '推送总开关或服务开关关着',
  push_no_topics: '该服务一个主题都没有',
  push_queued: '灯变已排进投递队列',
  push_delivered: '投递成功',
  push_failed: '投递失败，进入退避',
  config_reloaded: '组件配置热更新已应用到运行时',
  log_opened: '日志写入器开启（含档位）',
  log_rotated: '日志按体积或天数换了文件 / 删了旧文件',
  log_rotation_skipped_file_busy: '旧文件正被占用，本轮放弃回收',
  log_write_failed: '写日志本身失败（磁盘、权限）；灯态照旧',
  sink_disabled: '日志被关闭了',
}

/** 环节下拉的取值（`null` = 全部环节）。 */
export const STAGE_OPTIONS: ReadonlyArray<{ value: LogStageWire | null; label: string }> = [
  { value: null, label: '全部环节' },
  ...(Object.keys(STAGE_LABELS) as LogStageWire[]).map(stage => ({
    value: stage,
    label: STAGE_LABELS[stage],
  })),
]

/**
 * 「只看被拒 / 被丢」算被拒的环节。
 *
 * 这些环节的存在意义就是「事情没成」；`kernel_verdict` 也在内 —— 它的绝大多数行是拒绝理由，
 * 唯一的例外（`phase_changed`）另有 `light_transition` 环节，不会因此漏看。
 */
const REJECTING_STAGES: readonly LogStageWire[] = [
  'compat_verdict',
  'inbox_quarantine',
  'kernel_verdict',
  'light_unchanged',
  'subagent_alive_only',
  'session_reaped',
  'idle_lantern_cleared',
  'rollout_dropped',
]

/** 被拒理由里这两个是「静默忽略」，历史上完全无痕，是最该被这个过滤捞出来的。 */
const REJECTING_REASON_EXCLUDE: readonly LogReasonWire[] = ['phase_changed', 'flash_settled', 'seed_applied']

/** 这条记录是不是「没落成灯变化」的那类（后端 `only_rejected` 用同名口径）。 */
export function isRejected(entry: LogEntry): boolean {
  if (entry.reason && REJECTING_REASON_EXCLUDE.includes(entry.reason)) return false
  return REJECTING_STAGES.includes(entry.stage)
}

/** 行的色调：绿 = 灯真的变了，红 = 出错 / 摘灯 / 写失败，黄 = 被拒被丢，其余无彩条。 */
export type LogTone = 'ok' | 'warn' | 'err' | 'plain'

/** 出错类理由：它们不代表灯态，只代表「这条线索本身断了」。 */
const ERROR_REASONS: readonly LogReasonWire[] = [
  'unknown_provider',
  'stdin_unreadable',
  'input_too_large',
  'payload_not_json',
  'inbox_dir_unwritable',
  'temp_create_failed',
  'temp_write_failed',
  'publish_rename_failed',
  'parse_failed',
  'quarantine_rename_failed_file_lost',
  'inbox_unreadable',
  'schema_mismatch',
  'resolver_error',
  'source_degraded',
  'flow_error',
  'push_failed',
  'log_write_failed',
]

export function logTone(entry: LogEntry): LogTone {
  if (entry.stage === 'light_transition' || entry.reason === 'phase_changed') return 'ok'
  if (ERROR_REASONS.includes(entry.reason as LogReasonWire) || entry.stage === 'session_reaped') return 'err'
  if (isRejected(entry)) return 'warn'
  return 'plain'
}

/** 一行里最该被读到的那半句：理由 + 灯态变化（没有灯态变化时给理由本身）。 */
export function logHeadline(entry: LogEntry): string {
  return entry.reason ? REASON_EXPLAIN[entry.reason] : STAGE_EXPLAIN[entry.stage]
}

/**
 * 理由的整句解释；没有理由（纯记录性条目）给环节说明。
 *
 * 单独一个函数是因为调用点拿到的是 `entry.reason`（可能 `undefined`），
 * 直接下标 `REASON_EXPLAIN[...]` 在类型上就不成立，随手 `?? ''` 会查出 `undefined`。
 */
export function reasonExplain(reason?: LogReasonWire): string {
  return reason ? REASON_EXPLAIN[reason] : ''
}

/** 两行摘要里的第二行：把 detail 里的白名单标量拼成 `k=v`，没有就退回事件语义。 */
export function logDetailNote(entry: LogEntry): string {
  const bits = Object.entries(entry.detail ?? {})
    .filter(([, value]) => value === null || typeof value !== 'object')
    .map(([key, value]) => `${key}=${value ?? '—'}`)
  if (bits.length) return bits.join(' · ')
  return entry.kind ? `kind=${entry.kind}` : ''
}

/** 灯态变化写作「绿→黄」那种读法；两端缺失时返回空串（调用方省略这段）。 */
export function logPhaseChange(entry: LogEntry): string {
  if (!entry.phase_from && !entry.phase_to) return ''
  return `${PHASE_LIGHTS[entry.phase_from ?? ''] ?? entry.phase_from ?? '—'}→${
    PHASE_LIGHTS[entry.phase_to ?? ''] ?? entry.phase_to ?? '—'
  }`
}

/**
 * 灯态 → 灯色字（界面上这两个字要染成状态灯色，见 `.lg-light--*`）。
 *
 * 内核的 phase 取值是 `running` / `waiting` / `completed` / `failed` / `后台 running` 那一套，
 * 认不出的原样显示 —— 猜一个颜色会把「不知道」伪装成「知道」。
 */
const PHASE_LIGHTS: Record<string, string> = {
  running: '绿',
  waiting: '黄',
  completed: '黄',
  failed: '红',
}

/** 会话 key 里的平台段（`codex:8f2c…` → `codex`），右侧详情与过滤 chip 用。 */
export function logKeyProvider(key?: string): string {
  if (!key) return ''
  const index = key.indexOf(':')
  return index > 0 ? key.slice(0, index) : key
}

/** 本地时区的 `HH:MM:SS`（列表行的时间列）。 */
export function formatLogClock(tsMs: number): string {
  const date = new Date(tsMs)
  if (Number.isNaN(date.getTime())) return '—'
  const pad = (value: number, width = 2) => String(value).padStart(width, '0')
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

/** 带毫秒的 `HH:MM:SS.mmm`（右侧详情里同一条信号的相邻行要对得上先后）。 */
export function formatLogClockMillis(tsMs: number): string {
  const date = new Date(tsMs)
  if (Number.isNaN(date.getTime())) return '—'
  return `${formatLogClock(tsMs)}.${String(date.getMilliseconds()).padStart(3, '0')}`
}

/**
 * 把一条记录还原成线协议那一行（右侧详情用）。
 *
 * ⚠️ 内存游标 `seq` **不在磁盘行里**（它是本进程的序号），所以这里按 `LogEntry` 的字段顺序
 * 重新序列化并跳过空值 —— 与 Rust 的 `skip_serializing_if = "Option::is_none"` 同形，
 * 但它是「等价重建」而不是文件里那串字节，文案要照实写，别骗用户说是逐字原文。
 */
export function logWireLine(entry: LogEntry): string {
  const ordered: Record<string, unknown> = {}
  for (const key of [
    'ts_ms',
    'writer',
    'stage',
    'reason',
    'trace_id',
    'event_id',
    'provider',
    'session_id',
    'key',
    'actor',
    'kind',
    'phase_from',
    'phase_to',
    'sequence',
    'watermark',
    'revision',
    'detail',
  ] as const) {
    const value = entry[key]
    if (value === undefined || value === null) continue
    ordered[key] = value
  }
  return JSON.stringify(ordered)
}

/** 右侧详情的字段表：与线协议同名，缺的不显示（而不是显示 `undefined`）。 */
export const LOG_FIELD_ORDER: ReadonlyArray<{ key: keyof LogEntry; label: string }> = [
  { key: 'writer', label: 'writer' },
  { key: 'stage', label: 'stage' },
  { key: 'reason', label: 'reason' },
  { key: 'trace_id', label: 'trace_id' },
  { key: 'event_id', label: 'event_id' },
  { key: 'provider', label: 'provider' },
  { key: 'session_id', label: 'session_id' },
  { key: 'key', label: 'key' },
  { key: 'actor', label: 'actor' },
  { key: 'kind', label: 'kind' },
  { key: 'phase_from', label: 'phase_from' },
  { key: 'phase_to', label: 'phase_to' },
  { key: 'sequence', label: 'sequence' },
  { key: 'watermark', label: 'watermark' },
  { key: 'revision', label: 'revision' },
]
