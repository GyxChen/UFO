import {
  ABS_MAX_CELLS,
  CELL_STEPS,
  cellsGrid,
  clampCells,
  snapToStep,
  type FloatingLayout,
} from '@/lib/floating-layout'
import { colorForSession, type SessionSnapshot, type StatusColor } from './agent-status'

/**
 * Agent 状态灯模块自己的布局模型：
 * 「形态 + 会话数」→ 上报给宿主的格子需求（容量），以及把会话切成可见灯 + `+N` 溢出格。
 * 与模块无关的形态 / 格子换算（阶梯档位、网格行列）在 `floating-layout`。
 *
 * 溢出格（方案 B「分片环」）的三块纯逻辑也在这里：
 * - 悬停 key 的编解码（`overflowHoverKey` / `parseOverflowHoverKey`）；
 * - 折叠会话按灯色的分片（`overflowSegments`）与它对应的 `conic-gradient`（`overflowRingGradient`）；
 * - 由悬停 key 还原折叠清单（`overflowListFor`）。
 * 全是纯函数：悬浮窗、信息卡、测试用同一份口径，屏上的环与悬停清单不会各算各的。
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
 *
 * 除了可见的那批（`visible`）与折叠数量（`overflow`），还返回**被折叠的会话本身**（`hidden`）——
 * 溢出格的占比环与悬停清单都靠它，两者因此永远同源。`overflow` 恒等于 `hidden.length`，
 * 保留成数字是为了读屏文案与模板里少写一次 `.length`。
 */
export function partitionIndicatorSessions<T>(
  sessions: readonly T[],
  layout: FloatingLayout,
  cells: number = ABS_MAX_CELLS
): { visible: T[]; hidden: T[]; overflow: number } {
  const capacity = indicatorCapacity(layout, cells)
  if (sessions.length <= capacity) {
    return { visible: [...sessions], hidden: [], overflow: 0 }
  }
  const visibleCount = Math.max(capacity - 1, 0)
  const hidden = sessions.slice(visibleCount)
  return {
    visible: sessions.slice(0, visibleCount),
    hidden,
    overflow: hidden.length,
  }
}

/* ── 溢出格的合成悬停 key ───────────────────────────────────────────── */

/** 溢出格合成 key 的前缀。真实会话 key 是 `<provider>:<session-id>`，不会以 `@` 开头。 */
const OVERFLOW_KEY_PREFIX = '@overflow'

/** 溢出格悬停时上报的形态与格子数（折叠就是按它们算出来的）。 */
export interface OverflowHoverState {
  layout: FloatingLayout
  cells: number
}

/**
 * 造溢出格的合成悬停 key：`@overflow:<layout>:<cells>`。
 *
 * 为什么把形态与格子数编进 key：悬停事件只带「模块 id + key」，
 * 而提示窗是**另一个窗口**，既不知道悬浮窗当前形态、也不知道它分到几格
 * （`TooltipHoverPayload` 里只有外框与贴边状态）。可折叠到第几个会话正是由这两者决定的，
 * 所以让它们跟着 key 走 —— 提示窗据此用**同一个** `partitionIndicatorSessions` 重算，
 * 屏上的环与清单里的行必然一致。
 *
 * key 由模块自己解释，外壳只当字符串转发（见 `floating-module-context.ts`），
 * 因此这里编码模块语义不需要改任何对外契约。
 */
export function overflowHoverKey(layout: FloatingLayout, cells: number): string {
  return `${OVERFLOW_KEY_PREFIX}:${layout}:${clampCells(cells, 1, ABS_MAX_CELLS)}`
}

/**
 * 解析溢出格的合成 key；不是溢出格（或参数不合法）时返回 null，调用方走单会话逻辑。
 * 格子数取整数、夹进 `[1, ABS_MAX_CELLS]`，与 `overflowHoverKey` 对称。
 */
export function parseOverflowHoverKey(key: string): OverflowHoverState | null {
  const parts = key.split(':')
  if (parts.length !== 3 || parts[0] !== OVERFLOW_KEY_PREFIX) return null
  const layout = parts[1] as FloatingLayout
  if (layout !== 'square' && layout !== 'corner' && layout !== 'horizontal' && layout !== 'vertical') return null
  const cells = Number(parts[2])
  if (!Number.isInteger(cells) || cells < 1 || cells > ABS_MAX_CELLS) return null
  return { layout, cells }
}

/**
 * 由悬停 key 还原「被折叠的会话」清单。
 *
 * - 不是溢出格的 key → null（调用方按单会话处理）；
 * - 是溢出格但此刻已没有折叠（会话在悬停期间结束）→ 空数组（调用方据此给降级文案）。
 *
 * 顺序按注意力优先级（红 → 黄 → 绿，同色内保持原顺序）排过：环从 12 点起的第一个扇形就是这个颜色，
 * 清单第一行与它对应 —— 屏上的环和清单因此不只是同一批会话，连排列顺序都能对上。
 */
export function overflowListFor(key: string, sessions: readonly SessionSnapshot[]): SessionSnapshot[] | null {
  const state = parseOverflowHoverKey(key)
  if (!state) return null
  return sortByAttention(partitionIndicatorSessions(sessions, state.layout, state.cells).hidden)
}

/* ── 占比环 ─────────────────────────────────────────────────────────── */

/** 环上的一种灯色：折叠会话里有几个是这种颜色。 */
export interface OverflowSegment {
  /** 灯色（与指示灯同一套语义） */
  color: StatusColor
  /** 该灯色下的会话数 */
  count: number
}

/**
 * 分片的排列顺序 = 注意力优先级：红（出错）→ 黄（等你操作）→ 绿（执行中）。
 *
 * 不按会话开始时间排：一圈彩环缩到 45px 后，用户要能一眼看出「折叠里有没有红的」，
 * 而不是「谁先来的」。没有的灯色不出分片（不占角度、也不留零宽 stop）。
 */
const RING_COLOR_ORDER: readonly StatusColor[] = ['red', 'yellow', 'green']

/**
 * 注意力优先级（越小越靠前）。由下面「占比环」一节的 {@link RING_COLOR_ORDER} 推出 ——
 * 环的扇形次序与清单的行序是同一件事，共用一个来源就不会走散。
 */
const ATTENTION_ORDER: ReadonlyMap<StatusColor, number> = new Map(
  RING_COLOR_ORDER.map((color, index) => [color, index])
)

/**
 * 把会话按注意力优先级（红 → 黄 → 绿）排序，不改动入参。
 * `Array.prototype.sort` 在现代引擎里是稳定的：同色内保持原顺序（= 会话开始时间升序）。
 */
function sortByAttention(sessions: readonly SessionSnapshot[]): SessionSnapshot[] {
  return [...sessions].sort(
    (left, right) =>
      (ATTENTION_ORDER.get(colorForSession(left)) ?? 0) - (ATTENTION_ORDER.get(colorForSession(right)) ?? 0)
  )
}

/** 按注意力优先级汇总折叠会话的灯色，供溢出格的占比环与悬停清单的统计片共用。 */
export function overflowSegments(hidden: readonly SessionSnapshot[]): OverflowSegment[] {
  const counts = new Map<StatusColor, number>()
  for (const session of hidden) {
    const color = colorForSession(session)
    counts.set(color, (counts.get(color) ?? 0) + 1)
  }
  return RING_COLOR_ORDER.filter(color => (counts.get(color) ?? 0) > 0).map(color => ({
    color,
    count: counts.get(color) ?? 0,
  }))
}

/**
 * 把分片转成 `conic-gradient`：从 12 点起顺时针，扇形角度 = 该灯色的会话占比。
 *
 * stop 用**累计**比例（`0 → p1 → p1+p2 → … → 100%`）而不是各段宽度分别算：
 * 相邻两段的边界取同一个数，四舍五入到两位小数也不会在扇形之间留出缝。
 * 颜色一律走 `var(--ufo-status-*)` token，不在 TS 里再抄一份色值。
 * 无分片（不该出现，折叠量恒 ≥ 2）时返回空串 —— 调用方据此不渲染环。
 */
export function overflowRingGradient(segments: readonly OverflowSegment[]): string {
  const total = segments.reduce((sum, segment) => sum + segment.count, 0)
  if (total <= 0) return ''
  const stops: string[] = []
  let passed = 0
  let from = '0%'
  for (const segment of segments) {
    passed += segment.count
    const to = stopPercent(passed / total)
    stops.push(`var(--ufo-status-${segment.color}) ${from} ${to}`)
    from = to
  }
  return `conic-gradient(${stops.join(', ')})`
}

/** 比例 → CSS 百分比（保留两位小数，去掉多余的 0：`100.00` → `100`）。 */
function stopPercent(ratio: number): string {
  return `${(ratio * 100).toFixed(2).replace(/\.?0+$/, '')}%`
}
