/**
 * 待办《完成情况》的树形视图模型：把后端发来的**扁平**任务列表按 `parent_id` 组装成
 * 「目标 → 子目标」的树，并给出勾选 / 连带完成所需的判断。
 *
 * 视图只管渲染，四条口径在这里定死：
 *
 * 1. **只有未完成任务能当根**（已完成任务自己单独一组列出），但**子目标里保留已完成项**：
 *    后端只把未完成的任务放进 `list`，若子目标也只认 `list`，「子目标 1/3」会永远算成 `0/N`，
 *    父目标也就判不出「子目标是不是都完成了」。
 *    ⚠️ 保留在**模型**里，不代表要渲染出来：已完成子目标在「未完成」组里不占行（见
 *    [`openChildren`]），否则那一组里会冒出打勾的行，看着像「没完成的任务被勾了」；
 * 2. **已完成的任务在树里是叶子**（不再往下挂）。它的未完成子目标按第 3 条自己当根，
 *    归属因此与遍历顺序无关（否则子目标挂在哪一层要看 `open` 的排序，每次刷新都可能变）；
 * 3. **父目标已完成、子目标还没完成**的（用户在滴答 App 里单独勾掉了父目标）子目标提升为根 ——
 *    否则它会从「未完成」组里彻底消失（父目标不在未完成列表里，没人认领它）；
 * 4. 组装带**环保护**：`parent_id` 是外部数据，成环时不能让视图无限递归、也不能让任务凭空消失。
 *
 * ⚠️ 「完成」的判定一律看 `completed_at_ms`，不看 `status`：后端已经把状态折进这个字段。
 * ⚠️ 滴答的「子目标」是**父子任务**（独立 Task 带 `parentId`），不是勾选清单的 `items`
 *    （`items_total` / `items_done` 是另一个功能，本文件不碰）。
 */

import type { TodoTaskItem } from '../stores/todo'

/** 缩进的最大层级：再深的树也按这一层缩进，免得内容被推到卡片外面。 */
const MAX_INDENT_DEPTH = 3

/**
 * 展示树的一个节点：任务本体 + 由 `parent_id` 组装出的子目标。
 */
export interface TodoTreeNode {
  /** 任务本体（`completed_at_ms` 非空 = 已完成） */
  task: TodoTaskItem
  /**
   * 子目标（**含已完成项**，用来算进度）。
   * 已完成的项目己这一层恒为空数组（见文件头第 2 条口径）。
   */
  children: TodoTreeNode[]
}

/**
 * 可渲染的一行：树 → 行序列（[`flattenTree`] 的结果）。
 */
export interface TodoTreeRow {
  /** 本行的节点 */
  node: TodoTreeNode
  /**
   * 从根到本行**父级**的祖先链（不含本行自己）。
   * 点标题时算「要不要连带完成父目标」要用它，见 [`cascadeTargets`]。
   */
  ancestors: TodoTreeNode[]
  /** 缩进层级（根 = 0，已按 [`MAX_INDENT_DEPTH`] 夹取） */
  depth: number
}

/**
 * 把一条任务包成「没有子目标」的叶子节点。
 *
 * 已完成组是**扁平**的（后端不挂子目标树），渲染时用它与未完成组的行组件共用一套 DOM。
 *
 * @param task 任务
 * @returns 叶子节点
 */
export function leafNode(task: TodoTaskItem): TodoTreeNode {
  return { task, children: [] }
}

/**
 * 把扁平的未完成任务列表按 `parent_id` 组装成树。
 *
 * 根 = 未完成且（没有父任务 / 父任务不在本次数据里 / 父任务已完成）的任务。
 * 根保持传入顺序（后端已按截止 + 优先级排好），并按 `rootLimit` 截断 —— 卡片高度固定，
 * 根太多就滚不完了。
 * 子目标的顺序是「未完成的在前、已完成的沉底」，两边各自保持后端给的顺序。
 *
 * @param open 未完成任务（如 `snapshot.list`）
 * @param completed 已完成任务（如 `snapshot.completed`，用于补齐已勾掉的子目标）
 * @param rootLimit 根节点的上限（超出部分不展示；0 = 不展示）
 * @returns 根节点数组（每个根下可能带多级子目标）
 */
export function buildTodoTree(
  open: readonly TodoTaskItem[],
  completed: readonly TodoTaskItem[],
  rootLimit: number
): TodoTreeNode[] {
  const nodes = new Map<string, TodoTreeNode>()
  for (const task of open) nodes.set(task.id, leafNode(task))
  // 已完成的只在「未完成里没有同 id」时补进来（同 id 以未完成那份为准）
  for (const task of completed) {
    if (!nodes.has(task.id)) nodes.set(task.id, leafNode(task))
  }

  // 按父 id 归类，供下面递归时取用。已完成的子目标也要挂进父目标 —— 进度 `N/M` 就靠它们
  // （父任务是否已完成、要不要再往下递归，由 `grow` 决定）。
  // 归类顺序 = `nodes` 的插入顺序 = 未完成任务在前、已完成任务在后，于是子目标的展示顺序
  // 天然是「没做完的在上、做完的沉到底部」（两边各自保持后端给的顺序）。
  const childrenOf = new Map<string, TodoTaskItem[]>()
  for (const node of nodes.values()) {
    const parentId = node.task.parent_id
    if (!parentId || parentId === node.task.id) continue
    if (!nodes.has(parentId)) continue
    const siblings = childrenOf.get(parentId)
    if (siblings) siblings.push(node.task)
    else childrenOf.set(parentId, [node.task])
  }

  /** 走过的 id：同一条任务只出现在树里一次，成环时后面的分支被丢掉（递归因此必然收敛）。 */
  const visited = new Set<string>()

  const grow = (task: TodoTaskItem): TodoTreeNode => {
    visited.add(task.id)
    if (task.completed_at_ms != null) return leafNode(task)
    const children = (childrenOf.get(task.id) ?? [])
      .filter(child => !visited.has(child.id))
      .map(child => grow(child))
    return { task, children }
  }

  /**
   * 该不该由自己当根：父任务不存在 / 就是自己 / 已完成 → 当根；
   * 父任务未完成 → 交给父任务认领（它才该把这条挂在下面）。
   *
   * ⚠️ 已完成的任务**永远不当根**：「未完成」组里冒出一行打勾的，看着就像没做完的任务被勾了。
   * 后端已按 `status` 筛过（见 `models.rs` 的 `is_completed`），这里再兜一层 ——
   * 这条不变量的代价太低，而破了它的观感太差。
   */
  const isRootCandidate = (task: TodoTaskItem): boolean => {
    if (task.completed_at_ms != null) return false
    const parentId = task.parent_id
    if (!parentId || parentId === task.id) return true
    const parent = nodes.get(parentId)
    return !parent || parent.task.completed_at_ms != null
  }

  /** 能当根的任务（未完成）：两轮建根都只从这儿取。 */
  const rootSource = open.filter(task => task.completed_at_ms == null)

  const roots: TodoTreeNode[] = []
  for (const task of rootSource) {
    if (!isRootCandidate(task) || visited.has(task.id)) continue
    roots.push(grow(task))
  }
  // 兜底：`parent_id` 成环时上面谁都不会入选（互相是对方的父任务），这些任务会整枝消失。
  // 按原顺序补成根，保证每条未完成任务都能出现在树里。
  for (const task of rootSource) {
    if (visited.has(task.id)) continue
    roots.push(grow(task))
  }
  return roots.slice(0, Math.max(rootLimit, 0))
}

/**
 * 会在「未完成」组里**单独占一行**的子目标：只含未完成的那部分。
 *
 * 已完成的子目标**不渲染**（未完成组里冒出打勾的行，看着就像「没完成的任务被勾了」；
 * 而且它们在下面「已完成」组里本来就有一份）。但它们仍留在 `node.children` 里 ——
 * 进度 `N/M`、[`isTogglable`] 与 [`cascadeTargets`] 都要靠完整的孩子集合。
 *
 * @param node 树节点
 * @returns 未完成的子目标（保持树里的顺序）
 */
export function openChildren(node: TodoTreeNode): TodoTreeNode[] {
  return node.children.filter(child => child.task.completed_at_ms == null)
}

/**
 * 一个节点的子目标进度（**直接子目标**，含已完成项）。
 *
 * @param node 树节点
 * @returns `{ done, total }`；没有子目标时 `total` 为 0
 */
export function subtreeProgress(node: TodoTreeNode): { done: number; total: number } {
  const total = node.children.length
  const done = node.children.filter(child => child.task.completed_at_ms != null).length
  return { done, total }
}

/**
 * 这条任务能不能被直接勾选 / 取消勾选。
 *
 * 有**未完成**子目标时不能 —— 完成只由子目标驱动（最后一个子目标完成时父目标自动完成），
 * 允许手动勾父目标会让父子状态互相矛盾（父目标完成了、子目标还挂着）。
 * 子目标全都已完成时仍可点：数据可能来自滴答 App 等外部来源，
 * 那时这条父目标既自动完成不了、又不让点，就永远卡住了。
 *
 * @param node 树节点
 * @returns 可点时为 true
 */
export function isTogglable(node: TodoTreeNode): boolean {
  return node.children.every(child => child.task.completed_at_ms != null)
}

/**
 * 树 → 可渲染的行序列（深度优先，父在子前）。
 *
 * 只走**未完成**的子目标（见 [`openChildren`]），折叠的分支则**整枝**跳过（连同它的后代）。
 * 于是视图只需要一个 `v-for`：不用递归组件，也不用在组件之间转发事件。
 *
 * @param roots 根节点（[`buildTodoTree`] 的结果）
 * @param collapsedIds 处于折叠态的任务 id
 * @returns 行序列
 */
export function flattenTree(
  roots: readonly TodoTreeNode[],
  collapsedIds: ReadonlySet<string>
): TodoTreeRow[] {
  const rows: TodoTreeRow[] = []
  const walk = (node: TodoTreeNode, ancestors: TodoTreeNode[], depth: number): void => {
    rows.push({ node, ancestors, depth: Math.min(depth, MAX_INDENT_DEPTH) })
    if (collapsedIds.has(node.task.id)) return
    for (const child of openChildren(node)) walk(child, [...ancestors, node], depth + 1)
  }
  for (const root of roots) walk(root, [], 0)
  return rows
}

/**
 * 完成 `justCompletedId` 之后，需要**跟着一起完成**的祖先（由近到远）。
 *
 * 规则：某祖先的**全部子目标都已完成**时，它就该被自动完成 —— 这就是「最后一个子目标完成，
 * 父目标自动完成」；父目标完成了，它自己的父目标也可能满足条件，所以一路往上推。
 * 遇到已经完成态的祖先直接停住：它已经在完成态，再往上不该由这次操作负责。
 *
 * 判「子目标已完成」时既看它自己的 `completed_at_ms`（树里带着已完成项），也看本轮刚完成的
 * 那一个（它在提交返回前仍是未完成态），因此不需要调用方再拼一份 id 集合。
 *
 * @param ancestors 从根到被点节点父级的祖先链（`TodoTreeRow.ancestors`）
 * @param justCompletedId 本次被点完成的任务 id
 * @returns 需要连带完成的任务，**由近到远**（按顺序提交，父目标在子目标之后）
 */
export function cascadeTargets(
  ancestors: readonly TodoTreeNode[],
  justCompletedId: string
): TodoTaskItem[] {
  const done = new Set([justCompletedId])
  const targets: TodoTaskItem[] = []
  for (let index = ancestors.length - 1; index >= 0; index -= 1) {
    const ancestor = ancestors[index]
    if (ancestor.task.completed_at_ms != null) break
    const allDone = ancestor.children.every(
      child => child.task.completed_at_ms != null || done.has(child.task.id)
    )
    if (!allDone) break
    targets.push(ancestor.task)
    done.add(ancestor.task.id)
  }
  return targets
}
