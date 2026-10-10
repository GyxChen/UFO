/** 管理区用的几个小格式化：只格式化读数，不解释业务。 */

/** 把字节数说成人话（管理区一行要能一眼看出是 KB 还是 MB）。 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—'
  if (bytes < 1024) return `${bytes} B`
  const kb = bytes / 1024
  if (kb < 1024) return `${round(kb)} KB`
  return `${round(kb / 1024)} MB`
}

/** 毫秒读数：小于 1 秒给整数毫秒，否则给秒（一位小数）。 */
export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return '—'
  if (ms < 1000) return `${Math.round(ms)}ms`
  return `${round(ms / 1000)}s`
}

/** 一位小数取整（去掉 1.0 这种噪音尾巴）。 */
function round(value: number): string {
  const text = value.toFixed(1)
  return text.endsWith('.0') ? text.slice(0, -2) : text
}
