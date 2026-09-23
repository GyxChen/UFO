/**
 * 滴答待办的「三形态排布 + 数字/色条」护栏。
 *
 * 悬浮窗视图 = 三组「数字 + 色条」（方案 A · 数字主导），排布规则按形态切换，几处都容易改回去：
 *
 * 1. 方形 / 四角 / 横条：三组**横排**，组内竖着放（数字在上、色条在下）；
 * 2. 竖条（左右贴边）：三组**竖排**、组内横着放（色条在左、数字在右）—— 竖条只有一格宽、
 *    高反而富余，横排三组会挤在一条线上；
 * 3. 竖条**不把三组摊满整条**：整块随内容收拢后上下居中，行间隙固定 —— 否则三组会被拉开
 *    十几二十像素，一个个孤零零挂着；
 * 4. 三组之间的列间隙只按宽度取（`6cqw`）—— 早先写的 `min(10cqh, 4cqw)` 在横条上被 cqh
 *    压到 2px 上下，三组几乎贴在一起；
 * 5. 字号取**两条轴里更紧的那条**（`min(30cqh, 47cqw)`）：只看一条轴，另一边必然溢出，
 *    而槽位有 `overflow: hidden`，观感就是数字被切掉一截；
 * 6. 位数降档（`--todo-count-step`）只乘在那一个 `font-size` 上、且**只在窄槽位**（≤60px）生效 ——
 *    各写一条 `font-size` 会被竖条那条形态规则盖掉；在 90px 的横条里两位数本来就放得下，
 *    跟着一起缩纯属白缩。
 *
 * ⚠️ 这里断言的是**声明口径**，不是排版结果（测试环境没有排版引擎）。
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
  it('lays the three groups out in a row for square / corner / horizontal', () => {
    const groups = ruleBody(todoCss(), '.todo-module__groups')
    expect(groups, '.todo-module__groups 必须存在').not.toBe('')
    // 横排是默认方向（竖条那条形态规则才改成 column），组内是「数字在上、色条在下」。
    expect(decl(groups, 'display')).toBe('flex')
    expect(decl(groups, 'flex-direction')).toBe('')
    expect(decl(groups, 'align-items')).toBe('center')

    const cell = ruleBody(todoCss(), '.todo-module__cell')
    expect(cell, '.todo-module__cell 必须存在').not.toBe('')
    expect(decl(cell, 'flex-direction')).toBe('column')
  })

  it('stacks the three groups vertically (bar on the left, aligned) in the vertical bar', () => {
    const css = todoCss()
    const vertical = ruleBody(css, '.todo-module--vertical .todo-module__groups')
    expect(vertical, '竖条必须有单独的排布规则').not.toBe('')
    expect(decl(vertical, 'flex-direction')).toBe('column')
    // 三组随内容收拢后由父级上下居中；不能再给整块一个会撑开三组的固定 / 封顶高度。
    expect(decl(vertical, 'align-self')).toBe('center')
    expect(decl(vertical, 'height')).toBe('')
    // 竖条的限制轴是行宽（只有一格宽），字号必须带上 cqw，不能只看 cqh（行高很富余，会溢出）。
    expect(decl(vertical, 'font-size')).toContain('cqw')
    // 三行等宽：行宽不同时（`▬ 2` 对 `▬ 12`）按 center 各自居中会让色条左缘参差。
    expect(decl(vertical, 'align-items')).toBe('stretch')

    // 组内转置：色条跑到数字左边（row-reverse，DOM 顺序不动），且在行内贴左 —— 色条排成一条竖线。
    const cell = ruleBody(css, '.todo-module--vertical .todo-module__cell')
    expect(cell, '竖条里组内必须转置').not.toBe('')
    expect(decl(cell, 'flex-direction')).toBe('row-reverse')
    expect(decl(cell, 'justify-content')).toBe('flex-end')
  })

  it('keeps the three groups apart on the tight axis', () => {
    const groups = ruleBody(todoCss(), '.todo-module__groups')
    // 列间隙是三组疏密的唯一调节项，只能跟宽度走：带上 cqh 就会在横条（只有一格高）上
    // 被压到 2px，三组贴成一坨。
    expect(decl(groups, 'gap')).toContain('6cqw')
    expect(decl(groups, 'gap')).not.toContain('cqh')
  })

  it('sizes the type from whichever axis is tighter', () => {
    const css = todoCss()
    const groups = ruleBody(css, '.todo-module__groups')
    const size = decl(groups, 'font-size')

    // 两条轴都要在：只按一条取，另一条必然溢出（槽位是 overflow: hidden，切掉就看不出来）。
    expect(size).toContain('min(')
    expect(size).toContain('cqh')
    expect(size).toContain('cqw')
    // 上限是设计像素，必须跟格子缩放走。
    expect(size).toContain('--floating-cell-scale')
  })

  it('keeps the vertical type on its own budget, scaled to 0.8', () => {
    const css = todoCss()
    const row = decl(ruleBody(css, '.todo-module__groups'), 'font-size')
    const vertical = decl(ruleBody(css, '.todo-module--vertical .todo-module__groups'), 'font-size')

    // 横排的预算：一格里三组并排、宽度三等分 → 高度项 55cqh / 宽度项 42cqw / 设计上限 15px。
    expect(row).toContain('55cqh')
    expect(row).toContain('42cqw')
    expect(row).toContain('15px')

    /*
     * 竖条**不复用横排那套**：它一行只有一组，宽度不用三等分，预算本来就宽（23cqh / 80cqw / 20px）；
     * 再按用户定的 0.8× 收一档 → 18.4cqh / 64cqw / 16px。45 格下约 15.5px（与方形同档）。
     * ⚠️ 不是「横排预算 × 0.8」（那是 13px，比方形还小 —— 竖条明明更宽松，没有理由）；
     *    9px 的可读性下限也不跟着缩。
     */
    expect(vertical).toContain('18.4cqh')
    expect(vertical).toContain('64cqw')
    expect(vertical).toContain('16px')
    expect(vertical).toContain('9px')
    expect(vertical).toContain('--floating-cell-scale')
  })

  it('steps the type down by digit count, only inside the narrow slot', () => {
    const css = todoCss()

    // 档位是个倍率变量，乘在 font-size 上 —— 各写一条 font-size 会被竖条那条形态规则盖掉。
    expect(ruleBody(css, '.todo-module__groups')).toContain('--todo-count-step')
    for (const layout of ['', '.todo-module--vertical ']) {
      const size = decl(ruleBody(css, `${layout}.todo-module__groups`), 'font-size')
      expect(size, `${layout || '横排'}的字号必须乘上档位`).toContain('var(--todo-count-step)')
    }

    // 横排只在小槽位降档：90px 的横条里两位数按原字号放得下，不该跟着缩。
    const narrow = blockAfter(css, '@container floating-slot (max-width: 60px)')
    expect(narrow, '窄槽降档块必须存在').not.toBe('')
    expect(decl(ruleBody(narrow, '.todo-module__groups--wide'), '--todo-count-step')).toBe('0.5')
    expect(decl(ruleBody(narrow, '.todo-module__groups--xwide'), '--todo-count-step')).toBe('0.34')
    // ⚠️ 竖条必须被排除在外：它一行只有一组，窄的是槽位宽度、不是这一行的内容。
    expect(narrow, '竖条不吃横排的窄槽档').toContain(':not(.todo-module--vertical)')

    // 竖条自己的档：两位数放得下就不降，三位数才收一档（且只是兜住比例下的一线差）。
    expect(
      ruleBody(css, '.todo-module--vertical .todo-module__groups--wide'),
      '竖条的两位数档不该存在（放得下就别缩）'
    ).toBe('')
    expect(decl(ruleBody(css, '.todo-module--vertical .todo-module__groups--xwide'), '--todo-count-step')).toBe('0.9')
  })
})

/**
 * 数字与色条的护栏。
 *
 * 这一版**刻意不再有旗子字形**（没有 `flag.svg`、没有 mask、没有 `--todo-flag-mask`）：
 * 45px 方格里 9px 的旗形既认不出形状，又要和数字抢宽度。色条只是「哪一档」的锚点，
 * 几条一改就坏的地方逐条钉住：
 * 1. 色条的颜色必须来自 `currentColor` —— 一份颜色源同时管住数字与色条；三档各自另写
 *    `background-color` 就有了两个真相，迟早对不上；
 * 2. 色条的宽高与组内间隙都用 `em`（跟着字号走）：写死 px 会在格子调大后变成一根细线；
 * 3. 竖条里色条要**转 90°**（宽高互换），否则竖排里会出现一条横着的粗杠；
 * 4. 数据未到的中性灰只能落在 `.todo-module--empty` 上，且是「变灰」不是「变没」。
 */
describe('模块样式 · 滴答待办数字与色条', () => {
  it('paints the bars from currentColor so the three tones have one source of truth', () => {
    const css = todoCss()
    const tone = ruleBodyOwn(css, '.todo-module__tone')

    expect(tone, '.todo-module__tone 必须存在').not.toBe('')
    expect(decl(tone, 'background-color')).toBe('currentColor')
    // 旗子那套（字形 + mask）已经整块退场：留着就是两套并行的画法。
    expect(css).not.toContain('.todo-module__flag')
    expect(css).not.toContain('mask-image')
    expect(css).not.toContain('--todo-flag-mask')
  })

  it('gives each priority bucket its own color and nothing else', () => {
    const css = todoCss()

    // 取 `-text` 档而不是填充档：这里同时决定**数字的颜色**与色条（`currentColor`），
    // 要的是「浅色主题下也读得清」的那个取值（填充档的鲜艳黄 / 蓝在白底上读不出来）。
    expect(decl(ruleBody(css, '.todo-module__cell--red'), 'color')).toBe('var(--ufo-status-red-text)')
    expect(decl(ruleBody(css, '.todo-module__cell--yellow'), 'color')).toBe('var(--ufo-status-yellow-text)')
    expect(decl(ruleBody(css, '.todo-module__cell--blue'), 'color')).toBe('var(--ufo-status-blue-text)')

    // 三档只许改颜色：改尺寸会让三根色条长短不一，改字号会让三组数字大小参差。
    for (const tone of ['red', 'yellow', 'blue']) {
      const body = ruleBody(css, `.todo-module__cell--${tone}`)
      expect(body, `${tone} 档只许改颜色`).not.toContain('width')
      expect(body, `${tone} 档只许改颜色`).not.toContain('height')
      expect(body, `${tone} 档只许改颜色`).not.toContain('font-size')
    }
  })

  it('sizes the bars and inner gaps in em so they follow the font, not the design pixels', () => {
    const css = todoCss()
    const tone = ruleBodyOwn(css, '.todo-module__tone')
    const cell = ruleBodyOwn(css, '.todo-module__cell')

    // 色条宽度跟着数字走（100% = 数字的宽度），高度按 em 推 —— 都是相对量。
    expect(decl(tone, 'width')).toBe('100%')
    expect(decl(tone, 'height')).toMatch(/em$/)
    expect(decl(cell, 'gap')).toMatch(/em$/)
  })

  it('turns the bar 90° in the vertical bar instead of leaving a crosswise slab', () => {
    const tone = ruleBody(todoCss(), '.todo-module--vertical .todo-module__tone')

    expect(tone, '竖条里色条必须有转置规则').not.toBe('')
    // 细长条：宽 < 高，与数字等高
    const width = Number.parseFloat(decl(tone, 'width'))
    const height = Number.parseFloat(decl(tone, 'height'))
    expect(width).toBeLessThan(height)
    expect(decl(tone, 'width')).toMatch(/em$/)
    expect(decl(tone, 'height')).toMatch(/em$/)
  })

  it('keeps the grey placeholder for the not-yet-loaded state', () => {
    const css = todoCss()
    const emptyCell = ruleBody(css, '.todo-module--empty .todo-module__cell')
    const emptyTone = ruleBody(css, '.todo-module--empty .todo-module__tone')

    // 中性灰与三档语义色同为单类 / 双类规则：必须比 `.todo-module__cell--red` 更具体才盖得住。
    expect(emptyCell, '数据未到必须有单独的中性灰规则').not.toBe('')
    expect(decl(emptyCell, 'color')).toBe('var(--ufo-muted)')
    expect(decl(emptyTone, 'opacity')).toBe('0.55')
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
    // 设置页那份标题：一行 + 省略号（列表要密，一屏能扫更多条）。
    // ⚠️ `min-width: 0` 是关键：flex 子项默认可缩到内容以下 —— 少了它，里面的 nowrap 文本
    // 会把行顶宽、`text-overflow` 永远等不到溢出（表现为「文字直接冲出卡片」）。
    // ⚠️ 卡片那一层的同类坑在 main.css：`.ufo-card` 是 grid 容器，子项也必须 `min-width: 0`。
    const title = ruleBodyOwn(todoCss(), '.todo-status__title')

    expect(title, '.todo-status__title 必须存在').not.toBe('')
    expect(decl(title, 'min-width')).toBe('0')
    expect(decl(title, 'overflow')).toBe('hidden')
    expect(decl(title, 'text-overflow')).toBe('ellipsis')
    expect(decl(title, 'white-space')).toBe('nowrap')
  })

  it('lets the tooltip title wrap up to three lines before ellipsising', () => {
    // 信息卡是「停下来读一条」的地方，标题被截一半就白搭 —— 只有这里放开换行。
    const tooltip = ruleBodyOwn(todoCss(), '.todo-tooltip__scroll .todo-status__title')

    expect(tooltip, '信息卡的标题必须有单独的换行规则').not.toBe('')
    // 三行封顶 + 末行省略号（Chromium 里 line-clamp 自己补省略号，不需要 text-overflow）。
    expect(decl(tooltip, 'display')).toBe('-webkit-box')
    expect(decl(tooltip, '-webkit-box-orient')).toBe('vertical')
    expect(decl(tooltip, '-webkit-line-clamp')).toBe('3')
    expect(decl(tooltip, 'line-clamp')).toBe('3')
    // ⚠️ 基类那条 `white-space: nowrap` 不解除就永远不会折行 —— 这条最容易被漏掉。
    expect(decl(tooltip, 'white-space')).toBe('normal')
    // 长 URL / 长 id 这种没有空格的串也要能断，否则它宁可溢出。
    expect(decl(tooltip, 'overflow-wrap')).toBe('anywhere')
    // 多行标题要有自己的行距（基类没写行高）。
    expect(decl(tooltip, 'line-height')).toBe('1.35')
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
