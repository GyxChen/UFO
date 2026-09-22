// @vitest-environment happy-dom

/**
 * 悬浮窗视图的回归护栏：**三组「数字 + 色条」**。
 *
 * 这一版把优先级从「旗子字形」换成了「数字 + 色条」（方案 A · 数字主导）：
 * 45px 方形里组件只有约 39×39px，9px 的旗形既认不出形状、又和数字抢那点宽度，
 * 旧版只好在窄槽位把数字整块降级掉 —— 方格里就只剩三面小旗，读不出任何条数。
 * 现在数字是主角、色条只当「哪一档」的锚点。这里锁住几件组件这一层才能保证的事：
 *
 * 1. 三组按 高 = 红 / 中 = 黄 / 低 = 蓝 的固定顺序排，每组都是「数字 + 色条」两件；
 * 2. 位数档（`--wide` / `--xwide`）挂在**整块**上、且由**最长**的那个计数决定 ——
 *    挂在单个数字上会让三组大小参差；漏挂则在 39px 的方格里直接溢出；
 * 3. 数据未到时数字是空串（不是 `0`）、色条留位（格子不跳）；
 * 4. 计数封顶在 `99+`，免得三位数把一格宽的槽位撑破。
 *
 * ⚠️ 样式侧（色条怎么画、按形态怎么排、降档降多少）在 `todo-css.spec.ts` 里锁。
 */
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import TodoModule from './TodoModule.vue'
import { useTodoStore, type TodoSnapshot, type TodoTaskItem } from './stores/todo'

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

describe('TodoModule · 三组「数字 + 色条」', () => {
  it('paints one group per bucket in the fixed high / medium / low order', () => {
    const wrapper = mountModule(snapshotOf([2, 1, 3]))

    const cells = wrapper.findAll('.todo-module__cell')
    expect(cells).toHaveLength(3)
    expect(cells[0].classes()).toContain('todo-module__cell--red')
    expect(cells[1].classes()).toContain('todo-module__cell--yellow')
    expect(cells[2].classes()).toContain('todo-module__cell--blue')

    // 每组都是「数字 + 色条」两件：少了色条就只剩数字，优先级无处承载（只靠位置）
    expect(wrapper.findAll('.todo-module__count').map(node => node.text())).toEqual(['2', '1', '3'])
    expect(cells.every(cell => cell.find('.todo-module__tone').exists())).toBe(true)
    // 色条是纯装饰：优先级由整块的 aria-label 说给读屏听，不要在无障碍树里重复一遍数字
    expect(wrapper.findAll('.todo-module__tone[aria-hidden="true"]')).toHaveLength(3)

    // DOM 顺序 = 数字在前、色条在后。两个形态的摆位都从它推出来：竖条是 `flex-direction: row`
    // （数字在左、色条在右），横排是 `column`（数字在上、色条在下）—— 顺序反过来竖条就成了色条在左。
    for (const cell of cells) {
      expect(cell.element.children[0].className).toContain('todo-module__count')
      expect(cell.element.children[1].className).toContain('todo-module__tone')
    }
  })

  it('caps the counts at 99+ so three digits cannot burst the cell', () => {
    const wrapper = mountModule(snapshotOf([2, 8, 199]))

    expect(wrapper.findAll('.todo-module__count').map(node => node.text())).toEqual(['2', '8', '99+'])
  })

  it('puts the digit-width step on the whole block, keyed off the longest count', () => {
    // 一位数：不降档
    expect(mountModule(snapshotOf([2, 1, 3])).get('.todo-module__groups').classes()).not.toContain(
      'todo-module__groups--wide'
    )
    // 出现两位数：整块降一档
    expect(mountModule(snapshotOf([12, 1, 3])).get('.todo-module__groups').classes()).toContain(
      'todo-module__groups--wide'
    )
    // 出现三位数（含 `99+`）：降到最紧那档，且是**整块**一起降 —— 三组数字大小必须一致
    const xwide = mountModule(snapshotOf([2, 1, 199])).get('.todo-module__groups')
    expect(xwide.classes()).toContain('todo-module__groups--xwide')
    expect(xwide.classes()).not.toContain('todo-module__groups--wide')
    expect(xwide.findAll('.todo-module__count')).toHaveLength(3)
  })

  it('shows empty numbers with the bars still in place before the data arrives', () => {
    const wrapper = mountModule(null)

    expect(wrapper.get('.todo-module').classes()).toContain('todo-module--empty')
    // 空串（不是 0）：写 0 会让人以为「真的没有任务」，而这只是还没拉到数据；
    // 色条仍在原位（盒子不塌），三组的位置与格子因此不会跳。
    expect(wrapper.findAll('.todo-module__count').map(node => node.text())).toEqual(['', '', ''])
    expect(wrapper.findAll('.todo-module__tone')).toHaveLength(3)
    // 空态不该带上位数档：没有数字可降
    expect(wrapper.get('.todo-module__groups').classes()).not.toContain('todo-module__groups--wide')
  })

  it('describes the three buckets plus the total for screen readers', () => {
    const wrapper = mountModule(snapshotOf([2, 1, 3]))

    // 悬浮窗里只有数字与色条，读屏拿不到「哪档几颗」以外的信息，整块必须自己说清楚
    expect(wrapper.get('.todo-module').attributes('aria-label')).toBe('高 2，中 1，低 3，共 6 个未完成任务')
    expect(mountModule(null).get('.todo-module').attributes('aria-label')).toBe('滴答待办（等待数据）')
  })
})
