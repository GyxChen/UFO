import type { FloatingModuleConfig } from '@/stores/floatingModules'

/**
 * 待办组件私有配置（不含凭据：滴答 API Token 存在系统 OS 凭据管理器里）。
 *
 * 字段名 / 默认值 / 取值范围与 Rust 侧 `src-tauri/src/modules/todo/settings.rs` 逐字一致。
 */
export interface TodoSettings {
  /** 是否已在 OS 凭据管理器中保存了个人 API Token（只做界面状态，真实校验在后端） */
  has_token: boolean
  /** 只统计哪个清单的任务：空串 = 全部清单；`inbox` = 收件箱；其余 = 滴答的清单 id */
  project_id: string
  /** 上面那个清单的名字，仅用于界面回显（后端不解释它） */
  project_name: string
  /** 数据自动刷新周期（分钟）：范围 5..=60 */
  refresh_interval_min: number
}

/** 刷新周期的上下限；前端 clamp 与 Rust 侧 `TodoSettings::from_block` 用的是同一对数。 */
export const MIN_REFRESH_INTERVAL_MIN = 5
export const MAX_REFRESH_INTERVAL_MIN = 60

/**
 * 默认待办组件配置：未配置 Token、监控全部清单、15 分钟刷新一次。
 */
export const DEFAULT_TODO_SETTINGS: TodoSettings = {
  has_token: false,
  project_id: '',
  project_name: '',
  refresh_interval_min: 15,
}

/** 把未知值夹成 [min, max] 的整数；类型不对 / 非有限数时回落 fallback。 */
function clampInt(value: unknown, min: number, max: number, fallback: number): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return fallback
  return Math.min(Math.max(Math.round(value), min), max)
}

/** 字符串字段的宽松取值：非字符串 / 全空白一律回落 fallback。 */
function readText(value: unknown, fallback: string): string {
  return typeof value === 'string' ? value.trim() : fallback
}

/**
 * 从通用的 FloatingModuleConfig 中提取并标准化 todo 模块私有配置。
 *
 * 逐字段宽松取值：某个字段写错只影响它自己，不让整块配置失效。
 *
 * @param config 全局悬浮窗模块配置（未加载时为 null）
 * @returns 经过校验与兜底的 TodoSettings
 */
export function todoSettings(config: FloatingModuleConfig | null): TodoSettings {
  const raw = (config?.module_settings?.['todo'] ?? {}) as Record<string, unknown>

  return {
    has_token: Boolean(raw.has_token),
    project_id: readText(raw.project_id, DEFAULT_TODO_SETTINGS.project_id),
    project_name: readText(raw.project_name, DEFAULT_TODO_SETTINGS.project_name),
    refresh_interval_min: clampInt(
      raw.refresh_interval_min,
      MIN_REFRESH_INTERVAL_MIN,
      MAX_REFRESH_INTERVAL_MIN,
      DEFAULT_TODO_SETTINGS.refresh_interval_min,
    ),
  }
}
