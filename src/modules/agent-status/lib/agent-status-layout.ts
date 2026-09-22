import { ABS_MAX_CELLS, CELL_STEPS, cellsGrid, clampCells, snapToStep, type FloatingLayout } from '@/lib/floating-layout'

/**
 * Agent 状态灯模块自己的布局模型：
 * 「形态 + 会话数」→ 上报给宿主的格子需求（容量），以及把会话切成可见灯 + `+N` 溢出格。
 * 与模块无关的形态 / 格子换算（阶梯档位、网格行列）在 `floating-layout`。
 */

/** 形态的地板：贴角最少 1×2（2 格，用户要的「最少 1×2」）；方形 / 边条可到 1 格。 */
function layoutFloor(layout: FloatingLayout): number {
  return layout === 'corner' ? 2 : 1
}

/**
 * 该形态的有效格子上限：把配置上限**向下**夹到最近的阶梯档位，封顶 9、且不低于形态地板。
 *
 * 这样窗口尺寸永远是合法档位（贴角才是 1×2…3×3 的网格）：配置里写 7 / 8 也归一到 6 / 9，
 * 不会出现「8 格」这种没有对应形状的值把贴角撑成奇怪的比例。
 */
export function effectiveLayoutMax(layout: FloatingLayout, layoutMax: number): number {
  const capped = clampCells(layoutMax, 1, ABS_MAX_CELLS)
  let step = 1
  for (const candidate of CELL_STEPS) {
    if (candidate <= capped) step = candidate
  }
  return Math.max(step, layoutFloor(layout))
}

/**
 * Agent 状态灯的格子需求（容量）：按形态把会话数归一到阶梯档位。
 * - 边条（横条 / 竖条）：按会话个数**精确**取长度（用户要「实际显示按个数」），封顶有效上限；
 * - 贴角：向上夹到阶梯档位，最少 1×2（2 格）；
 * - 方形：向上夹到阶梯档位，可到 1×1。
 *
 * 纯函数：只读会话数与配置上限，**不读宿主分配了多少格**（读 `allocatedCells` 会形成尺寸回路）。
 */
export function agentStatusCapacity(layout: FloatingLayout, sessionCount: number, layoutMax: number): number {
  const max = effectiveLayoutMax(layout, layoutMax)
  const count = Number.isFinite(sessionCount) ? Math.max(0, Math.floor(sessionCount)) : 0
  if (layout === 'horizontal' || layout === 'vertical') {
    return clampCells(count, 1, max)
  }
  // 方形 / 贴角：先夹到 [地板, 上限]，再向上归一到阶梯档位（结果必 ≤ 上限，见 effectiveLayoutMax）。
  return snapToStep(clampCells(count, layoutFloor(layout), max))
}

/**
 * 当前形态下分到的格子能放下几盏灯（含溢出格位）。
 * 统一按网格行列乘积算：边条 = cells，方形 / 贴角 = 档位的 rows×cols。
 */
export function indicatorCapacity(layout: FloatingLayout, cells: number = ABS_MAX_CELLS): number {
  const grid = cellsGrid(layout, cells)
  return grid.rows * grid.cols
}

/**
 * 按容量切分会话：放得下则全显；超出时留出最后一格给 `+N`。
 */
export function partitionIndicatorSessions<T>(
  sessions: readonly T[],
  layout: FloatingLayout,
  cells: number = ABS_MAX_CELLS
): { visible: T[]; overflow: number } {
  const capacity = indicatorCapacity(layout, cells)
  if (sessions.length <= capacity) {
    return { visible: [...sessions], overflow: 0 }
  }
  const visibleCount = Math.max(capacity - 1, 0)
  return {
    visible: sessions.slice(0, visibleCount),
    overflow: sessions.length - visibleCount,
  }
}
