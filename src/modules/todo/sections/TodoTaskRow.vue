<script setup lang="ts">
import { computed } from 'vue'
import { priorityBucket, priorityBucketLabel, priorityTone } from '../lib/todo-format'
import { isTogglable, openChildren, type TodoTreeNode } from '../lib/todo-tree'

/**
 * 《完成情况》里的一行目标。
 *
 * 三条交互约定（与设置页的其它卡片保持一致）：
 * - **只有勾选框能切换完成态**：标题是纯文字，点它什么都不做（避免「想选个字 / 想看看标题」
 *   就把任务勾掉了）；行里也不放「完成 / 取消完成」按钮 —— 一条任务只有一个开关；
 * - **优先级靠勾选框的边框表达**（颜色 + 粗细），不再另画一个优先级圆点；
 * - **有未完成子目标时父目标不能点**（勾选框转虚线、按钮 disabled）：
 *   它的完成由子目标驱动 —— 最后一个子目标完成时父目标自动完成。
 *
 * 组件本身**无状态**：折叠态、提交中、缩进层级都由调用方通过 props 传入，
 * 子目标也由调用方拍平成行序列（见 `lib/todo-tree.ts` 的 `flattenTree`），所以这里不递归。
 */

interface Props {
  /** 本行的树节点（已完成组用 `leafNode()` 包一个没有子目标的节点即可） */
  node: TodoTreeNode
  /** 缩进层级（根 = 0），子目标逐级缩进 */
  depth?: number
  /** 本行的子目标是否处于折叠态 */
  collapsed?: boolean
  /** 本行是否正在提交（完成 / 取消完成） */
  pending?: boolean
  /** 是否展示「高 / 中 / 低」标签（已完成组里没必要再提优先级） */
  showPriority?: boolean
  /** 右侧附加信息的语义色（截止紧迫度），与勾选框的优先级色是两回事 */
  metaTone?: 'red' | 'yellow' | 'muted'
}

const props = withDefaults(defineProps<Props>(), {
  depth: 0,
  collapsed: false,
  pending: false,
  showPriority: true,
  metaTone: 'muted',
})

const emit = defineEmits<{
  /** 点勾选框：未完成 → 完成（可能连带完成父目标）；已完成 → 取消完成 */
  (event: 'toggle'): void
  /** 点折叠按钮：展开 / 收起本行的子目标 */
  (event: 'toggle-collapse'): void
}>()

/** 任务本体。 */
const task = computed(() => props.node.task)
/** 是否已完成（判定只看 `completed_at_ms`，与后端口径一致）。 */
const isDone = computed(() => task.value.completed_at_ms != null)
/**
 * 有没有**可展开的**子目标：只数未完成的（已完成的子目标不占行，见 `openChildren`）。
 * 一条子目标都没得展开就不配折叠按钮了，免得点了个空箭头。
 */
const hasChildren = computed(() => openChildren(props.node).length > 0)
/** 能不能直接点：有未完成子目标时为 false（由子目标驱动完成）。 */
const togglable = computed(() => isTogglable(props.node))
/** 优先级三档与语义色（勾选框边框用的就是它）。 */
const bucket = computed(() => priorityBucket(task.value.priority))
const tone = computed(() => priorityTone(bucket.value))

/** 勾选框的无障碍标签：说清点下去会发生什么（标题是纯文字，读屏不会把它读成开关）。 */
const toggleLabel = computed(() => {
  const title = task.value.title
  if (isDone.value) return `取消完成：${title}`
  if (!togglable.value) return `「${title}」由子目标驱动，完成全部子目标后自动完成`
  return `完成：${title}`
})
</script>

<template>
  <li class="todo-status__row" :class="{ 'is-done': isDone }" :style="{ '--todo-depth': depth }">
    <!-- 折叠按钮：有子目标才有；叶子节点留同宽占位，勾选框才能上下对齐成一列 -->
    <span class="todo-status__twist">
      <button
        v-if="hasChildren"
        type="button"
        class="todo-status__twist-btn"
        :aria-expanded="!collapsed"
        :aria-label="collapsed ? `展开「${task.title}」的子目标` : `收起「${task.title}」的子目标`"
        @click="emit('toggle-collapse')"
      >
        <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
          <path
            d="M2.5 4.5 L6 8 L9.5 4.5"
            fill="none"
            stroke="currentColor"
            stroke-width="1.6"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </button>
    </span>

    <!--
      勾选框按钮：本行**唯一**能切换完成态的控件。
      命中区 24×24 包着 16px 的方框（见 todo.css）—— 视觉还是那个 16px 的小方框，
      但点起来不用瞄准；标题则退成纯文字，点它不做任何事。
    -->
    <button
      type="button"
      class="todo-status__check-btn"
      :disabled="!togglable || pending"
      :aria-label="toggleLabel"
      :aria-busy="pending || undefined"
      @click="emit('toggle')"
    >
      <!--
        优先级直接写在勾选框的**边框**上：高 = 红、中 = 黄、低 = 蓝（粗细统一加粗）。
        已完成转成实心强调色（完成是比优先级更强的信号）；由子目标驱动的转虚线段，
        一眼看出「这一条不是给你点的」。
      -->
      <span
        class="todo-status__check"
        :class="[`todo-status__check--${tone}`, { 'is-done': isDone, 'is-derived': !togglable && !isDone }]"
        aria-hidden="true"
      >{{ isDone ? '✓' : '' }}</span>
    </button>

    <span class="todo-status__title">{{ task.title }}</span>

    <span class="todo-status__meta" :class="`todo-status__meta--${metaTone}`">
      <span v-if="showPriority" class="todo-status__priority" :class="`todo-status__priority--${tone}`">
        {{ priorityBucketLabel(bucket) }}
      </span>
      <slot name="meta" />
    </span>
  </li>
</template>
