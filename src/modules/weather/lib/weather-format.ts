/**
 * 天气数据展示相关纯函数格式化工具
 */

/**
 * 格式化温度
 *
 * @param temp 摄氏度数值
 * @returns 格式化后的温度字串，如 "24°" 或 "24°C"
 */
export function formatTemp(temp: number, unit = true): string {
  return `${Math.round(temp)}°${unit ? 'C' : ''}`
}

/**
 * 格式化风力与风向
 *
 * @param dir 风向文本（如 "东北风"）
 * @param scale 风力等级（如 "3" 或 "3-4"）
 * @returns 整合文本，如 "东北风 3级"
 */
export function formatWind(dir: string, scale: string): string {
  if (!dir && !scale) return '微风'
  if (!scale) return dir
  const scaleText = scale.endsWith('级') ? scale : `${scale}级`
  return `${dir} ${scaleText}`.trim()
}

/**
 * 格式化更新时间
 *
 * @param timestampMs 毫秒时间戳
 * @returns "更新于 HH:mm:ss" 或 "刚刚更新"
 */
export function formatWeatherUpdatedTime(timestampMs: number): string {
  if (!timestampMs || timestampMs <= 0) return '尚未更新'
  const date = new Date(timestampMs)
  const hours = String(date.getHours()).padStart(2, '0')
  const minutes = String(date.getMinutes()).padStart(2, '0')
  const seconds = String(date.getSeconds()).padStart(2, '0')
  return `更新于 ${hours}:${minutes}:${seconds}`
}
