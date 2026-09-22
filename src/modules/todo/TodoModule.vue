<script setup lang="ts">
import { computed, inject } from 'vue'
import type { FloatingLayout } from '@/lib/floating-layout'
import { floatingModuleKey } from '@/lib/floating-module-context'
import { PRIORITY_BUCKETS, countByPriority, priorityBucketLabel, priorityTone } from './lib/todo-format'
import { useTodoStore } from './stores/todo'

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

/** 计数展示上限：超过就写 `99+`。 */
const COUNT_DISPLAY_MAX = 99

const snapshot = computed(() => todoStore.snapshot)

/** 三档未完成数（无优先级归入「低」，三数之和 = 未完成总数）。 */
const counts = computed(() => countByPriority(snapshot.value?.list ?? []))

/**
 * 悬浮窗里的三组「数字 + 色条」：一组一档，颜色固定为高 = 红 / 中 = 黄 / 低 = 蓝。
 *
 * 数字是主角（这组数据的全部信息就在这里），色条只是「哪一档」的锚点 ——
 * 优先级不再靠旗子字形承载：45px 方格里只有约 39px 可用，9px 的旗形既认不出形状、
 * 又和数字抢那点宽度。位置（高 → 中 → 低）与色条颜色是两条通道，不只靠颜色分辨。
 *
 * 数据未到时 `count` 是空串（只留灰色色条）：写 `0` 会让人以为「真的没有任务」，
 * 而实际只是还没拉到数据 / 还没配 Token。
 */
const groups = computed(() => {
  const data = snapshot.value
  return PRIORITY_BUCKETS.map(bucket => ({
    bucket,
    label: priorityBucketLabel(bucket),
    tone: priorityTone(bucket),
    count: data ? formatCount(counts.value[bucket]) : '',
  }))
})

/**
 * 三组共用的字号档：按**最长的那个计数**取（两位 / 三位各降一档）。
 *
 * 取最长而不是「谁长谁缩」：三组数字大小必须一致，一个三位数让那一组单独变小，
 * 一眼就看出参差。窄槽位（未贴边的方形只有约 39px 宽）下三位数按原字号会溢出，
 * 整体降一档才既放得下、又不截断（截断会把 `128` 显示成 `12`，等于报错数）。
 * 具体到几位数由这里决定，缩多少写在样式表的 `--todo-count-step` 上。
 */
const sizeStep = computed(() => {
  const longest = groups.value.reduce((max, group) => Math.max(max, group.count.length), 0)
  if (longest >= 3) return 'todo-module__groups--xwide'
  if (longest === 2) return 'todo-module__groups--wide'
  return ''
})

/**
 * 无障碍描述：悬浮窗里只有数字与色条，读屏需要一句话说清三档各多少。
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
 * 计数的展示文案。
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
    @pointerenter="handlePointerEnter"
    @pointerleave="handlePointerLeave"
  >
    <!--
      三组「数字（上 / 右）+ 色条（下 / 左）」= 高中低三档未完成数。
      方形 / 四角 / 横条里三组**横排**、组内竖着放（数字在上）；竖条里三组**竖排**、组内横着放
      （色条在左）—— 轨道方向由 CSS 按形态切换（见 todo.css），DOM 只有一份。
      数据未到时数字是空串、色条转灰，但三组的位置与大小不变（格子不会跳）。
    -->
    <span class="todo-module__groups" :class="sizeStep">
      <span
        v-for="group in groups"
        :key="group.bucket"
        class="todo-module__cell"
        :class="`todo-module__cell--${group.tone}`"
      >
        <span class="todo-module__count" aria-hidden="true">{{ group.count }}</span>
        <i class="todo-module__tone" aria-hidden="true"></i>
      </span>
    </span>
  </div>
</template>
