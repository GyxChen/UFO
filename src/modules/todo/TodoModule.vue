<script setup lang="ts">
import { computed, inject } from 'vue'
import type { FloatingLayout } from '@/lib/floating-layout'
import { floatingModuleKey } from '@/lib/floating-module-context'
import { dueBucket, dueTone, formatDueCompact, isOverdue } from './lib/todo-format'
import { useTodoStore, type TodoTaskItem } from './stores/todo'

/**
 * 宿主注入的当前贴边形态。
 */
const props = defineProps<{
  layoutMode: FloatingLayout
}>()

/**
 * 宿主注入的模块上下文：本组件只用到「上报悬停对象」。
 * 单测 / 主窗口预览里可能没有宿主，给 null 兜底才渲染得出来。
 */
const moduleApi = inject(floatingModuleKey, null)

const todoStore = useTodoStore()

/** 悬停对象 key：整块待办只对应一份信息卡，用常量即可（外壳只当字符串转发）。 */
const HOVER_KEY = 'todo-list'

/** 悬浮窗里最多预告两条任务：贴角 / 竖条放得下，横条由 CSS 收掉第二条。 */
const PREVIEW_LIMIT = 2

/** 未完成数展示上限：超过就写 `99+`，避免三位数把窄槽位里的徽标撑破。 */
const COUNT_DISPLAY_MAX = 99

const snapshot = computed(() => todoStore.snapshot)

/** 徽标里显示的未完成数（超上限写 `99+`）。 */
const totalLabel = computed(() => {
  const total = snapshot.value?.total_incomplete ?? 0
  return total > COUNT_DISPLAY_MAX ? `${COUNT_DISPLAY_MAX}+` : String(total)
})

/** 悬浮窗里预告的任务（后端已按「截止早 → 优先级高」排好，取前两条即可）。 */
const previewTasks = computed(() => (snapshot.value?.list ?? []).slice(0, PREVIEW_LIMIT))

/** 逾期条数；> 0 时徽标转危险色，一眼看出「有欠账」。 */
const overdueCount = computed(() => {
  const data = snapshot.value
  if (!data) return 0
  return data.list.filter(task => isOverdue(task.due_at_ms, data.updated_at_ms, task.has_time)).length
})

/**
 * 某条任务在悬浮窗里的紧凑截止文案（不带时刻，宽度可控）。
 *
 * @param task 任务
 * @returns 逾期 / 今天 / 明天 / MM-DD；未设置截止时为空串
 */
function dueText(task: TodoTaskItem): string {
  const data = snapshot.value
  if (!data || task.due_at_ms == null) return ''
  return formatDueCompact(task.due_at_ms, data.updated_at_ms, task.has_time)
}

/**
 * 某条任务的截止语义色（点与文案共用）。
 *
 * @param task 任务
 * @returns `red`（逾期）/ `yellow`（今天）/ `muted`（其余）
 */
function dueToneOf(task: TodoTaskItem): string {
  const data = snapshot.value
  return dueTone(dueBucket(task.due_at_ms, data?.updated_at_ms ?? Date.now(), task.has_time))
}

/** 指针进入：把「悬停在本模块上」上报给宿主，宿主据此弹出信息卡。 */
function handlePointerEnter(): void {
  moduleApi?.reportHover(HOVER_KEY)
}

/** 指针离开：撤销悬停对象（null = 没有悬停目标）。 */
function handlePointerLeave(): void {
  moduleApi?.reportHover(null)
}
</script>

<template>
  <div
    class="todo-module"
    :class="`todo-module--${props.layoutMode}`"
    role="region"
    :aria-label="snapshot ? `${totalLabel} 个未完成任务` : '滴答待办'"
    @pointerenter="handlePointerEnter"
    @pointerleave="handlePointerLeave"
  >
    <template v-if="snapshot">
      <!-- 徽标：勾选图标 + 未完成数；有逾期任务时整块转危险色 -->
      <span class="todo-module__badge" :class="{ 'todo-module__badge--alert': overdueCount > 0 }">
        <svg class="todo-module__icon" viewBox="0 0 24 24" aria-hidden="true">
          <rect
            x="3.5"
            y="3.5"
            width="17"
            height="17"
            rx="4"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
          />
          <path
            d="m8 12 2.6 2.6L16 9"
            fill="none"
            stroke="currentColor"
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
          />
        </svg>
        <span class="todo-module__count">{{ totalLabel }}</span>
      </span>

      <!-- 最近两条任务：太小的槽位由 CSS（容器查询）整块收起 -->
      <ul v-if="previewTasks.length > 0" class="todo-module__list">
        <li v-for="task in previewTasks" :key="task.id" class="todo-module__item">
          <span
            class="todo-module__dot"
            :class="`todo-module__dot--${dueToneOf(task)}`"
            aria-hidden="true"
          />
          <span class="todo-module__title">{{ task.title }}</span>
          <span
            v-if="dueText(task)"
            class="todo-module__due"
            :class="`todo-module__due--${dueToneOf(task)}`"
          >
            {{ dueText(task) }}
          </span>
        </li>
      </ul>
    </template>

    <!-- 未配置 Token / 数据未到：只放一个中性图标占位，不显示误导性的 0 -->
    <span v-else class="todo-module__empty">
      <svg class="todo-module__icon" viewBox="0 0 24 24" aria-hidden="true">
        <rect
          x="3.5"
          y="3.5"
          width="17"
          height="17"
          rx="4"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
        />
        <path
          d="m8 12 2.6 2.6L16 9"
          fill="none"
          stroke="currentColor"
          stroke-linecap="round"
          stroke-linejoin="round"
          stroke-width="2"
        />
      </svg>
    </span>
  </div>
</template>
