/**
 * 滴答待办的「三形态排布」护栏。
 *
 * 三面小旗的排布规则是按形态切换的，且**几处都容易改回去**：
 *
 * 1. 方形 / 四角 / 横条：横排三面（每组「旗在上、数量在下」）；
 * 2. 竖条（左右贴边）：竖排三面（一行一档「旗在左、数量在右」）—— 竖条只有一格宽、高反而富余；
 * 3. 竖条**不把三行摊满整条**：行轨取 `auto`（各取内容高）、整块随内容收拢后上下居中，仅留
 *    小呼吸间隙 —— 否则（1fr 等分或塞满封顶高度）三面 ~10px 的小旗会被拉开十几二十像素，
 *    观感是一个个孤零零挂着（实测 1fr 封顶 96px 时行距 32px）；
 * 4. 三面旗之间的列间隙只按宽度取（`8cqw`）—— 早先写的 `min(10cqh, 4cqw)` 在横条上被 cqh
 *    压到 2px 上下，三组几乎贴在一起；
 * 5. 数量被降级掉（窄槽位）时网格里只剩三面旗，**轨道必须跟着收**：
 *    横排那套「按列填充 + 三列」会让三面旗摞成一列两行（旗 1、旗 2 上下叠、旗 3 跑到第二列），
 *    竖排那套「按行填充 + 两列」会让旗 1、旗 2 并排 —— 观感都不再是「三面旗排一排」。
 *
 * ⚠️ 这里断言的是**声明口径**，不是排版结果（测试环境没有排版引擎）。
 * 真实像素用无头 Edge + Plus Jakarta Sans 量过：45 格方形 39×39（三点一行，宽 29.4px）、
 * 竖条 39×174（三行等距地占中间 96px，最宽一行 31px）、32 格竖条 26×122（只剩三点、居中）。
 * 三档都到 `99+` 的最坏情形（四角 84×84）也没溢出；换成小旗后盒子放大到 0.7em
 * （横排窄槽 23cqw/23cqh、竖排窄槽 44cqw/16cqh），45 格下三面旗 + 两个间隙 ≈ 36px < 39.5px。
 */
import { describe, expect, it } from 'vitest'

/** 模块样式文件：路径 → 原文。 */
const moduleStyles = import.meta.glob('/src/modules/todo/*.css', {
  eager: true,
  query: '?raw',
  import: 'default',
}) as Record<string, string>

/** 取样式原文（本模块只有一个样式文件）。 */
function todoCss(): string {
  const entry = Object.entries(moduleStyles).find(([path]) => path.endsWith('todo.css'))
  expect(entry, '没找到 todo.css').toBeTruthy()
  return entry?.[1] ?? ''
}

/** 按花括号配对取出一段块（用于 @container 这类嵌套块）。 */
function blockAfter(css: string, header: string): string {
  const start = css.indexOf(header)
  if (start < 0) return ''
  const open = css.indexOf('{', start)
  if (open < 0) return ''
  let depth = 0
  for (let index = open; index < css.length; index += 1) {
    if (css[index] === '{') depth += 1
    else if (css[index] === '}') {
      depth -= 1
      if (depth === 0) return css.slice(open + 1, index)
    }
  }
  return ''
}

/** 取某个选择器的规则体；找不到返回空串。 */
function ruleBody(css: string, selector: string): string {
  return blockAfter(css, `${selector} {`)
}

/**
 * 取「选择器独占一行」的那条规则体。
 *
 * `ruleBody` 用的是 `indexOf`，会被更长的选择器截胡：形如 `.a .b {` 的复合 / 后代选择器里
 * 就含有 `.b`，查基类规则时可能先命中那条变体规则。这里要求选择器前面是行首。
 *
 * @param css 样式原文
 * @param selector 选择器（会做正则转义）
 * @returns 规则体；找不到返回空串
 */
function ruleBodyOwn(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const match = new RegExp(`(?:^|\\n)[ \\t]*${escaped}[ \\t]*\\{`).exec(css)
  return match ? blockAfter(css.slice(match.index), `${selector} {`) : ''
}

/** 从规则体里读一个声明值（如 `grid-auto-flow: column` → `column`）。 */
function decl(body: string, property: string): string {
  const match = new RegExp(`(?:^|[;{\\s])${property}\\s*:\\s*([^;]+)`).exec(body)
  return match ? match[1].trim() : ''
}

describe('模块样式 · 滴答待办三形态排布', () => {
  it('lays the three flags out in a row for square / corner / horizontal', () => {
    const groups = ruleBody(todoCss(), '.todo-module__groups')
    expect(groups, '.todo-module__groups 必须存在').not.toBe('')
    expect(decl(groups, 'grid-auto-flow')).toBe('column')
    expect(decl(groups, 'grid-template-columns')).toBe('auto auto auto')
  })

  it('stacks the three flags vertically in the vertical bar', () => {
    const vertical = ruleBody(todoCss(), '.todo-module--vertical .todo-module__groups')
    expect(vertical, '竖条必须有单独的排布规则').not.toBe('')
    // 一行一档：两列（旗 | 数量）、三行、按行填充。
    expect(decl(vertical, 'grid-template-columns')).toBe('auto auto')
    // 行轨必须是 auto（各取内容高），不能是 1fr —— 1fr 等分或塞满封顶高度会把三面旗拉开。
    expect(decl(vertical, 'grid-template-rows')).toBe('repeat(3, auto)')
    expect(decl(vertical, 'grid-auto-flow')).toBe('row')
    // 三行随内容收拢后由父级上下居中；不能再给整块一个会撑开三面旗的固定 / 封顶高度。
    expect(decl(vertical, 'align-self')).toBe('center')
    expect(decl(vertical, 'height')).toBe('')
    // 竖条的限制轴是行宽（只有一格宽），字号必须按 cqw 取，不能按 cqh（行高很富余，会溢出）。
    expect(decl(vertical, 'font-size')).toContain('cqw')
  })

  it('keeps the three flags apart on the tight axis', () => {
    const groups = ruleBody(todoCss(), '.todo-module__groups')
    // gap = 「行间隙 列间隙」两段。列间隙是三面旗疏密的唯一调节项，只能跟宽度走：
    // 带上 cqh 就会在横条（只有一格高）上被压到 2px，三组贴成一坨。
    const [, columnGap = ''] = decl(groups, 'gap').split(/\s+(?=clamp\()/)
    expect(columnGap, '列间隙必须按 cqw 取').toContain('8cqw')
    expect(columnGap, '列间隙不得再受 cqh 限制').not.toContain('cqh')
  })

  it('collapses the grid tracks when the counts are hidden', () => {
    const css = todoCss()

    // 横排（方形 / 四角 / 横条）的窄槽位：藏数字 + 把「按列填充」改回按行填充，三面旗才在一行。
    const rowNarrow = blockAfter(css, '@container floating-slot (max-width: 60px)')
    expect(rowNarrow, '横排窄槽降级块必须存在').not.toBe('')
    expect(rowNarrow).toContain(':not(.todo-module--vertical) .todo-module__count')
    const rowGroups = ruleBody(rowNarrow, '.todo-module:not(.todo-module--vertical) .todo-module__groups')
    expect(decl(rowGroups, 'grid-auto-flow')).toBe('row')
    // 数字一走，为「数字占满列宽」留的 8cqw 预算就没意义了，间隙改按字号取，三面旗才不挤。
    // 0.3em（不是更宽的 0.45em）：三面旗并排时宽度是限制轴，间隙留多了旗子就得让小。
    expect(decl(rowGroups, 'column-gap')).toBe('0.3em')

    // 竖条的窄槽位：藏数字 + 收成一列，三面旗才竖着叠（两列会变成旗 1、旗 2 并排）。
    const colNarrow = blockAfter(css, '@container floating-slot (max-width: 28px)')
    expect(colNarrow, '竖排窄槽降级块必须存在').not.toBe('')
    expect(decl(ruleBody(colNarrow, '.todo-module--vertical .todo-module__groups'), 'grid-template-columns')).toBe('auto')
    expect(colNarrow).toContain('display: none')
    // 形态规则（`.todo-module--vertical .todo-module__flag`）与降级规则等长，不带同一条选择器就盖不住它。
    expect(colNarrow).toContain('.todo-module--vertical .todo-module__flag')
  })
})

/**
 * 小旗子的护栏：**字形来自 `src/assets/flag.svg`，颜色来自 CSS，比例只由「高」推出来**。
 *
 * 这套「一份字形 + 三条 mask」的写法有几个一改就坏的地方，逐条钉住：
 * 1. 少了 `mask-image`（或少了那个 CSS 变量）旗子就退化成**一块实心方块** —— 界面上只是
 *    「三个方块」，不看测试很难发现是图标没生效；
 * 2. 少了 `mask-size: contain`，图标会按资源原始尺寸铺开、不跟盒子缩放；
 * 3. 少了 `mask-repeat: no-repeat`，一个盒子里会平铺出一排小旗；
 * 4. **宽高必须一起给、且比例锁死 6:7**（= `flag.svg` 的 viewBox 12×14）：只给一个尺寸，
 *    或两条轴各按各的槽位百分比算，某条轴先被压住时图标就被拉扁 —— 旗面忽宽忽窄，
 *    格子调大后尤其明显。所以基类只声明一个「高」变量、宽由它算出来，降级块也只改那一个。
 *
 * ⚠️ 同时锁住三档**只改颜色、不改字形**：形状必须由同一份 mask 提供，否则「同一套图标换色」
 * 就退化成了三份不同的图。
 */
describe('模块样式 · 滴答待办小旗子', () => {
  it('paints the flag glyph through a mask so one asset yields three colors', () => {
    const flag = ruleBodyOwn(todoCss(), '.todo-module__flag')

    expect(flag, '.todo-module__flag 必须存在').not.toBe('')
    expect(decl(flag, 'mask-image')).toBe('var(--todo-flag-mask)')
    expect(decl(flag, '-webkit-mask-image')).toBe('var(--todo-flag-mask)')
    expect(decl(flag, 'mask-size')).toBe('contain')
    expect(decl(flag, 'mask-repeat')).toBe('no-repeat')
    expect(decl(flag, 'mask-position')).toBe('center')
    // 底色只是「数据未到」的中性灰兜底；三档颜色由下面的修饰类给。
    expect(decl(flag, 'background-color')).toBe('var(--ufo-muted)')
  })

  it('derives the width from a single height variable so the glyph cannot be squashed', () => {
    const flag = ruleBodyOwn(todoCss(), '.todo-module__flag')

    // 宽高都给（缺一个就是「只按一条轴缩放」），且宽是从那唯一的「高」按 6:7 算出来的
    expect(decl(flag, 'height')).toBe('var(--todo-flag-h)')
    expect(decl(flag, 'width')).toBe('calc(var(--todo-flag-h) * 6 / 7)')

    // ⚠️ 降级块只许改 `--todo-flag-h`：各写各的 width / height 就会让两条轴各按各的轴被压，
    // 比例当场散掉（这正是这轮修掉的问题）。
    for (const query of [
      '@container floating-slot (max-width: 60px)',
      '@container floating-slot (max-width: 28px)',
    ]) {
      const block = blockAfter(todoCss(), query)
      const flagRule = block.slice(block.indexOf('.todo-module__flag'))
      expect(flagRule, `${query} 里必须给出旗子尺寸`).toContain('--todo-flag-h')
      expect(flagRule, `${query} 里不得另写 width`).not.toContain('width:')
      expect(flagRule, `${query} 里不得另写 height`).not.toContain('height:')
    }
  })

  it('gives each priority bucket its own flag color and nothing else', () => {
    const css = todoCss()

    expect(decl(ruleBody(css, '.todo-module__flag--red'), 'background-color')).toBe('var(--ufo-status-red)')
    expect(decl(ruleBody(css, '.todo-module__flag--yellow'), 'background-color')).toBe('var(--ufo-status-yellow)')
    expect(decl(ruleBody(css, '.todo-module__flag--blue'), 'background-color')).toBe('var(--ufo-status-blue)')

    // 三档只许改颜色：改尺寸会让三面旗大小不一，改 mask 会让它们不再是同一个字形。
    for (const tone of ['red', 'yellow', 'blue']) {
      const body = ruleBody(css, `.todo-module__flag--${tone}`)
      expect(body, `${tone} 档只许改颜色`).not.toContain('mask')
      expect(body, `${tone} 档只许改颜色`).not.toContain('width')
      expect(body, `${tone} 档只许改颜色`).not.toContain('height')
    }
  })

  it('keeps the grey placeholder for the not-yet-loaded state', () => {
    const empty = ruleBody(todoCss(), '.todo-module--empty .todo-module__flag')

    expect(empty, '数据未到必须有单独的中性灰规则').not.toBe('')
    expect(decl(empty, 'background-color')).toBe('var(--ufo-muted)')
  })
})

/**
 * 这一块的五条约定都是**看着像小样式、改起来却会毁掉交互**的，逐条钉住：
 * 1. 优先级画在勾选框的**边框**上（统一加粗，只有颜色分三档），不另立优先级圆点；
 * 2. **只有勾选框能切换完成态**：标题是纯文字，且没有任何「可点」的暗示
 *    （早先整行是一个按钮、点标题即完成，会误勾）；
 * 3. 勾选框的命中区不小于 24px（WCAG 2.2 的指针目标下限），但视觉上仍是 16px 的方框 ——
 *    靠负外边距把多出来的命中区还给排版；
 * 4. 父目标（有未完成子目标）的勾选框转虚线，且「已完成」必须用**两个类**的选择器
 *    才盖得住单类的三档优先级规则；
 * 5. 折叠箭头靠 `aria-expanded='false'` 转 -90°，收起状态因此与无障碍属性同源。
 */
describe('模块样式 · 滴答待办完成情况的勾选框与折叠', () => {
  /** 取一条声明里的像素数值（`3px` → 3）。 */
  function px(body: string, property: string): number {
    return Number.parseFloat(decl(body, property))
  }

  it('paints priority on the checkbox border with one shared bold weight', () => {
    const css = todoCss()

    // 优先级圆点与旧的子目标勾选框都已退场
    expect(css).not.toContain('.todo-status__dot')
    expect(css).not.toContain('.todo-status__sub-check')
    // 「完成 / 取消完成」按钮也一并去掉（勾选框本身就是开关），
    // 整行的「点哪都算切换」按钮（`.todo-status__toggle`）同样退场 —— 只有勾选框能切换。
    expect(css).not.toContain('.todo-status__action-btn')
    expect(css).not.toContain('.todo-status__toggle')

    // 三档只有颜色不同
    expect(decl(ruleBody(css, '.todo-status__check--red'), 'border-color')).toBe('var(--ufo-status-red)')
    expect(decl(ruleBody(css, '.todo-status__check--yellow'), 'border-color')).toBe('var(--ufo-status-yellow)')
    expect(decl(ruleBody(css, '.todo-status__check--blue'), 'border-color')).toBe('var(--ufo-status-blue)')

    // ⚠️ 三档**都不许再动粗细**：早先按优先级写 3 / 2 / 1px，蓝色那条细得像发丝，
    // 三个框看起来像三种控件。粗细是统一的「加粗」，分级只走颜色。
    for (const tone of ['red', 'yellow', 'blue']) {
      expect(ruleBody(css, `.todo-status__check--${tone}`), `${tone} 档只许改颜色`).not.toContain('border-width')
    }
    // 加粗写在基类上，三档共用
    expect(px(ruleBody(css, '.todo-status__check'), 'border')).toBe(2)
  })

  it('makes the checkbox the only toggle, with a hit area of at least 24px', () => {
    const css = todoCss()
    const button = ruleBodyOwn(css, '.todo-status__check-btn')

    expect(button, '.todo-status__check-btn 必须存在').not.toBe('')
    // 24px 是 WCAG 2.2「目标尺寸（最小）」的指针目标下限：16px 的方框按原样放上去太难点
    expect(px(button, 'width')).toBeGreaterThanOrEqual(24)
    expect(px(button, 'height')).toBeGreaterThanOrEqual(24)
    // 多出来的命中区靠负外边距还回去：行的排版与放大之前一致
    expect(px(button, 'margin')).toBeLessThan(0)

    // 标题不得有任何「可点」的暗示（早先 hover 加下划线，就是在说「点我能完成」）
    const title = ruleBodyOwn(css, '.todo-status__title')
    expect(decl(title, 'cursor')).toBe('')
    expect(decl(title, 'text-decoration')).toBe('')
    expect(css).not.toContain('.todo-status__toggle')
  })

  it('keeps the done and derived states able to override the tone rules', () => {
    const css = todoCss()

    // 已完成 / 由子目标驱动：都必须是「两个类」的复合选择器，才盖得住上面的单类三档规则
    const done = ruleBody(css, '.todo-status__check.is-done')
    expect(done, '已完成态必须写成 .todo-status__check.is-done').not.toBe('')
    expect(decl(done, 'background')).toBe('var(--ufo-accent)')

    const derived = ruleBody(css, '.todo-status__check.is-derived')
    expect(derived, '由子目标驱动的态必须写成 .todo-status__check.is-derived').not.toBe('')
    expect(decl(derived, 'border-style')).toBe('dashed')
  })

  it('ties the collapse arrow rotation to aria-expanded', () => {
    const css = todoCss()
    const collapsed = ruleBody(css, ".todo-status__twist-btn[aria-expanded='false'] svg")

    expect(collapsed, '收起态的旋转规则必须挂在 aria-expanded 上').not.toBe('')
    expect(decl(collapsed, 'transform')).toContain('rotate(-90deg)')
  })

  it('indents rows by the depth variable so parents and children line up', () => {
    const row = ruleBody(todoCss(), '.todo-status__row')

    expect(row, '.todo-status__row 必须存在').not.toBe('')
    // 缩进必须来自 flattenTree 给的 --todo-depth（写死 padding 就没有层级了）
    expect(decl(row, 'padding')).toContain('--todo-depth')
  })

  it('truncates a long title with an ellipsis instead of spilling out of the row', () => {
    // 标题必须凑齐这套配方，少一条都会变成「文字直接冲出卡片」（没有任何省略号）：
    // min-width: 0 让 flex 子项能缩到内容以下，其余三条负责把溢出的部分换成 …。
    // ⚠️ 卡片那一层的坑在 main.css：`.ufo-card` 是 grid 容器，子项必须 `min-width: 0`
    // 才能被卡片宽度约束（否则卡片内部轨道会被 nowrap 文本顶宽，标题永远"放得下"）。
    const title = ruleBodyOwn(todoCss(), '.todo-status__title')

    expect(title, '.todo-status__title 必须存在').not.toBe('')
    expect(decl(title, 'min-width')).toBe('0')
    expect(decl(title, 'overflow')).toBe('hidden')
    expect(decl(title, 'text-overflow')).toBe('ellipsis')
    expect(decl(title, 'white-space')).toBe('nowrap')
  })
})

/**
 * 主窗设置页的版面护栏。
 *
 * 这一页的高度由「高度链」给定（页面不滚、两列各自滚），于是两个坑都是「看着能改、
 * 一改就漏」的：
 * 1. 左右列宽：左列（定宽表单）固定、右列（任务树）吃剩余空间；窄窗口的塌陷**必须写在
 *    模块样式里**——共享 `.general-cards` 那条媒体查询是单类选择器，压不过这里的两个类；
 * 2. 左卡内容比卡片高时要在**卡内**滚：滚动一旦放到卡片自己身上，卡片里的加载遮罩
 *    （`position: absolute; inset: 0`）就会跟着内容滚走；而滚动区要占满卡片高度，
 *    卡片这个 grid 容器的行就必须写 `minmax(0, 1fr)`（默认 `auto` 行会跟着内容长高）。
 */
describe('模块样式 · 滴答待办设置页版面', () => {
  it('pins the left column and lets the right column take the rest', () => {
    const layout = ruleBody(todoCss(), '.general-cards.todo-general')

    expect(decl(layout, 'grid-template-columns')).toBe('340px minmax(0, 1fr)')
  })

  it('re-collapses to one column on narrow windows', () => {
    const narrow = blockAfter(todoCss(), '@media (max-width: 760px)')
    const layout = ruleBody(narrow, '.general-cards.todo-general')

    expect(layout, '窄窗口必须有单独的塌陷规则').not.toBe('')
    expect(decl(layout, 'grid-template-columns')).toBe('minmax(0, 1fr)')
  })

  it('scrolls the left card inside itself instead of spilling out of it', () => {
    const css = todoCss()
    const card = ruleBody(css, '.ufo-card.todo-auth-card')

    expect(card, '左卡必须带 .todo-auth-card').not.toBe('')
    expect(decl(card, 'grid-template-rows')).toBe('minmax(0, 1fr)')

    // ⚠️ 滚动在内层：卡片是加载遮罩的定位祖先，卡片自己成了滚动容器的话遮罩会跟着滚走。
    const body = ruleBody(css, '.todo-auth-card__body')
    expect(body, '左卡必须有内层滚动区').not.toBe('')
    expect(decl(body, 'overflow-y')).toBe('auto')
    expect(decl(body, 'min-height')).toBe('0')
    expect(decl(body, 'scrollbar-gutter')).toBe('stable')
  })
})
