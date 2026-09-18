import type { ComputedRef, InjectionKey } from 'vue'

/**
 * 悬浮窗模块与宿主之间的契约（宿主为每个模块单独注入一份）。
 *
 * 模块的「格子需求」声明在模块定义里（见 `FloatingModuleDefinition.demand`），
 * 不走这里 —— 宿主需要在不挂载模块的情况下也能算出任意形态要多大窗口
 * （主窗口的三形态预览就靠这个）。注入通道只负责两件运行时的事：
 * 告诉模块分到了多少格、把悬停对象转给宿主。
 */
export interface FloatingModuleApi {
  /** 本模块此刻分到的格子数（宿主按需求比例切分窗口，至少 1 格）。 */
  allocatedCells: ComputedRef<number>
  /** 上报悬停对象（key 由模块自定义，宿主只负责转发）；null 表示离开，用于控制信息提示窗。 */
  reportHover: (key: string | null) => void
}

/** 宿主注入给单个模块的上下文 key。 */
export const floatingModuleKey: InjectionKey<FloatingModuleApi> = Symbol('floating-module')

/** 悬停目标：哪个模块的哪个对象（key 由模块自己定义与解释，外壳只当字符串转发）。 */
export interface FloatingHoverTarget {
  /** 触发悬停的模块 id（注册表里的那个）。 */
  moduleId: string
  /** 模块自定义的悬停对象 key。 */
  key: string
}