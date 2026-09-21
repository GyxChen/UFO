// @vitest-environment node

/**
 * 《完成情况》树形视图模型的单测。
 *
 * 锁的是五条容易改回去的口径：
 * 1. 已完成的子目标要留在父目标下面（否则进度 `N/M` 永远算成 `0/N`）；
 * 2. 已完成的任务在树里是**叶子**，它的未完成子目标自己当根（归属与遍历顺序无关）；
 * 3. `parent_id` 成环这类坏数据不能无限递归、也不能让任务凭空消失；
 * 4. 「最后一个子目标完成 → 父目标自动完成」的推断（`cascadeTargets`）；
 * 5. 已完成的子目标留在**模型**里，但**不渲染成行**（未完成组里不该出现打勾的行）。
 */

import { describe, expect, it } from 'vitest'
import type { TodoTaskItem } from '../stores/todo'
import {
  buildTodoTree,
  cascadeTargets,
  flattenTree,
  isTogglable,
  leafNode,
  openChildren,
  subtreeProgress,
  type TodoTreeNode,
} from './todo-tree'

/** 造一条任务，只覆盖测试关心的字段。 */
function task(
  id: string,
  parentId: string | null = null,
  completedAt: number | null = null,
  priority = 0
): TodoTaskItem {
  return {
    id,
    title: id,
    project_id: 'p1',
    project_name: '工作',
    priority,
    due_at_ms: null,
    has_time: false,
    items_total: 0,
    items_done: 0,
    completed_at_ms: completedAt,
    parent_id: parentId,
    subtasks: [],
  }
}

/** 手搓一个树节点（测 flatten / cascade 时比走一遍 buildTodoTree 更直白）。 */
function node(id: string, children: TodoTreeNode[] = [], completedAt: number | null = null): TodoTreeNode {
  return { task: task(id, null, completedAt), children }
}

/** 取树的形状（`id(子1,子2)`），断言起来比逐层钻对象清楚。 */
function shape(nodes: readonly TodoTreeNode[]): string {
  return nodes.map(item => (item.children.length ? `${item.task.id}(${shape(item.children)})` : item.task.id)).join(',')
}

describe('buildTodoTree', () => {
  it('nests open children under their open parent and keeps the incoming order', () => {
    const open = [task('S'), task('P'), task('A', 'P'), task('B', 'P')]
    const roots = buildTodoTree(open, [], 30)

    expect(shape(roots)).toBe('S,P(A,B)')
  })

  it('keeps completed children under the parent so the progress counts them', () => {
    // 后端只把未完成的任务放进 list，已勾掉的子目标只在 completed 里
    const open = [task('P'), task('B', 'P')]
    const completed = [task('A', 'P', 1_700_000_000_000)]

    const roots = buildTodoTree(open, completed, 30)

    // 未完成的子目标在前、已完成的沉底（这一条是显示口径，别改成按 id / 按时间混排）
    expect(shape(roots)).toBe('P(B,A)')
    expect(subtreeProgress(roots[0])).toEqual({ done: 1, total: 2 })
  })

  it('promotes an open child whose parent is already completed', () => {
    // 父目标在滴答 App 里被单独勾掉了：子目标不能跟着从「未完成」组里消失
    const open = [task('D', 'C')]
    const completed = [task('C', null, 1_700_000_000_000)]

    expect(shape(buildTodoTree(open, completed, 30))).toBe('D')
  })

  it('treats completed tasks as leaves and never nests under them', () => {
    // C 已完成、D 是 C 的未完成子目标：C 当叶子挂在 P 下面，D 自己当根。
    // 归属因此与 open 的排序无关（若允许往已完成节点里递归，D 挂在哪就取决于谁先被遍历到）。
    const open = [task('P'), task('D', 'C')]
    const completed = [task('C', 'P', 1_700_000_000_000)]

    expect(shape(buildTodoTree(open, completed, 30))).toBe('P(C),D')
  })

  it('survives a parent_id cycle without hanging or dropping tasks', () => {
    // A 的父是 B、B 的父是 A：互相认领的话谁都不会入选根，两条都会消失
    const open = [task('A', 'B'), task('B', 'A')]

    expect(shape(buildTodoTree(open, [], 30))).toBe('A(B)')
  })

  it('caps the number of roots at rootLimit', () => {
    const open = [task('A'), task('B'), task('C')]

    expect(shape(buildTodoTree(open, [], 2))).toBe('A,B')
  })

  it('never roots a completed task that leaked into the open list', () => {
    // 「未完成」组里冒出一行打勾的，观感就是「没做完的任务被勾了」。
    // 后端已按 status 筛过（见 models.rs 的 is_completed），前端再兜一层：
    // 无父的那条靠根过滤挡掉，有父的那条即便挂进了树，渲染时也会被 openChildren 滤掉。
    const open = [task('A'), task('泄漏的已完成', null, 1_700_000_000_000), task('带父的已完成', 'A', 1_700_000_000_000)]

    const rows = flattenTree(buildTodoTree(open, [], 30), new Set())

    expect(rows.map(row => row.node.task.id)).toEqual(['A'])
    expect(rows.every(row => row.node.task.completed_at_ms == null)).toBe(true)
  })
})

describe('isTogglable', () => {
  it('blocks a parent that still has open children', () => {
    const parent = node('P', [node('A'), node('B', [], 1_700_000_000_000)])

    expect(isTogglable(parent)).toBe(false)
  })

  it('allows a parent whose children are all completed', () => {
    // 数据来自滴答 App 等外部来源时会出现这种「子目标都完了、父目标还没完」的组合，
    // 不让点的话这条父目标就永远卡住了
    const parent = node('P', [node('A', [], 1_700_000_000_000)])

    expect(isTogglable(parent)).toBe(true)
  })

  it('allows a leaf', () => {
    expect(isTogglable(node('A'))).toBe(true)
  })
})

describe('flattenTree', () => {
  const tree = [node('P', [node('A'), node('B')]), node('S')]

  it('walks the tree depth-first with the ancestor chain on every row', () => {
    const rows = flattenTree(tree, new Set())

    expect(rows.map(row => row.node.task.id)).toEqual(['P', 'A', 'B', 'S'])
    expect(rows.map(row => row.depth)).toEqual([0, 1, 1, 0])
    // 祖先链给的是「从根到父级」，点标题时算连带完成要用它
    expect(rows[1].ancestors.map(item => item.task.id)).toEqual(['P'])
    expect(rows[0].ancestors).toEqual([])
  })

  it('skips the whole branch of a collapsed node', () => {
    const rows = flattenTree(tree, new Set(['P']))

    expect(rows.map(row => row.node.task.id)).toEqual(['P', 'S'])
  })

  it('caps the indent depth so deep trees stay inside the card', () => {
    const deep = node('L0', [node('L1', [node('L2', [node('L3', [node('L4')])])])])

    expect(flattenTree([deep], new Set()).map(row => row.depth)).toEqual([0, 1, 2, 3, 3])
  })

  it('leaves completed children out of the rows but keeps counting them', () => {
    // 未完成组里冒出一行打勾的，看起来就像「没完成的任务被勾了」——
    // 已完成的子目标不占行（它们在下面「已完成」组里本来就有一份）
    const tree = [node('P', [node('A', [], 1_700_000_000_000), node('B')])]

    const rows = flattenTree(tree, new Set())

    expect(rows.map(row => row.node.task.id)).toEqual(['P', 'B'])
    // 但进度仍按完整的孩子集合算：漏掉的话会显示 0/1
    expect(subtreeProgress(tree[0])).toEqual({ done: 1, total: 2 })
  })

  it('hides completed children that came from the completed list', () => {
    // 端到端复现：「未完成」组里不该出现任何已勾掉的行
    const open = [task('P'), task('B', 'P')]
    const completed = [task('A', 'P', 1_700_000_000_000)]

    const rows = flattenTree(buildTodoTree(open, completed, 30), new Set())

    expect(rows.map(row => row.node.task.id)).toEqual(['P', 'B'])
    expect(rows.every(row => row.node.task.completed_at_ms == null)).toBe(true)
  })
})

describe('openChildren', () => {
  it('keeps only the unfinished children', () => {
    const parent = node('P', [node('A'), node('B', [], 1_700_000_000_000)])

    expect(openChildren(parent).map(item => item.task.id)).toEqual(['A'])
  })
})

describe('cascadeTargets', () => {
  it('completes the ancestors whose children are all done, nearest first', () => {
    // 叶子 L 完成后：A 的子目标全完 → A 完成；A 完成后 P 的子目标也全完 → P 完成
    const leaf = node('L')
    const a = node('A', [leaf])
    const p = node('P', [a])

    expect(cascadeTargets([p, a], 'L').map(item => item.id)).toEqual(['A', 'P'])
  })

  it('stops at the first ancestor that still has an open sibling', () => {
    const leaf = node('L')
    const other = node('X')
    const a = node('A', [leaf, other])
    const p = node('P', [a])

    expect(cascadeTargets([p, a], 'L')).toEqual([])
  })

  it('counts an already completed sibling as done', () => {
    const leaf = node('L')
    const sibling = node('X', [], 1_700_000_000_000)
    const a = node('A', [leaf, sibling])

    expect(cascadeTargets([a], 'L').map(item => item.id)).toEqual(['A'])
  })

  it('does not walk past an ancestor that is already completed', () => {
    // A 已经是完成态（外部数据）：再往上不该由这次操作负责
    const leaf = node('L')
    const a = node('A', [leaf], 1_700_000_000_000)
    const p = node('P', [a])

    expect(cascadeTargets([p, a], 'L')).toEqual([])
  })
})

describe('leafNode', () => {
  it('wraps a task with no children so both groups share one row component', () => {
    const wrapped = leafNode(task('A'))

    expect(wrapped.task.id).toBe('A')
    expect(wrapped.children).toEqual([])
    expect(isTogglable(wrapped)).toBe(true)
  })
})
