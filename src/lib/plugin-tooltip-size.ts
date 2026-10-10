import { shallowRef } from 'vue'
import { PLUGIN_LIMITS } from './plugin-limits'

/** 一张卡的目标尺寸（物理像素）。 */
export interface TooltipSizeJson {
  width: number
  height: number
}

/**
 * 组件包**运行时**申请的信息卡尺寸，按包 id 存。
 *
 * 为什么这条留在前端而不是加一条 IPC 命令：这张卡就渲染在本窗口里，尺寸是纯界面状态；
 * 绕一圈 Rust 只是多出一条能被滥用的命令，而该做的夹取在这里一样做得到。
 *
 * ⚠️ 区间刻意复用清单那一套常量（`PLUGIN_LIMITS.*_TOOLTIP_*`）：
 * 「清单里能声明的」与「运行时能改的」必须是同一个区间，否则等于开了第二条更宽的口子。
 * 宿主提示窗还会再按工作区夹一次，所以这块不可能盖满屏幕。
 */
const sizes = shallowRef<Record<string, TooltipSizeJson>>({})

/** 把请求尺寸夹进区间（四舍五入到整像素）。 */
function clampSize(size: TooltipSizeJson): TooltipSizeJson {
  return {
    width: Math.round(
      Math.min(Math.max(size.width, PLUGIN_LIMITS.MIN_TOOLTIP_WIDTH), PLUGIN_LIMITS.MAX_TOOLTIP_WIDTH)
    ),
    height: Math.round(
      Math.min(Math.max(size.height, PLUGIN_LIMITS.MIN_TOOLTIP_HEIGHT), PLUGIN_LIMITS.MAX_TOOLTIP_HEIGHT)
    ),
  }
}

/** 记下这个包要的卡尺寸，返回实际生效的那一份（被夹过就如实报给作者）。 */
export function setPluginTooltipSize(pluginId: string, size: TooltipSizeJson): TooltipSizeJson {
  const applied = clampSize(size)
  sizes.value = { ...sizes.value, [pluginId]: applied }
  return applied
}

/** 这个包在运行时改过卡尺寸吗；没改过返回 null（那就用清单声明的那份）。 */
export function pluginTooltipSizeOf(pluginId: string): TooltipSizeJson | null {
  return sizes.value[pluginId] ?? null
}

/** 响应式源：提示窗用它来监听「正显示的那张卡改尺寸了」。 */
export const pluginTooltipSizes = sizes

/** 清掉全部（装载链路重跑与单测复位用）。 */
export function clearPluginTooltipSizes(): void {
  sizes.value = {}
}
