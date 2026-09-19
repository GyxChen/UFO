<script setup lang="ts">
import { computed } from 'vue'
import TooltipRow from '@/components/floating/tooltip/TooltipRow.vue'
import TooltipSection from '@/components/floating/tooltip/TooltipSection.vue'
import type { FloatingHoverTarget } from '@/lib/floating-module-context'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import {
  dueBucket,
  dueTone,
  formatClockTime,
  formatDue,
  isOverdue,
  priorityLabel,
} from './lib/todo-format'
import { todoSettings } from './settings'
import { useTodoStore, type TodoTaskItem } from './stores/todo'

/**
 * 滴答待办的信息卡。
 *
 * 多根节点渲染（`.tooltip-shell` 直接是行级 flex 容器），所以关掉属性透传；
 * `target` 声明成 prop 是「组件自己的契约」——提示窗只负责按 moduleId 分派并透传悬停目标。
 */
defineOptions({ inheritAttrs: false })

defineProps<{
  /** 悬停目标（哪个模块的哪个对象）；本卡是整块聚合信息，只用它满足提示窗契约。 */
  target: FloatingHoverTarget
}>()

/**
 * 卡片里最多逐条列几条任务，超出收进一行「另有 N 项」。
 *
 * 高度预算（窗口 340×400，见 `index.ts` 的 `tooltip.size`）：
 * 概览分组 4 行 + 任务 8 行 + 页脚 1 行 ≈ 290px，余量留给行距与被裁的极端情况。
 */
const MAX_SHOWN = 8

const todoStore = useTodoStore()
const modulesStore = useFloatingModulesStore()

const snapshot = computed(() => todoStore.snapshot)
/** 当前生效的清单筛选（信息卡要说明「这些是哪个范围里的任务」）。 */
const settings = computed(() => todoSettings(modulesStore.config))

/** 逐条展示的任务。 */
const shownTasks = computed(() => (snapshot.value?.list ?? []).slice(0, MAX_SHOWN))

/** 没铺开显示的剩余条数。 */
const moreCount = computed(() =>
  Math.max(0, (snapshot.value?.total_incomplete ?? 0) - shownTasks.value.length),
)

/** 逾期条数（参考时刻取快照构建时刻，保证整卡口径一致）。 */
const overdueCount = computed(() => {
  const data = snapshot.value
  if (!data) return 0
  return data.list.filter(task => isOverdue(task.due_at_ms, data.updated_at_ms, task.has_time)).length
})

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
</script>

<template>
  <template v-if="snapshot">
    <!-- 概览：范围 + 未完成 / 逾期计数 + 更新时间 -->
    <TooltipSection>
      <TooltipRow label="范围" :value="settings.project_name || '全部清单'" />
      <TooltipRow label="未完成" :value="`${snapshot.total_incomplete} 项`" mono />
      <TooltipRow
        label="已逾期"
        :value="`${overdueCount} 项`"
        :tone="overdueCount > 0 ? 'red' : 'muted'"
        mono
      />
      <TooltipRow label="更新时间" :value="formatClockTime(snapshot.updated_at_ms)" tone="muted" mono />
    </TooltipSection>

    <!-- 逐条未完成任务清单 -->
    <TooltipSection v-if="shownTasks.length > 0">
      <div class="todo-tooltip__list">
        <div v-for="task in shownTasks" :key="task.id" class="todo-tooltip__task">
          <span
            class="todo-tooltip__dot"
            :class="`todo-tooltip__dot--${toneOf(task)}`"
            aria-hidden="true"
          />
          <span class="todo-tooltip__title">{{ task.title }}</span>
          <span class="todo-tooltip__meta">
            <span v-if="priorityLabel(task.priority)" class="todo-tooltip__priority">
              {{ priorityLabel(task.priority) }}
            </span>
            <span
              v-if="dueText(task)"
              class="todo-tooltip__due"
              :class="`todo-tooltip__due--${toneOf(task)}`"
            >
              {{ dueText(task) }}
            </span>
          </span>
        </div>
      </div>

      <TooltipRow v-if="moreCount > 0" label="…" :value="`另有 ${moreCount} 项`" tone="muted" />
    </TooltipSection>

    <TooltipSection v-else>
      <TooltipRow label="状态" value="已清空，暂无未完成任务" tone="green" />
    </TooltipSection>
  </template>

  <!-- 未配置 Token / 数据未到：给出引导，避免弹空框 -->
  <template v-else>
    <TooltipRow label="状态" :value="todoStore.lastError || '正在获取待办…'" tone="muted" />
    <TooltipSection>
      <TooltipRow label="提示" value="请在主窗口「滴答待办」设置中填写个人 API Token" />
    </TooltipSection>
  </template>
</template>
