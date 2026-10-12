// @vitest-environment happy-dom

/**
 * 信息卡插入块的高度预算与筛选判据。
 *
 * ⚠️ 三个数字与 `main.css` 的 `.tooltip-plugs*` 是同一件事的两种写法：
 * 这里的算式决定提示窗长多高，那边决定实际占多高。任一处改了而另一处没改，
 * 表现就是「追加的块被裁掉一截」或「卡下面多出一条空白」—— 所以本文件直接读 CSS 文本比对。
 */
import type { Component } from 'vue'
import { describe, expect, it } from 'vitest'
import css from '../assets/main.css?raw'
import {
  TOOLTIP_PLUG_GAP_PX,
  TOOLTIP_PLUG_HEAD_PX,
  TOOLTIP_PLUG_ROW_PX,
  extendTooltipSizeForPlugs,
  plugBlockHeight,
  resolveTooltipExtensions,
  type TooltipExtensionSlot,
} from './tooltip'

/** 归一行尾（CRLF 会藏进正则捕获组）。 */
const source = css.replace(/\r\n/g, '\n')

/** 造一块挂载位。 */
function slot(patch: Partial<TooltipExtensionSlot> = {}): TooltipExtensionSlot {
  return { hostId: 'agent-status', pluginId: 'balance-board', label: '余额看板', maxRows: 2, component: { template: '<div />' } as Component, ...patch }
}

const target = { moduleId: 'agent-status', key: 'session-a' }

describe('插入块的高度预算', () => {
  it('算式 = 头一行 + 声明的行数 + 间隔', () => {
    expect(plugBlockHeight(0)).toBe(TOOLTIP_PLUG_HEAD_PX + TOOLTIP_PLUG_GAP_PX)
    expect(plugBlockHeight(1)).toBe(TOOLTIP_PLUG_HEAD_PX + TOOLTIP_PLUG_ROW_PX + TOOLTIP_PLUG_GAP_PX)
    expect(plugBlockHeight(3)).toBe(TOOLTIP_PLUG_HEAD_PX + 3 * TOOLTIP_PLUG_ROW_PX + TOOLTIP_PLUG_GAP_PX)
    // 坏数字不许把窗口撑成 NaN（NaN 进 setSize 是整窗尺寸失控）。
    expect(plugBlockHeight(Number.NaN)).toBe(TOOLTIP_PLUG_HEAD_PX + TOOLTIP_PLUG_GAP_PX)
    expect(plugBlockHeight(-5)).toBe(TOOLTIP_PLUG_HEAD_PX + TOOLTIP_PLUG_GAP_PX)
  })

  it('多块累加，宽度不动', () => {
    const base = { width: 300, height: 260 }
    const grown = extendTooltipSizeForPlugs(base, [slot({ maxRows: 2 }), slot({ pluginId: 'ci', maxRows: 1 })])
    expect(grown.width).toBe(300)
    expect(grown.height).toBe(260 + plugBlockHeight(2) + plugBlockHeight(1))
    // 没有块就原样返回：不许给宿主卡平白多加一段。
    expect(extendTooltipSizeForPlugs(base, [])).toEqual(base)
  })

  it('CSS 里的来源标注行高与块间距用的是同一组数', () => {
    const head = source.match(/\.tooltip-plug__source \{[\s\S]*?min-height: (\d+)px/)
    expect(head?.[1], 'main.css 里找不到 .tooltip-plug__source 的 min-height').toBe(String(TOOLTIP_PLUG_HEAD_PX))
    const gap = source.match(/\.tooltip-plugs \{[\s\S]*?gap: (\d+)px/)
    expect(gap?.[1], 'main.css 里找不到 .tooltip-plugs 的 gap').toBe(String(TOOLTIP_PLUG_GAP_PX))
  })
})

describe('插入块的筛选', () => {
  it('只挂到它声明的那个宿主卡上', () => {
    const slots = [slot(), slot({ pluginId: 'ci', hostId: 'other-module' })]
    expect(resolveTooltipExtensions(slots, target, () => true).map(item => item.pluginId)).toEqual([
      'balance-board',
    ])
  })

  it('被停用的包那块不出现', () => {
    const slots = [slot(), slot({ pluginId: 'ci' })]
    const enabled = (id: string) => id !== 'ci'
    expect(resolveTooltipExtensions(slots, target, enabled).map(item => item.pluginId)).toEqual(['balance-board'])
  })

  it('块自己说没内容就整块不出现（不留一条虚线挂在卡尾）', () => {
    const slots = [slot({ hasContent: () => false }), slot({ pluginId: 'ci', hasContent: () => true })]
    expect(resolveTooltipExtensions(slots, target, () => true).map(item => item.pluginId)).toEqual(['ci'])
    // 没给 hasContent = 有悬停目标就出现（与独立信息卡同一默认口径）。
    expect(resolveTooltipExtensions([slot()], target, () => true)).toHaveLength(1)
  })

  it('没有悬停目标时一块都不出', () => {
    expect(resolveTooltipExtensions([slot()], null, () => true)).toEqual([])
    expect(resolveTooltipExtensions([slot()], undefined, () => true)).toEqual([])
  })
})
