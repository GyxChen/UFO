/**
 * 插件体积 / 配额上限的手抄表。
 *
 * 唯一事实来源是 Rust `src-tauri/src/plugin/paths.rs`，这份是**故意手抄的第二份**：
 * 只有两边各写一遍，`plugin-limits.spec.ts` 才有证词能力（从 Rust 反推等于 `x === x`）。
 * 改 Rust 没改这里 → 红；Rust 新增一条上限而这里没登记 → 也红。
 */

/** 清单里出现的所有数字上限与常量（名字 = Rust 的 `pub const` 名）。 */
export const PLUGIN_LIMITS = {
  /** 宿主插件契约版本：能力面变了才递增。 */
  PLUGIN_API_VERSION: 1,
  /** 清单文件自身的格式版本。 */
  MANIFEST_VERSION: 1,
  /** 包目录里那份声明的文件名。 */
  MANIFEST_FILE: 'plugin.json',
  /** app-data 下存放已装包的目录名。 */
  PLUGINS_DIR: 'plugins',
  /** 包目录里存放日志的目录名。 */
  LOG_DIR: 'logs',
  /** 安装记录文件名。 */
  LEDGER_FILE: 'ledger.json',

  MAX_MANIFEST_BYTES: 65536,
  MAX_SCRIPT_BYTES: 8388608,
  MAX_STYLE_BYTES: 1048576,
  MAX_DATA_BYTES: 2097152,
  MAX_IMAGE_BYTES: 4194304,
  MAX_FILES: 256,
  MAX_TOTAL_BYTES: 50331648,

  MAX_ID_CHARS: 40,
  MAX_NAME_CHARS: 40,
  MAX_DESCRIPTION_CHARS: 200,
  MAX_VERSION_CHARS: 32,
  MAX_REL_PATH_CHARS: 160,

  MIN_CELLS: 1,
  MAX_CELLS: 16,
  MAX_EXTENSION_ROWS: 3,
  MIN_TOOLTIP_WIDTH: 220,
  MAX_TOOLTIP_WIDTH: 420,
  MIN_TOOLTIP_HEIGHT: 120,
  MAX_TOOLTIP_HEIGHT: 620,
  MAX_HOSTS: 64,

  STORAGE_PARTITION_BYTES: 8388608,
  STORAGE_VALUE_BYTES: 262144,
  STORAGE_MAX_KEYS: 1024,
  STORAGE_MAX_KEY_CHARS: 128,

  HTTP_RESPONSE_BYTES: 4194304,

  LOG_RING_LINES: 200,
  /** 事件环形：每包多少条（拉得慢会丢最旧的，`oldestSeq` 会如实说明）。 */
  EVENT_RING_LINES: 256,
  /** 事件巡检间隔（毫秒），与悬浮窗快照广播同一节奏。 */
  EVENT_POLL_INTERVAL_MS: 1000,
  LOG_LINE_CHARS: 2000,
  LOG_FILE_BYTES: 8388608,
  LOG_PAGE_LINES: 200,

  /** 组件私有目录的目录名（`plugins/<id>/files`）。 */
  FS_DIR: 'files',
  /** 一次写入的字节上限。 */
  FS_WRITE_BYTES: 8388608,
  /** 一个包私有目录的上限。 */
  FS_PACKAGE_BYTES: 268435456,
  /** 所有包私有目录的合计上限（单包帽挡不住「装二十个包各写满」）。 */
  FS_GLOBAL_BYTES: 2147483648,
  /** 一次可读的窗口上限（URL 的 Range 一刀）。 */
  FS_READ_WINDOW_BYTES: 4194304,
  /** 用户选中的文件最大体积。 */
  FS_PICK_MAX_BYTES: 2147483648,
  /** 一个包同时能握着几个选件句柄。 */
  PICK_HANDLES_PER_PACKAGE: 8,
  /** 整个 App 的句柄总数帽。 */
  PICK_HANDLES_TOTAL: 64,
  /** 压缩包目录表一次最多列几项。 */
  ZIP_ENTRIES_MAX: 5000,
  /** 单个条目解压后的上限。 */
  ZIP_ENTRY_BYTES: 67108864,
  /** 解压后 / 压缩后的最大比例（超过按压缩包炸弹处理）。 */
  ZIP_RATIO_MAX: 200,
} as const

/** 上限表里数字类条目的类型（字符串条目只在路径拼接时用）。 */
export type PluginLimitNumeric = {
  [K in keyof typeof PLUGIN_LIMITS]: (typeof PLUGIN_LIMITS)[K] extends number ? K : never
}[keyof typeof PLUGIN_LIMITS]

/**
 * 插件 id 的合法字符：小写字母 / 数字 / 短横线，短横线不在首尾且不连续。
 * 与 Rust `paths::is_valid_plugin_id` 同一套规则，由 spec 断言两条正则**判据一致**。
 */
export const PLUGIN_ID_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/

/** id 的最大长度（与 `PLUGIN_LIMITS.MAX_ID_CHARS` 同一件事，这里单独给正则用不了）。 */
export const PLUGIN_ID_MAX_CHARS = 40

/**
 * 包内相对路径的安全判断（多段允许，绝对 / 盘符 / `\` / `..` / 空段 / 非法字符都拒）。
 * 与 Rust `paths::is_safe_rel_path` 逐形状一致；长度上限单独查表。
 */
export function isSafePluginRelPath(path: string): boolean {
  if (!path || path.length > PLUGIN_LIMITS.MAX_REL_PATH_CHARS) return false
  if (path.startsWith('/') || path.includes('\\') || path.includes('\0')) return false
  // eslint-disable-next-line no-control-regex
  if (/[\x00-\x1f]/.test(path)) return false
  if (/^[a-zA-Z]:/.test(path)) return false
  const segments = path.split('/')
  if (segments.some(segment => !/^[A-Za-z0-9._-]+$/.test(segment))) return false
  return !segments.some(segment => segment === '.' || segment === '..')
}
