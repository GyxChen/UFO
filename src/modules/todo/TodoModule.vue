<script setup lang="ts">
import { computed, inject } from 'vue'
import type { FloatingLayout } from '@/lib/floating-layout'
import { floatingModuleKey } from '@/lib/floating-module-context'
import { PRIORITY_BUCKETS, countByPriority, priorityBucketLabel, priorityTone } from './lib/todo-format'
import { useTodoStore } from './stores/todo'
import flagIcon from '@/assets/flag.svg'

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

/**
 * 悬停对象 key：整块待办只对应一份信息卡，用常量即可（外壳只当字符串转发）。
 */
const HOVER_KEY = 'todo-list'

/** 计数展示上限：超过就写 `99+`，避免三位数把一格宽的槽位撑破。 */
const COUNT_DISPLAY_MAX = 99

/**
 * 三面小旗子的字形（`src/assets/flag.svg`）。
 *
 * 图标本身不带颜色：形状以 `mask-image` 用，颜色由 CSS 的 `background-color` 给
 * （所以一面字形就能出红 / 黄 / 蓝三种旗子，见 `todo.css` 的 `.todo-module__flag`）。
 * 这里把 URL 声明成自定义属性交给样式表，避免在 CSS 里写资源路径 —— 打包后
 * 文件名带哈希，只有 `import` 拿得到最终地址。
 */
const flagMask = `url("${flagIcon}")`

const snapshot = computed(() => todoStore.snapshot)

/** 三档未完成数（无优先级归入「低」，三数之和 = 未完成总数）。 */
const counts = computed(() => countByPriority(snapshot.value?.list ?? []))

/**
 * 悬浮窗的三面小旗子：一档一面，颜色固定为高 = 红 / 中 = 黄 / 低 = 蓝。
 *
 * 数据未到时 `count` 是空串（只留灰色旗子）：写 `0` 会让人以为「真的没有任务」，
 * 而实际只是还没拉到数据 / 还没配 Token。
 */
const flags = computed(() => {
  const data = snapshot.value
  return PRIORITY_BUCKETS.map(bucket => ({
    bucket,
    label: priorityBucketLabel(bucket),
    tone: priorityTone(bucket),
    count: data ? formatCount(counts.value[bucket]) : '',
  }))
})

/**
 * 无障碍描述：悬浮窗里只有旗子与数字，读屏需要一句话说清三档各多少。
 *
 * @returns 如「高 2，中 1，低 3，共 6 个未完成任务」
 */
const ariaLabel = computed(() => {
  const data = snapshot.value
  if (!data) return '滴答待办（等待数据）'
  const parts = PRIORITY_BUCKETS.map(bucket => `${priorityBucketLabel(bucket)} ${counts.value[bucket]}`)
  return `${parts.join('，')}，共 ${data.total_incomplete} 个未完成任务`
})

/**
 * 旗子旁的计数文案。
 *
 * @param value 未完成条数
 * @returns 条数文本（超过 [`COUNT_DISPLAY_MAX`] 时写 `99+`）
 */
function formatCount(value: number): string {
  return value > COUNT_DISPLAY_MAX ? `${COUNT_DISPLAY_MAX}+` : String(value)
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
    :class="[`todo-module--${props.layoutMode}`, { 'todo-module--empty': !snapshot }]"
    role="region"
    :aria-label="ariaLabel"
    :style="{ '--todo-flag-mask': flagMask }"
    @pointerenter="handlePointerEnter"
    @pointerleave="handlePointerLeave"
  >
    <!--
      三面小旗子 = 高中低三档未完成数。方形 / 四角 / 横条里三组「旗在上、数量在下」横向排开，
      竖条里换成三行「旗在左、数量在右」上下叠放 —— 轨道方向由 CSS 按形态切换（见 todo.css）。
      数据未到时数量是空串，但仍占着网格单元 —— 三面灰旗的位置不会跳。
    -->
    <span class="todo-module__groups">
      <template v-for="flag in flags" :key="flag.bucket">
        <span class="todo-module__flag" :class="`todo-module__flag--${flag.tone}`" aria-hidden="true" />
        <span class="todo-module__count" aria-hidden="true">{{ flag.count }}</span>
      </template>
    </span>
  </div>
</template>
