<script setup lang="ts">
import { computed, ref } from 'vue'
import TooltipRow from '@/components/floating/tooltip/TooltipRow.vue'
import TooltipSection from '@/components/floating/tooltip/TooltipSection.vue'
import type { FloatingHoverTarget } from '@/lib/floating-module-context'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import { dueBucket, dueTone, formatClockTime, formatDue, isOverdue } from './lib/todo-format'
import { buildTodoTree, cascadeTargets, flattenTree, subtreeProgress, type TodoTreeRow } from './lib/todo-tree'
import { todoSettings } from './settings'
import { useTodoStore, type TodoTaskItem } from './stores/todo'
import TodoTaskRow from './sections/TodoTaskRow.vue'

/**
 * 滴答待办的信息卡：**设置页「完成情况」里那一组「未完成」的原样搬过来**
 * ——同一套行组件（`TodoTaskRow`）、同一套树（`lib/todo-tree.ts`）、同一套 `.todo-status__*` 样式，
 * 所以勾选框配色、子目标缩进、进度 `N/M`、折叠箭头、连带完成父目标的规则两处完全一致。
 *
 * 与设置页的三点差别（刻意为之）：
 * 1. **不带右键菜单**（顺延 / 删除）：那两条是破坏性 / 批量写操作，留在设置页；
 * 2. **不带卡头的「刷新 / 全部顺延」按钮**：信息卡是「看一眼 + 勾一下」的地方；
 * 3. **标题最多换三行、之后才省略**（设置页仍是一行省略）：信息卡是停下来读一条的地方，
 *    标题被截一半就白搭；样式在 `todo.css` 的 `.todo-tooltip__scroll .todo-status__title`。
 *
 * 多根节点渲染（`.tooltip-shell` 直接是行级 flex 容器），所以关掉属性透传；
 * `target` 声明成 prop 是「组件自己的契约」——提示窗只负责按 moduleId 分派并透传悬停目标。
 *
 * 高度预算（窗口 408×400，见 `index.ts` 的 `tooltip.size`；宽在原来 340 上按 1.2× 放大，
 * 好让三行标题每行装得下更多字）：概览 4 行 ≈ 80px，剩下 ≈ 300px 归滚动区 ——
 * 任务再多 / 标题再高也只是滚，不会把概览顶出窗口。
 */
defineOptions({ inheritAttrs: false })

defineProps<{
  /** 悬停目标（哪个模块的哪个对象）；本卡是整块聚合信息，只用它满足提示窗契约。 */
  target: FloatingHoverTarget
}>()

const todoStore = useTodoStore()
const modulesStore = useFloatingModulesStore()

/** 树里最多渲染多少个根目标（与设置页同口径：卡片 / 窗口高度固定，根太多就滚不完）。 */
const PREVIEW_LIMIT = 30

/** 最近一次「完成」失败的原因；成功或重试时清空。 */
const errorText = ref<string | null>(null)

const snapshot = computed(() => todoStore.snapshot)

/** 当前生效的清单筛选（信息卡要说明「这些是哪个范围里的任务」）。 */
const settings = computed(() => todoSettings(modulesStore.config))

/** 逾期条数（参考时刻取快照构建时刻，保证整卡口径一致）。 */
const overdueCount = computed(() => {
  const data = snapshot.value
  if (!data) return 0
  return data.list.filter(task => isOverdue(task.due_at_ms, data.updated_at_ms, task.has_time)).length
})

/**
 * 处于折叠态的任务 id（按任务记，快照刷新后仍保持）。
 *
 * 只装 id 不装节点：快照每轮都是新对象，存节点会立刻过期；存 id 则跨刷新有效。
 * ⚠️ 这个状态活在信息卡组件里，而信息卡是「隐藏即卸载」的（`TooltipWindow` 的 v-if）——
 * 下次悬停时折叠态会重置成「全展开」，这是可接受的：信息卡本来就是一次性的快照视图。
 */
const collapsedTaskIds = ref<ReadonlySet<string>>(new Set())

/**
 * 未完成组的树（子目标挂在父目标下，含已勾掉的子目标 —— 进度 `N/M` 靠它们）。
 * 与设置页同一个口径：`completed` 也要传进来，否则子目标进度永远算成 `0/N`。
 */
const incompleteTree = computed(() =>
  buildTodoTree(snapshot.value?.list ?? [], snapshot.value?.completed ?? [], PREVIEW_LIMIT)
)

/** 要渲染的行序列（折叠的分支整枝跳过，视图只用一个 `v-for`）。 */
const rows = computed(() => flattenTree(incompleteTree.value, collapsedTaskIds.value))

/**
 * 展开 / 收起某条目标的子目标。
 *
 * @param taskId 目标 id
 */
function toggleCollapsed(taskId: string): void {
  const next = new Set(collapsedTaskIds.value)
  if (next.has(taskId)) next.delete(taskId)
  else next.add(taskId)
  collapsedTaskIds.value = next
}

/**
 * 该任务是否正在提交完成（提交中把行置灰，挡掉重复点击）。
 *
 * @param taskId 任务 id
 * @returns 提交中时为 true
 */
function isPending(taskId: string): boolean {
  return todoStore.pendingTaskIds.includes(taskId)
}

/**
 * 未完成行的子目标进度文案（如 `1/3`）；没有子目标时返回空串。
 *
 * @param row 行
 * @returns 进度文案；无子目标为空串
 */
function subtaskLabel(row: TodoTreeRow): string {
  const progress = subtreeProgress(row.node)
  if (progress.total === 0) return ''
  return `子目标 ${progress.done}/${progress.total}`
}

/**
 * 任务的截止文案（带时刻，信息卡里空间够）。
 *
 * @param task 任务
 * @returns 中文相对日期文案；未设置截止时返回空串
 */
function dueText(task: TodoTaskItem): string {
  const data = snapshot.value
  if (!data || task.due_at_ms == null) return ''
  return formatDue(task.due_at_ms, data.updated_at_ms, task.has_time)
}

/**
 * 任务截止的语义色名。
 *
 * @param task 任务
 * @returns `red` / `yellow` / `muted`
 */
function toneOf(task: TodoTaskItem): string {
  return dueTone(dueBucket(task.due_at_ms, snapshot.value?.updated_at_ms ?? Date.now(), task.has_time))
}

/**
 * 点一条任务的勾选框 = 完成它（与设置页同一条路：连带完成父目标）。
 *
 * 本组里**只有未完成任务**（`flattenTree` 只走未完成分支），所以不存在「取消完成」那一支。
 * 完成时按 [`cascadeTargets`] 一路往上推：「最后一个子目标完成，父目标自动完成」，
 * 整条链**一次**提交给后端，避免每层都重拉一遍列表。
 *
 * 失败只把原因写进卡片顶部（错误行），不弹系统对话框：提示窗是「看一眼就走」的地方，
 * 弹窗会比错误本身更打断人；成功的路径由后端广播的新快照接管（那一行会自己消失）。
 *
 * @param row 被勾的行（`row.ancestors` 是这条任务从根到父级的祖先链）
 */
async function onToggle(row: TodoTreeRow): Promise<void> {
  const task = row.node.task
  if (isPending(task.id)) return
  errorText.value = null
  try {
    const cascade = cascadeTargets(row.ancestors, task.id)
    await todoStore.completeTasks([task, ...cascade])
  } catch (err) {
    errorText.value = typeof err === 'string' ? err : '完成任务失败'
  }
}
</script>

<template>
  <template v-if="snapshot">
    <!-- 概览：范围 + 未完成 / 逾期计数 + 更新时间（改动都在滚动区里，这几行始终看得见） -->
    <TooltipSection class="todo-tooltip__overview" style="border-top: 0;">
      <TooltipRow label="范围" :value="settings.project_name || '全部清单'" />
      <TooltipRow label="未完成" :value="`${snapshot.total_incomplete} 项`" mono />
      <TooltipRow label="已逾期" :value="`${overdueCount} 项`" :tone="overdueCount > 0 ? 'red' : 'muted'" mono />
      <TooltipRow v-if="errorText" label="完成失败" :value="errorText" tone="red" />
      <TooltipRow label="更新时间" :value="formatClockTime(snapshot.updated_at_ms)" tone="muted" mono />
    </TooltipSection>

    <!--
      「未完成」组：与设置页「完成情况」的未完成组同款（标题带计数 + 目标树）。
      整块可滚；行只有勾选框能点（提示窗显示时是接收点击的），点一下 = 完成该任务。
    -->
    <div v-if="rows.length > 0" class="todo-tooltip__scroll">
      <section class="todo-status__group">
        <h4 class="todo-status__group-title">
          未完成
          <span>{{ snapshot.total_incomplete }}</span>
        </h4>

        <ul class="todo-status__list">
          <TodoTaskRow
            v-for="row in rows"
            :key="row.node.task.id"
            :node="row.node"
            :depth="row.depth"
            :collapsed="collapsedTaskIds.has(row.node.task.id)"
            :pending="isPending(row.node.task.id)"
            :meta-tone="toneOf(row.node.task)"
            @toggle="onToggle(row)"
            @toggle-collapse="toggleCollapsed(row.node.task.id)"
          >
            <template #meta>
              <span v-if="subtaskLabel(row)">{{ subtaskLabel(row) }}</span>
              <span v-if="dueText(row.node.task)">{{ dueText(row.node.task) }}</span>
            </template>
          </TodoTaskRow>
        </ul>
      </section>
    </div>

    <TooltipSection v-else>
      <TooltipRow label="状态" value="已清空，暂无未完成任务" tone="green" />
    </TooltipSection>
  </template>

  <!-- 未配置 Token / 数据未到：给出引导，避免弹空框 -->
  <template v-else>
    <TooltipRow label="状态" :value="todoStore.lastError || '正在获取待办…'" tone="muted" />
    <TooltipSection>
      <TooltipRow label="设置" value="主窗「滴答待办 → 常规设置」里填入 API Token" tone="muted" />
    </TooltipSection>
  </template>
</template>
