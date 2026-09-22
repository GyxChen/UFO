import type { DockedEdges, PhysicalPoint, PhysicalSize, WorkArea } from './snap'
import type { FloatingHoverTarget } from './floating-module-context'
import type { FloatingModuleDefinition, FloatingModuleTooltip, FloatingTooltipSize } from './floating-modules'

/** 信息提示窗的窗口 label。 */
export const TOOLTIP_WINDOW_LABEL = 'tooltip'
/** 悬浮窗 → 提示窗的悬停事件名。 */
export const TOOLTIP_HOVER_EVENT = 'tooltip-hover'
/** 提示窗 → 悬浮窗的鼠标进入/离开事件名。 */
export const TOOLTIP_INTERACTION_EVENT = 'tooltip-interaction'

/** 悬浮窗外框的物理矩形。 */
export interface TooltipAnchor {
  x: number
  y: number
  width: number
  height: number
}

/** 悬浮窗发给提示窗的悬停状态；所有坐标均为物理像素。 */
export interface TooltipHoverPayload {
  /** 是否应显示提示窗。 */
  visible: boolean
  /** 悬停目标（哪个模块的哪个对象）；null 表示未指向具体对象。 */
  target: FloatingHoverTarget | null
  /** 悬浮窗外框，提示窗据此定位。 */
  anchor: TooltipAnchor
  /** 悬浮窗当前贴边状态，决定提示窗优先出现在哪一侧。 */
  docked: DockedEdges
}

/**
 * 按悬停目标找出该由哪个模块提供信息卡内容。
 *
 * 提示窗是独立窗口，它只认注册表：目标模块不存在，或模块没有声明「带组件的信息卡」时返回 null
 * （外壳据此不弹窗）。要求组件存在，是为了让「解析出结果」等价于「有东西可渲染」——
 * 否则会弹出一个空白的提示窗。这样新增模块自带信息卡，提示窗零改动。
 */
export function resolveTooltipDefinition(
  definitions: readonly FloatingModuleDefinition[],
  target: FloatingHoverTarget | null | undefined
): FloatingModuleDefinition | null {
  if (!target) return null
  const definition = definitions.find(item => item.id === target.moduleId)
  return definition?.tooltip?.component ? definition : null
}

/** 提示窗相对悬浮窗出现的方向。 */
export type TooltipSide = 'left' | 'right' | 'top' | 'bottom'

export interface TooltipPlacementOptions {
  /** 提示窗与悬浮窗之间的间隙（物理像素）。 */
  gap?: number
  /** 与工作区边缘的最小留白（物理像素）。 */
  margin?: number
}

export interface TooltipPlacement {
  position: PhysicalPoint
  side: TooltipSide
}

/** 提示窗与悬浮窗之间的默认间隙。 */
const DEFAULT_GAP = 8
/** 提示窗与工作区边缘的默认留白。 */
const DEFAULT_MARGIN = 4

/**
 * 提示窗尺寸的地板：再矮 / 再窄也要露得出头部一行与一段文字，
 * 否则（模块算错或工作区极小）窗口会塌成一条线，看上去像没弹。
 */
const MIN_TOOLTIP_WIDTH = 200
const MIN_TOOLTIP_HEIGHT = 88

/**
 * 取模块为这次悬停声明的提示窗尺寸；没声明（或声明成函数但这次没有悬停目标）时返回 null，
 * 调用方沿用窗口的默认尺寸。
 *
 * 提示窗只认「模块 → 尺寸」这一步，不认识尺寸是怎么算出来的 ——
 * 折叠清单那类「行数随目标变化」的内容因此可以把「要几行」的逻辑留在模块自己手里。
 */
export function resolveTooltipSize(
  tooltip: FloatingModuleTooltip | null | undefined,
  target: FloatingHoverTarget | null | undefined
): FloatingTooltipSize | null {
  const declared = tooltip?.size
  if (!declared) return null
  if (typeof declared === 'function') {
    return target ? declared(target) : null
  }
  return declared
}

/**
 * 把模块声明的尺寸夹进工作区：内容再多也不能让提示窗比屏幕还高
 * （超出的部分由内容自己滚，见 agent 折叠清单的列表容器）。
 *
 * 上下各留 `margin`，与 `placeTooltip` 的留白同一口径；工作区小于地板时取地板
 * —— 此时窗口必然越界，交给 `placeTooltip` 的夹取兜底，至少还能看见左上角那块内容。
 */
export function clampTooltipSize(
  size: FloatingTooltipSize,
  workAreaSize: PhysicalSize,
  margin: number = DEFAULT_MARGIN
): FloatingTooltipSize {
  const maxWidth = Math.max(MIN_TOOLTIP_WIDTH, Math.floor(workAreaSize.width - margin * 2))
  const maxHeight = Math.max(MIN_TOOLTIP_HEIGHT, Math.floor(workAreaSize.height - margin * 2))
  return {
    width: Math.min(Math.max(Math.round(size.width), MIN_TOOLTIP_WIDTH), maxWidth),
    height: Math.min(Math.max(Math.round(size.height), MIN_TOOLTIP_HEIGHT), maxHeight),
  }
}

/**
 * 计算提示窗的物理坐标。
 *
 * 优先放在悬浮窗「未贴边」的一侧，避免遮挡贴边方向；
 * 首选侧放不下时按候选顺序换侧；全部放不下则退回首选侧并夹进工作区。
 */
export function placeTooltip(
  anchor: TooltipAnchor,
  tooltip: PhysicalSize,
  workArea: WorkArea,
  docked: DockedEdges,
  { gap = DEFAULT_GAP, margin = DEFAULT_MARGIN }: TooltipPlacementOptions = {}
): TooltipPlacement {
  const areaLeft = workArea.position.x
  const areaTop = workArea.position.y
  const areaRight = areaLeft + workArea.size.width
  const areaBottom = areaTop + workArea.size.height

  for (const side of candidateSides(docked)) {
    const position = positionForSide(side, anchor, tooltip, gap)
    if (fits(position, tooltip, areaLeft, areaTop, areaRight, areaBottom, margin)) {
      return { position: roundPoint(position), side }
    }
  }

  // 全部放不下（例如提示窗比工作区还大）：退回首选侧后夹进工作区，
  // 用 Math.max 保证不会越出左上角（与 snap.ts 同款兜底）。
  const side = candidateSides(docked)[0]
  const fallback = positionForSide(side, anchor, tooltip, gap)
  return { position: roundPoint(clampToArea(fallback, tooltip, workArea, margin)), side }
}

/**
 * 候选方向顺序：优先远离悬浮窗贴边的方向。
 *
 * 贴左 → 优先往右；贴右 → 优先往左；贴上 → 优先往下；贴下 → 优先往上。
 * 角落（两轴同时贴边）时两个逃逸方向都排在前，水平优先。
 */
function candidateSides(docked: DockedEdges): TooltipSide[] {
  if (!docked.left && !docked.right && !docked.top && !docked.bottom) {
    return ['right', 'left', 'bottom', 'top']
  }

  const sides: TooltipSide[] = []
  if (docked.left && !docked.right) sides.push('right')
  else if (docked.right && !docked.left) sides.push('left')
  if (docked.top && !docked.bottom) sides.push('bottom')
  else if (docked.bottom && !docked.top) sides.push('top')

  // 补齐其余方向作为兜底。
  for (const side of ['right', 'left', 'bottom', 'top'] as TooltipSide[]) {
    if (!sides.includes(side)) sides.push(side)
  }
  return sides
}

/**
 * 某一侧的候选坐标。
 * 非主轴上与悬浮窗居中对齐，避免横条 / 竖条形态下提示窗偏在一角。
 */
function positionForSide(side: TooltipSide, anchor: TooltipAnchor, tooltip: PhysicalSize, gap: number): PhysicalPoint {
  const centeredX = anchor.x + (anchor.width - tooltip.width) / 2
  const centeredY = anchor.y + (anchor.height - tooltip.height) / 2
  switch (side) {
    case 'right':
      return { x: anchor.x + anchor.width + gap, y: centeredY }
    case 'left':
      return { x: anchor.x - tooltip.width - gap, y: centeredY }
    case 'bottom':
      return { x: centeredX, y: anchor.y + anchor.height + gap }
    case 'top':
      return { x: centeredX, y: anchor.y - tooltip.height - gap }
  }
}

/** 判断提示窗是否能完整落在工作区内（含留白）。 */
function fits(
  position: PhysicalPoint,
  size: PhysicalSize,
  areaLeft: number,
  areaTop: number,
  areaRight: number,
  areaBottom: number,
  margin: number
): boolean {
  return (
    position.x >= areaLeft + margin &&
    position.y >= areaTop + margin &&
    position.x + size.width <= areaRight - margin &&
    position.y + size.height <= areaBottom - margin
  )
}

/** 将坐标夹进工作区；提示窗大于工作区时固定在工作区左上角。 */
function clampToArea(position: PhysicalPoint, size: PhysicalSize, workArea: WorkArea, margin: number): PhysicalPoint {
  const areaLeft = workArea.position.x
  const areaTop = workArea.position.y
  const maxX = Math.max(areaLeft + margin, areaLeft + workArea.size.width - margin - size.width)
  const maxY = Math.max(areaTop + margin, areaTop + workArea.size.height - margin - size.height)
  return {
    x: clamp(position.x, areaLeft + margin, maxX),
    y: clamp(position.y, areaTop + margin, maxY),
  }
}

/** 物理坐标必须是整数，居中计算可能产生小数。 */
function roundPoint(point: PhysicalPoint): PhysicalPoint {
  return { x: Math.round(point.x), y: Math.round(point.y) }
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}
