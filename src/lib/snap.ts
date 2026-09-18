import { DEFAULT_WINDOW_CELLS, floatingWindowSize, layoutFromDocked } from './floating-layout'

export interface PhysicalPoint {
  x: number
  y: number
}

export interface PhysicalSize {
  width: number
  height: number
}

export interface WorkArea {
  position: PhysicalPoint
  size: PhysicalSize
}

export interface SnapOptions {
  threshold?: number
  /** 当前申请的格子数；默认等于悬浮窗默认容量。 */
  cells?: number
}

export interface DockedEdges {
  left: boolean
  top: boolean
  right: boolean
  bottom: boolean
}

export interface SnapResult {
  position: PhysicalPoint
  size: PhysicalSize
  docked: DockedEdges
}

/**
 * 将窗口外框吸附到最近的工作区边缘，并依据吸附到的边缘与格子数调整窗口尺寸：
 * - 上下贴边时沿该边展开为横条（宽 = base × cells）；
 * - 左右贴边时沿该边展开为竖条（高 = base × cells）；
 * - 角落贴边时保持正方形（边长倍率 = ceil(sqrt(cells))）；
 * - 未命中阈值时吸附到距离最近的单条边缘；
 * - 贴边时依据吸附到的边缘恢复对应的窗口尺寸。
 * 返回吸附后的位置、尺寸以及各边缘是否贴边。
 */
export function snapToWorkArea(
  position: PhysicalPoint,
  windowSize: PhysicalSize,
  baseSize: PhysicalSize,
  workArea: WorkArea,
  { threshold = 24, cells = DEFAULT_WINDOW_CELLS }: SnapOptions = {},
): SnapResult {
  // 工作区可位于副屏，因此坐标不能假定从 (0, 0) 开始。
  const areaLeft = workArea.position.x
  const areaTop = workArea.position.y
  const areaRight = areaLeft + workArea.size.width
  const areaBottom = areaTop + workArea.size.height

  // 窗口左上角允许到达的最大坐标，确保窗口右侧和底部不越出工作区。
  // 当窗口大于工作区时，Math.max 会将其固定在工作区左上角。
  const maxX = Math.max(areaLeft, areaRight - windowSize.width)
  const maxY = Math.max(areaTop, areaBottom - windowSize.height)
  // 先将拖拽后的位置限制在工作区内，再以限制后的坐标计算吸附距离。
  let x = clamp(position.x, areaLeft, maxX)
  let y = clamp(position.y, areaTop, maxY)

  const docked: DockedEdges = { left: false, top: false, right: false, bottom: false }

  // 左右方向独立吸附：靠近左边缘时对齐左侧；否则靠近右边缘时，
  // 将窗口左上角移至 maxX，使窗口右侧恰好与工作区右侧重合。
  if (distance(x, areaLeft) <= threshold) {
    x = areaLeft
    docked.left = true
  } else if (distance(x + windowSize.width, areaRight) <= threshold) {
    x = maxX
    docked.right = true
  }

  // 上下方向采用相同规则，因此窗口可同时吸附到任意一个角落。
  if (distance(y, areaTop) <= threshold) {
    y = areaTop
    docked.top = true
  } else if (distance(y + windowSize.height, areaBottom) <= threshold) {
    y = maxY
    docked.bottom = true
  }

  // 阈值没有命中时仍然吸附到最近的一条边。此时只选择一条边，
  // 避免窗口在远离边缘时因为两个坐标轴分别取最近边而意外变成角落形态。
  if (!docked.left && !docked.right && !docked.top && !docked.bottom) {
    const nearest = [
      { edge: 'left' as const, distance: distance(x, areaLeft) },
      { edge: 'right' as const, distance: distance(x + windowSize.width, areaRight) },
      { edge: 'top' as const, distance: distance(y, areaTop) },
      { edge: 'bottom' as const, distance: distance(y + windowSize.height, areaBottom) },
    ].reduce((closest, candidate) => (candidate.distance < closest.distance ? candidate : closest))
    docked[nearest.edge] = true
  }

  const size = dockedSize(baseSize, docked, cells)

  // 贴边后尺寸变化，需重新锚定位置使窗口仍紧贴对应边缘；
  // 未贴边的坐标轴也要依据新尺寸重新限制，避免展开后越出工作区。
  const maxSnappedX = Math.max(areaLeft, areaRight - size.width)
  const maxSnappedY = Math.max(areaTop, areaBottom - size.height)
  if (docked.left) {
    x = areaLeft
  } else if (docked.right) {
    x = clamp(areaRight - size.width, areaLeft, maxSnappedX)
  } else {
    x = clamp(x, areaLeft, maxSnappedX)
  }
  if (docked.top) {
    y = areaTop
  } else if (docked.bottom) {
    y = clamp(areaBottom - size.height, areaTop, maxSnappedY)
  } else {
    y = clamp(y, areaTop, maxSnappedY)
  }

  return { position: { x, y }, size, docked }
}

/**
 * 贴边后的窗口尺寸。
 * 口径统一在 `floatingWindowSize`（同一处也供主窗口的三形态预览使用）：
 * 角 / 方形按边长倍率、上下贴边横条、左右贴边竖条。
 */
function dockedSize(base: PhysicalSize, docked: DockedEdges, cells: number): PhysicalSize {
  return floatingWindowSize(layoutFromDocked(docked), cells, {
    width: base.width,
    height: base.height,
  })
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}

function distance(left: number, right: number): number {
  return Math.abs(left - right)
}
