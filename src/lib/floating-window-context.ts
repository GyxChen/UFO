import type { InjectionKey, Ref } from 'vue'
import type { FloatingLayout } from '@/lib/floating-modules'

/** 悬浮窗壳提供给内部模块的尺寸控制接口。 */
export interface FloatingWindowApi {
  layoutMode: Ref<FloatingLayout>
  /** 按格子数申请窗口尺寸；壳负责 clamp 与贴边重算。 */
  setWindowCells: (cells: number) => void
}

export const floatingWindowKey: InjectionKey<FloatingWindowApi> = Symbol('floating-window')
