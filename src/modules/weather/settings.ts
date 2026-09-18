import type { FloatingModuleConfig } from '@/stores/floatingModules'

/**
 * 和风天气组件私有配置（不含敏感 API Key，Key 保存在系统 OS 凭据管理器中）
 */
export interface WeatherSettings {
  /** 是否已在 OS 凭据管理器中存储了 API Key */
  has_key: boolean
  /** API 主机服务类型：'dev' 开发版 | 'commercial' 商业版 | 'custom' 自定义 */
  host_type: 'dev' | 'commercial' | 'custom'
  /** 自定义 API 域名前缀（如选择 custom 时填写的完整 https 前缀） */
  custom_host: string
  /** 定位方式：'auto' 自动 IP 定位 | 'manual' 手动指定城市 */
  location_mode: 'auto' | 'manual'
  /** 手动指定的 LocationID 或城市检索标识 */
  manual_location: string
  /** 手动指定的城市名展示文本（如 "北京市海淀区"） */
  manual_location_name: string
  /** 数据自动刷新周期（分钟）：范围 10..=60 */
  refresh_interval_min: number
}

/**
 * 默认天气组件配置
 */
export const DEFAULT_WEATHER_SETTINGS: WeatherSettings = {
  has_key: false,
  host_type: 'dev',
  custom_host: '',
  location_mode: 'auto',
  manual_location: '',
  manual_location_name: '',
  refresh_interval_min: 15,
}

/**
 * 数值区间限制辅助函数
 */
function clamp(value: unknown, min: number, max: number, fallback: number): number {
  if (typeof value !== 'number' || Number.isNaN(value)) {
    return fallback
  }
  return Math.min(Math.max(value, min), max)
}

/**
 * 从通用的 FloatingModuleConfig 中提取并标准化 weather 模块私有配置
 *
 * @param config 全局悬浮窗模块配置
 * @returns 经过校验与兜底的 WeatherSettings 对象
 */
export function weatherSettings(config: FloatingModuleConfig | null): WeatherSettings {
  const raw = (config?.module_settings?.['weather'] ?? {}) as Record<string, unknown>

  return {
    has_key: Boolean(raw.has_key),
    host_type:
      raw.host_type === 'commercial' ? 'commercial' : raw.host_type === 'custom' ? 'custom' : 'dev',
    custom_host: typeof raw.custom_host === 'string' ? raw.custom_host.trim() : '',
    location_mode: raw.location_mode === 'manual' ? 'manual' : 'auto',
    manual_location: typeof raw.manual_location === 'string' ? raw.manual_location.trim() : '',
    manual_location_name:
      typeof raw.manual_location_name === 'string' ? raw.manual_location_name.trim() : '',
    refresh_interval_min: clamp(raw.refresh_interval_min, 10, 60, 15),
  }
}
