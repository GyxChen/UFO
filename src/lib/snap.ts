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
}

/**
 * Snaps a window's outer position to the nearest work-area edge when close
 * enough, while keeping the whole window inside the work area when possible.
 */
export function snapToWorkArea(
  position: PhysicalPoint,
  windowSize: PhysicalSize,
  workArea: WorkArea,
  { threshold = 24 }: SnapOptions = {},
): PhysicalPoint {
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

  // 左右方向独立吸附：靠近左边缘时对齐左侧；否则靠近右边缘时，
  // 将窗口左上角移至 maxX，使窗口右侧恰好与工作区右侧重合。
  if (distance(x, areaLeft) <= threshold) {
    x = areaLeft
  } else if (distance(x + windowSize.width, areaRight) <= threshold) {
    x = maxX
  }

  // 上下方向采用相同规则，因此窗口可同时吸附到任意一个角落。
  if (distance(y, areaTop) <= threshold) {
    y = areaTop
  } else if (distance(y + windowSize.height, areaBottom) <= threshold) {
    y = maxY
  }

  return { x, y }
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}

function distance(left: number, right: number): number {
  return Math.abs(left - right)
}
