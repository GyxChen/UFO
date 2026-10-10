/**
 * 插件产物的装载运行时：URL 拼接、宿主全局注入、入口 `import()`。
 *
 * 三条不能省的规矩：
 * 1. **逐段编码**。整串 `encodeURIComponent('assets/chunk.js')` 会把 `/` 也编掉，
 *    于是相对说明符在浏览器里解析不回包内 —— 表现是「只有单文件包能用」。
 * 2. **Vue / Pinia 的全局注入必须早于 `app.mount`**。插件产物 external 掉 Vue，
 *    运行时从 `window.__UFO_VUE__` 取；晚一步就是冷启动拿到 undefined，
 *    而 HMR 之后又「好了」—— 那是最难查的一类不对称。
 * 3. **入口失败要有形状**。`import()` 的失败分三类（取不到 / 语法错 / 顶层抛错），
 *    管理区要能分别说清，否则作者只能瞎猜。
 */

import type { Component } from 'vue'
import * as Vue from 'vue'
import * as Pinia from 'pinia'
import type { FloatingTooltipSize, FloatingModuleSection } from '@/lib/floating-modules'
import type { PluginHost } from '@/lib/plugin-host'

/**
 * 协议基址。
 *
 * ⚠️ Windows 上 WebView2 把自定义协议呈递成 `http://ufo-plugin.localhost/…`，
 * macOS / Linux 才是 `ufo-plugin://…`。写死任一种都会在另一个平台上「所有产物 400」。
 */
export const PLUGIN_ORIGIN =
  typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent)
    ? 'http://ufo-plugin.localhost'
    : 'ufo-plugin://'

/**
 * 把一个包内相对路径拼成可 `import()` 的 URL（逐段编码，保留 `/` 结构）。
 *
 * `version` 只给开发期热重载用：ESM 的 `import()` 按 URL 缓存，同一个 URL 第二次拿到的
 * 是同一个模块实例 —— 不带版本串的话「热重载」表现是「重扫成功了但界面一行没变」。
 */
export function pluginAssetUrl(id: string, path: string, version?: string): string {
  const segments = path.split('/').filter(Boolean).map(encode)
  if (segments.length === 0) return ''
  const query = version ? `?v=${encodeURIComponent(version)}` : ''
  return `${PLUGIN_ORIGIN}/${encode(id)}/${segments.join('/')}${query}`
}

/** 百分号编码一段路径；`encode` 单独抽出来是为了让「逐段」这件事看得出来。 */
function encode(segment: string): string {
  return encodeURIComponent(segment)
}

/**
 * 插件入口默认导出的形状（作者侧 `export default { ... }`）。
 *
 * 每一项都对应清单 `surfaces` 的一个声明：清单没声明的，外壳不会去找，也渲染不出来。
 */
export interface PluginUiEntry {
  /** 悬浮窗格子里渲染的组件（清单声明 `surfaces.cell` 时才有意义）。 */
  cell?: Component
  /** 独立信息卡内容组件（清单声明 `surfaces.tooltip.own` 时才有意义）。 */
  tooltip?: Component
  /** 信息卡尺寸：可以固定给，也可以按悬停目标算（外壳夹进工作区）。 */
  tooltipSize?: FloatingTooltipSize | ((target: unknown) => FloatingTooltipSize)
  /** 「此刻有没有内容」：假 = 整卡不弹，不留一块空的糊在屏幕上。 */
  hasContent?: (target: unknown) => boolean
  /** 插进别的组件信息卡尾部的那一块（清单声明 `surfaces.extension` 时才有意义）。 */
  extension?: Component
  /** 插入块此刻有没有内容可展示；假 = 整块不出现（与 `hasContent` 对独立卡的作用一致）。 */
  extensionHasContent?: (target: unknown) => boolean
  /** 主窗设置页的分区（清单声明 `surfaces.settings` 时才有意义）。 */
  settings?: FloatingModuleSection[]
  /**
   * 生命周期：外壳在每个挂模块的窗口调一次，并把这一包的宿主对象交给它。
   * 必须走这里而不是组件的 `onMounted` —— 提示窗的 `hasContent` 在组件挂载之前就要问有没有内容。
   * 组件里也可以不接参数：`inject(PLUGIN_HOST_KEY)` 拿到的是同一个 host。
   */
  activate?: (host: PluginHost) => void | (() => void)
}

/** 入口装载的三种失败（`null` = 成功）。 */
export type PluginEntryErrorKind = 'missing' | 'syntax' | 'runtime'

/** 入口装载结果：成功带 entry，失败带中文原因。 */
export type PluginEntryResult =
  | { ok: true; entry: PluginUiEntry; loadMs: number }
  | { ok: false; kind: PluginEntryErrorKind; reason: string; loadMs: number }

/**
 * 取并执行一个插件入口。
 *
 * 单独成一个可注入的函数（`plugin-loader.ts` 的 `importEntry` 参数）：
 * vitest 里 `import()` 一个自定义协议 URL 走不通，真产物只能在 bench 台量。
 */
export async function importPluginEntry(
  id: string,
  entry: string,
  version?: string
): Promise<PluginEntryResult> {
  const url = pluginAssetUrl(id, entry, version)
  if (!url) {
    return { ok: false, kind: 'missing', reason: `入口「${entry}」拼不出合法 URL`, loadMs: 0 }
  }
  const startedAt = performance.now()
  try {
    const module = (await import(/* @vite-ignore */ url)) as { default?: PluginUiEntry }
    const loadMs = Math.round(performance.now() - startedAt)
    if (!module || typeof module.default !== 'object' || module.default === null) {
      return {
        ok: false,
        kind: 'syntax',
        reason: '入口没有默认导出（应写 `export default { cell() {...} }`）',
        loadMs,
      }
    }
    return { ok: true, entry: module.default, loadMs }
  } catch (error) {
    const loadMs = Math.round(performance.now() - startedAt)
    return { ok: false, ...classify(error, url, entry), loadMs }
  }
}

/** 把 `import()` 抛上来的东西分档：作者看到的三句话要各不相同。 */
function classify(error: unknown, url: string, entry: string): { kind: PluginEntryErrorKind; reason: string } {
  const text = error instanceof Error ? `${error.name}: ${error.message}` : String(error)
  // 取不到（404 / 协议没起 / 文件不在包里）：先让作者查路径。
  if (/TypeError|Failed to fetch|Module not found|network/i.test(text)) {
    return { kind: 'missing', reason: `取不到入口 ${entry}（${url}）：检查包目录里有没有这个文件` }
  }
  // 语法错：解析阶段就炸，产物根本没跑起来。
  if (/SyntaxError/i.test(text)) {
    return { kind: 'syntax', reason: `${entry} 解析失败：${text}` }
  }
  return { kind: 'runtime', reason: `${entry} 顶层执行抛错：${text}` }
}

/** 宿主挂给插件产物的全局键（作者 SDK 里的 `window.__UFO_VUE__` 同一支）。 */
export const VUE_GLOBAL_KEY = '__UFO_VUE__'

/** 宿主挂给插件产物的 Pinia 全局键。 */
export const PINIA_GLOBAL_KEY = '__UFO_PINIA__'

/**
 * 把 Vue / Pinia 交给插件产物（保证「全场只有一个 Vue 实例」）。
 *
 * ⚠️ 调用时机：`main.ts` 里必须**早于** `app.mount()` 与任何入口 `import()`。
 * 多实例的坏法是 effect scope / provide-inject 静默错乱，不会报错。
 */
export function installPluginGlobals(target: Record<string, unknown> = globalThis as unknown as Record<string, unknown>) {
  target[VUE_GLOBAL_KEY] = Vue
  target[PINIA_GLOBAL_KEY] = Pinia
}
