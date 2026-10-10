/**
 * 插件体系的数据形状（前端侧）。
 *
 * 这些 interface 是 Rust 序列化结果的**镜像**，字段名按 serde 的 camelCase 写：
 * 结构性变更要两边一起改，`plugin-types.spec.ts` 锁住「清单字段清单」与 Rust 的一致性。
 * 数字上限不在这里（见 `plugin-limits.ts`），文案不在这里（见 `plugin-capabilities.ts`）。
 */

import type { FloatingTooltipSize } from '@/lib/floating-modules'

/** `surfaces.tooltip`：插件自己的独立信息卡声明。 */
export interface PluginOwnTooltipJson {
  /** 显式开关；写了对象但 `own: false` = 没有卡。 */
  own: boolean
  /** 期望尺寸 `[宽, 高]`（物理像素），未给 = 用宿主默认。 */
  size?: [number, number] | null
}

/** `surfaces.extension`：往别的组件信息卡尾部追加一块。 */
export interface PluginExtensionJson {
  /** 目标组件 id。 */
  into: string
  /** 最多贡献几行（宿主夹到 3）。 */
  maxRows: number
}

/** `surfaces`：界面挂载点的声明。 */
export interface PluginSurfacesJson {
  /** 悬浮窗申请几格；`null` = 不占格子。 */
  cell?: number | null
  tooltip?: PluginOwnTooltipJson | null
  extension?: PluginExtensionJson | null
  /** 主窗一级页签里是否有自己的设置页。 */
  settings?: boolean
}

/** `plugin.json` 本体（Rust `manifest::Manifest` 的镜像）。 */
export interface PluginManifestJson {
  manifestVersion?: number
  apiVersion?: number
  id: string
  name: string
  version: string
  description: string
  entry: string
  surfaces: PluginSurfacesJson
  permissions: string[]
  hosts: string[]
}

/** 台账里一条已装记录（Rust `plugin::store::PluginRecord` 的镜像）。 */
export interface PluginRecordJson {
  id: string
  /** 展示名：清单有效时取清单，坏包尽力从原始 JSON 读，读不到是目录名。 */
  name: string
  /** 展示版本号（尽力而为，可能为空串）。 */
  version: string
  /** 校验并夹取后的清单；`null` = 校验失败，这条挂不上。 */
  manifest: PluginManifestJson | null
  /** 包目录的绝对路径（只用于显示；前端绝不拿它拼路径）。 */
  dir: string
  /** 来源：`local` = 就地装载用户目录，`copied` = 已拷进 app-data。 */
  source: 'local' | 'copied'
  /** 逐包热重载开关；⚠️ Rust 侧不变量：为真 ⇒ 必须是 local 源。 */
  devReload: boolean
  /** 校验时的夹取/忽略告警，管理区详情逐条显示。 */
  warnings: string[]
  /** 包内文件数。 */
  files: number
  /** 包总体积（字节）。 */
  bytes: number
  /** 上次装载耗时（毫秒）；0 = 还没装载过。 */
  loadMs: number
  /** 校验失败的原因（中文，直接渲染在列表行；成功时是 null）。 */
  error: string | null
}

/** 一次台账读取 / 变更的返回（Rust `commands::LedgerBroadcast` 的镜像）。 */
export interface LedgerBroadcastJson {
  /** 台账版本：每次写盘 +1，前端用它丢弃迟到的广播。 */
  revision: number
  /** 全量记录（含校验失败的包）。 */
  records: PluginRecordJson[]
}

/** 能力 / 挂载点的一条事实（Rust `capability::Capability` 序列化后的形状）。 */
export interface CapabilityFactJson {
  id: string
  /** `permission`（宿主能力）/ `surface`（界面挂载点）。 */
  kind: string
  label: string
  icon: string
  tone: string
  disclosure: string
}

/** 一条插件日志（`ufo.log` 落进环形缓冲 / 盘上 JSONL 后的形状）。 */
export interface PluginLogLineJson {
  /** 毫秒时间戳。 */
  ts: number
  /** 级别：与状态色三档同名。 */
  level: 'info' | 'warn' | 'error'
  /** 消息正文（已按 LOG_LINE_CHARS 截断）。 */
  msg: string
  /** 结构化字段：只允许标量，值原样显示。 */
  fields?: Record<string, string | number | boolean> | null
  /** 盘上坐标（导出与「同一次调用的上下游」用它定位）。 */
  traceId?: string | null
}

/** 调试卡「最近日志」的行数：这是界面预算，不是宿主上限，所以不进 Rust 表。 */
export const RECENT_LOG_LINES = 3

/**
 * 从清单算出该包申请的挂载点 id（顺序与 Rust `Manifest::declared_surfaces` 一致）。
 *
 * ⚠️ 徽标顺序在界面上是稳定的：同一份清单，前端算出来的顺序必须与 Rust 命令返回的一致，
 * 否则「列表里徽标的先后」会随渲染路径变（`plugin-types.spec.ts` 锁住这张表）。
 */
export function declaredSurfaceIds(manifest: PluginManifestJson): string[] {
  const ids: string[] = []
  if (manifest.surfaces.cell != null) ids.push('cell')
  if (manifest.surfaces.tooltip?.own) ids.push('tooltip-own')
  if (manifest.surfaces.extension != null) ids.push('tooltip-extension')
  if (manifest.surfaces.settings) ids.push('settings')
  return ids
}

/** 把清单声明的尺寸换成外壳用的尺寸对象；没声明返回 null（回落 tauri.conf.json 的默认值）。 */
export function tooltipSizeOf(manifest: PluginManifestJson): FloatingTooltipSize | null {
  const size = manifest.surfaces.tooltip?.size
  if (!size) return null
  return { width: size[0], height: size[1] }
}
