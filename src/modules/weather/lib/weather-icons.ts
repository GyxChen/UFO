import { h, type VNode } from 'vue'

/**
 * 极简深色控制台 SVG 图标生成器
 * 针对和风常见天气 Code 分流映射：
 * 100~104: 晴 / 多云 / 阴
 * 300~399: 雨系列
 * 400~499: 雪系列
 * 500~599: 雾霾系列
 */

/**
 * 晴天图标 (100)
 */
export function SunnyIcon(): VNode {
  return h(
    'svg',
    {
      viewBox: '0 0 24 24',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': '2',
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    },
    [
      h('circle', { cx: '12', cy: '12', r: '4' }),
      h('path', { d: 'M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41' }),
    ],
  )
}

/**
 * 多云图标 (101, 102, 103)
 */
export function CloudyIcon(): VNode {
  return h(
    'svg',
    {
      viewBox: '0 0 24 24',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': '2',
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    },
    [
      h('path', { d: 'M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z' }),
    ],
  )
}

/**
 * 阴天图标 (104)
 */
export function OvercastIcon(): VNode {
  return h(
    'svg',
    {
      viewBox: '0 0 24 24',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': '2',
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    },
    [
      h('path', { d: 'M4 14.899A7 7 0 1 1 15.71 8h1.79a4.5 4.5 0 0 1 2.5 8.242' }),
      h('path', { d: 'M16 20H7a5 5 0 0 1-.5-9.975' }),
    ],
  )
}

/**
 * 下雨图标 (300~399)
 */
export function RainIcon(): VNode {
  return h(
    'svg',
    {
      viewBox: '0 0 24 24',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': '2',
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    },
    [
      h('path', { d: 'M4 14.899A7 7 0 1 1 15.71 8h1.79a4.5 4.5 0 0 1 2.5 8.242' }),
      h('path', { d: 'M16 14v6M8 14v6M12 16v6' }),
    ],
  )
}

/**
 * 下雪图标 (400~499)
 */
export function SnowIcon(): VNode {
  return h(
    'svg',
    {
      viewBox: '0 0 24 24',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': '2',
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    },
    [
      h('path', { d: 'M4 14.899A7 7 0 1 1 15.71 8h1.79a4.5 4.5 0 0 1 2.5 8.242' }),
      h('path', { d: 'm8 15 2 2M8 17l2-2M14 15l2 2M14 17l2-2M11 19l2 2M11 21l2-2' }),
    ],
  )
}

/**
 * 雾霾图标 (500~599)
 */
export function FogIcon(): VNode {
  return h(
    'svg',
    {
      viewBox: '0 0 24 24',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': '2',
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    },
    [
      h('path', { d: 'M4 14h16M4 18h16M7 10h10M9 6h6' }),
    ],
  )
}

/**
 * 根据和风天气代码解析对应的 SVG 图标组件
 *
 * @param code 和风天气状态码（如 "100", "101", "305" 等）
 * @returns 对应的 Vue 渲染函数
 */
export function resolveWeatherIcon(code: string | undefined): () => VNode {
  if (!code) return CloudyIcon
  const num = parseInt(code, 10)
  if (Number.isNaN(num)) return CloudyIcon

  if (num === 100) {
    return SunnyIcon
  }
  if (num >= 101 && num <= 103) {
    return CloudyIcon
  }
  if (num === 104) {
    return OvercastIcon
  }
  if (num >= 300 && num <= 399) {
    return RainIcon
  }
  if (num >= 400 && num <= 499) {
    return SnowIcon
  }
  if (num >= 500 && num <= 599) {
    return FogIcon
  }

  return CloudyIcon
}
