/**
 * 构建期注入的应用元信息。
 *
 * 版本号只有**一个**来源：`package.json`。`vite.config.ts` 在构建 / 开发启动时把它塞进
 * `VITE_APP_VERSION`，因此界面里不写死任何数字 —— 发版只改 package.json。
 */

/** `import.meta.env` 里与本模块相关的字段（收窄成一个接口便于测试时传假值）。 */
export interface AppInfoEnv {
  /** 构建期注入的 package.json 版本号；注入失败 / 非 Vite 环境下可能缺失。 */
  VITE_APP_VERSION?: unknown
}

/** 版本号拿不到时的占位符：宁可显式写「—」，也不要让界面出现 `undefined`。 */
export const UNKNOWN_VERSION = '—'

/**
 * 取应用版本号。
 * 认不出（未注入 / 空串 / 非字符串）时返回 [`UNKNOWN_VERSION`]。
 */
export function appVersion(env: AppInfoEnv = import.meta.env): string {
  const value = env.VITE_APP_VERSION
  return typeof value === 'string' && value.trim() ? value.trim() : UNKNOWN_VERSION
}
