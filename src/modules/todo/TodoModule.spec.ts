// @vitest-environment happy-dom

/**
 * 悬浮窗视图的回归护栏：**三面小旗 = 一份字形 + 三条 CSS**。
 *
 * `todo-css.spec.ts` 锁的是样式表里的那套 mask 配方（形状来自 `flag.svg`、颜色来自
 * `background-color`、宽高按 6:7 推导），可它看不出两块东西：一是「组件到底有没有把字形
 * 地址交给样式表」，二是「那份字形本身还能不能被解析」—— 少前者旗子退化成实心方块，
 * 少后者旗子整片不显示，而样式测试照样全绿。所以这里从组件与资源这一层补上：
 *
 * 1. 三档各一面旗，颜色类按 高 = 红 / 中 = 黄 / 低 = 蓝 的顺序排；
 * 2. 根节点必须给出 `--todo-flag-mask`（就是 `flag.svg` 的地址），否则 `mask-image` 取不到值；
 * 3. 字形资源本身必须是**能被解析的 XML**，且 viewBox 的比例与 CSS 里写死的 6:7 一致；
 * 4. 数据未到时三面旗仍是灰的、且不带数字（与「三档都是 0」区分开）；
 * 5. 计数封顶在 `99+`，免得三位数把一格宽的槽位撑破。
 */
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import TodoModule from './TodoModule.vue'
import { useTodoStore, type TodoSnapshot, type TodoTaskItem } from './stores/todo'

/** 小旗子的字形资源（`?raw` 读原文，用来验证它本身没坏）。 */
const flagAssets = import.meta.glob('/src/assets/flag.svg', {
  eager: true,
  query: '?raw',
  import: 'default',
}) as Record<string, string>

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
// store 在 setup 里读窗口 label（只在悬浮窗起轮询）；测试环境没有 Tauri 运行时。
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ label: 'floating' }) }))

/**
 * 造一条任务：只写测试关心的字段，其余给中性值。
 *
 * @param priority 滴答优先级（5 高 / 3 中 / 1 低 / 0 无）
 * @param id 任务 id（同一份快照里要唯一）
 * @returns 一条未完成的普通任务
 */
function makeTask(priority: number, id: string): TodoTaskItem {
  return {
    id,
    title: '任务',
    project_id: 'inbox',
    project_name: '收件箱',
    priority,
    due_at_ms: null,
    has_time: false,
    items_total: 0,
    items_done: 0,
    completed_at_ms: null,
    parent_id: null,
    subtasks: [],
  }
}

/**
 * 挂载悬浮窗视图。
 *
 * @param snapshot 要灌进 store 的快照；null = 还没拉到数据
 * @returns 组件包装器
 */
function mountModule(snapshot: TodoSnapshot | null) {
  const pinia = createPinia()
  setActivePinia(pinia)
  const todoStore = useTodoStore()
  todoStore.snapshot = snapshot
  return mount(TodoModule, { props: { layoutMode: 'square' }, global: { plugins: [pinia] } })
}

/**
 * 造一份只剩计数的快照。
 *
 * @param counts 高 / 中 / 低三档的条数
 * @returns 快照
 */
function snapshotOf(counts: [number, number, number]): TodoSnapshot {
  const list: TodoTaskItem[] = []
  const priorities = [5, 3, 1]
  counts.forEach((count, index) => {
    for (let n = 0; n < count; n += 1) list.push(makeTask(priorities[index], `${index}-${n}`))
  })
  return { list, total_incomplete: list.length, completed: [], updated_at_ms: 1_700_000_000_000 }
}

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('TodoModule · 三面优先级小旗', () => {
  it('paints one flag per bucket in the fixed high / medium / low order', () => {
    const wrapper = mountModule(snapshotOf([2, 1, 3]))

    const flags = wrapper.findAll('.todo-module__flag')
    expect(flags).toHaveLength(3)
    expect(flags[0].classes()).toContain('todo-module__flag--red')
    expect(flags[1].classes()).toContain('todo-module__flag--yellow')
    expect(flags[2].classes()).toContain('todo-module__flag--blue')
  })

  it('hands the flag glyph url to the stylesheet so the mask can find it', () => {
    const wrapper = mountModule(snapshotOf([1, 0, 0]))

    // 少了这个变量，`.todo-module__flag` 的 mask-image 取不到值 → 旗子退化成实心方块
    const style = wrapper.get('.todo-module').attributes('style') ?? ''
    expect(style).toContain('--todo-flag-mask')
    expect(style).toContain('svg')
  })

  it('keeps the counts next to the flags and caps them at 99+', () => {
    const wrapper = mountModule(snapshotOf([2, 8, 199]))

    // 「旗 + 数字」成组：一档一面旗、一个数字
    expect(wrapper.findAll('.todo-module__count').map(node => node.text())).toEqual(['2', '8', '99+'])
  })

  it('shows three grey flags without numbers before the data arrives', () => {
    const wrapper = mountModule(null)

    expect(wrapper.get('.todo-module').classes()).toContain('todo-module--empty')
    // 空串（不是 0）：写 0 会让人以为「真的没有任务」，而这只是还没拉到数据
    expect(wrapper.findAll('.todo-module__count').map(node => node.text())).toEqual(['', '', ''])
  })

  it('keeps the glyph asset parseable and in the ratio the stylesheet assumes', () => {
    const entry = Object.entries(flagAssets).find(([path]) => path.endsWith('flag.svg'))
    expect(entry, '没找到 src/assets/flag.svg').toBeTruthy()
    const svg = entry?.[1] ?? ''

    /*
     * ⚠️ XML 注释里**不能出现连续两个减号**：在注释里写 CSS 变量名（如那个旗子高度变量）
     * 就会踩到，整份 SVG 解析失败 → 旗子整片不显示，而别的测试全绿（mask 地址照给、
     * 颜色照上、盒子尺寸照算），是最难从代码上看出来的一种坏。
     */
    for (const comment of svg.match(/<!--[\s\S]*?-->/g) ?? []) {
      expect(comment.slice(4, -3), 'XML 注释里不得出现连续两个减号').not.toContain('--')
    }

    // viewBox 的 12:14（= 6:7）就是 CSS 里宽高推导的依据：改了字形必须同步改那边
    expect(svg).toContain('viewBox="0 0 12 14"')
  })
})
