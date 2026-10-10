/**
 * 入口的导出形状（模板本地的声明，给编辑器与类型检查看；宿主侧的真判据在 UFO 仓库的 `plugin-runtime.ts`）。
 * 只写形状、不引宿主的任何类型 —— 这份模板要能被拷出仓库独立编译。
 */
import type { Component } from 'vue'

/** 宿主在每个回调里给的悬停目标。 */
interface HoverTarget {
  moduleId: string
  key: string
}

/** 主窗一级页签里的一个分区。 */
interface Section {
  id: string
  label: string
  component: Component
  fill?: boolean
}

declare const entry: {
  cell: Component
  hasContent?: (target: HoverTarget) => boolean
  tooltip?: Component
  tooltipSize?: { width: number; height: number }
  extension?: Component
  extensionHasContent?: (target: HoverTarget) => boolean
  settings?: Section[]
  activate?: (host: unknown) => void | (() => void)
}

export default entry
