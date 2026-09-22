/**
 * Agent 状态灯信息卡的**窗口高度口径**（纯函数）。
 *
 * 悬停信息卡是独立窗口，尺寸由模块在 `tooltip.size` 里声明，提示窗显示前按它 `setSize`。
 * 单会话卡片的行数固定，高度写死成一个预算值即可；但 `+N` 折叠清单是**一行一个会话**，
 * 行数随折叠了几个而变化 —— 写死高度就必然要截断（用户要求「有几个就全列出来」），
 * 所以这里按行数把高度算出来。
 *
 * ⚠️ 这套常数与样式表是一份契约：`agent-status.css` 里
 * `.tooltip-overflow-row` 的行高、`.tooltip-shell` 的内边距 / 行间距、`.tooltip-row--head`
 * 的底线厚度必须与这里对上，否则窗口会差出几十像素（行被切掉一截，或底部空一大块）。
 * 护栏见 `agent-status-css.spec.ts`。
 */

/** 信息卡宽度（物理像素）：三种内容共用，够放下「已使用 token: 1.26M」这类最长行。 */
export const AGENT_TOOLTIP_WIDTH = 300

/**
 * 单会话信息卡的高度预算（物理像素）：
 * head + Endpoint + token + 4 个拓展字段 + 刷新标注 + 提问 1 行 ≈ 236px，留一点余量（见 CLAUDE.md）。
 */
export const AGENT_TOOLTIP_SESSION_HEIGHT = 260

/** 折叠清单的高度口径；每项都对应样式表里的一条声明。 */
export const OVERFLOW_TOOLTIP_METRICS = {
  /** `.tooltip-shell` 的 `p-2.5`：上下各 10px */
  shellPaddingY: 10,
  /** `.tooltip-shell` 的 `gap-1.5`：相邻子项之间 6px */
  gap: 6,
  /** 头部行：一行 18px + `.tooltip-row--head` 的 `pb-1.5`(6px) + 1px 分隔线 */
  head: 25,
  /** 折叠会话行：`.tooltip-overflow-row` 的固定行高 */
  row: 18,
  /** 兜底小字的行高（`.tooltip-overflow-note`） */
  note: 15,
} as const

/**
 * 折叠清单信息卡的高度：`外壳内边距 + 头部 + 全部折叠行 + 行间距 [+ 兜底行]`。
 *
 * 行数不设上限 —— 折叠几个就多高，装不下屏幕时由外壳把窗口夹进工作区、
 * 多余的行走清单内部的滚动条（窗口不会长到屏幕外面去）。
 * 行数为 0（悬停期间会话都结束了）时只算头部 + 兜底行，卡片不会剩一个空框。
 */
export function overflowTooltipHeight(hiddenCount: number): number {
  const { shellPaddingY, gap, head, row, note } = OVERFLOW_TOOLTIP_METRICS
  const count = Number.isFinite(hiddenCount) ? Math.max(0, Math.floor(hiddenCount)) : 0
  if (count <= 0) {
    // 只剩「头部 + 清单容器（空，高 0）+ 兜底行」：两条子项间距 + 兜底行本身。
    return shellPaddingY * 2 + head + gap * 2 + note
  }
  // 子项是「头部 + 清单容器」：容器与头之间一条 gap，容器内部每两行之间还有一条。
  return shellPaddingY * 2 + head + gap + count * row + (count - 1) * gap
}
