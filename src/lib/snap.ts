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
  const areaLeft = workArea.position.x
  const areaTop = workArea.position.y
  const areaRight = areaLeft + workArea.size.width
  const areaBottom = areaTop + workArea.size.height

  const maxX = Math.max(areaLeft, areaRight - windowSize.width)
  const maxY = Math.max(areaTop, areaBottom - windowSize.height)
  let x = clamp(position.x, areaLeft, maxX)
  let y = clamp(position.y, areaTop, maxY)

  if (distance(x, areaLeft) <= threshold) {
    x = areaLeft
  } else if (distance(x + windowSize.width, areaRight) <= threshold) {
    x = maxX
  }

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
