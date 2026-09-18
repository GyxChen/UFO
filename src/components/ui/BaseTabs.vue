<script setup lang="ts">
/**
 * 通用 tab 条：主窗口的一级（下划线）与二级（胶囊）共用同一套键盘导航与 ARIA 关联。
 *
 * 两种外观刻意做得不一样（`.main-tabs--chips`）：两级 tabbar 上下相邻，
 * 长得一样会让人分不清哪条是一级。
 *
 * id 约定（`aria-controls` 要指得到面板）：按钮 = `${idPrefix}-tab-${key}`，
 * 面板 = `${idPrefix}-panel-${key}`，调用方按同一规则给面板加 id。
 */
import { nextTick, ref } from 'vue'

withDefaults(
  defineProps<{
    /** tab 清单；key 既是选中值，也是面板 id 的后缀 */
    tabs: { key: string; label: string }[]
    /** 当前选中的 key（v-model） */
    modelValue: string
    /** 外观：`underline` = 一级下划线，`chips` = 二级胶囊 */
    variant?: 'underline' | 'chips'
    /** tablist 的无障碍名称 */
    ariaLabel?: string
    /** id 前缀，须与调用方给面板加的 id 规则一致 */
    idPrefix: string
  }>(),
  { variant: 'underline', ariaLabel: undefined }
)

const emit = defineEmits<{
  /** 选中项变化（v-model 写回） */
  'update:modelValue': [key: string]
}>()

/** tab 按钮元素，用于方向键切换后把焦点移到新选中项（roving tabindex）。 */
const tabRefs = ref<HTMLButtonElement[]>([])

function setTabRef(el: unknown, index: number) {
  if (el) tabRefs.value[index] = el as HTMLButtonElement
}

/** 选中某个 tab；点击与键盘共用，选中同一项时不重复 emit。 */
function select(key: string, current: string) {
  if (key !== current) emit('update:modelValue', key)
}

/**
 * tablist 键盘导航：←/→ 在 tab 间循环，Home/End 跳到首尾。
 * 只有选中项能被 Tab 到达，因此方向键是唯一的横向移动方式（WAI-ARIA Tabs 模式）。
 */
function onKeydown(event: KeyboardEvent, index: number, tabs: { key: string }[]) {
  let next: number | null = null
  if (event.key === 'ArrowRight') next = (index + 1) % tabs.length
  else if (event.key === 'ArrowLeft') next = (index - 1 + tabs.length) % tabs.length
  else if (event.key === 'Home') next = 0
  else if (event.key === 'End') next = tabs.length - 1
  if (next === null) return
  event.preventDefault()
  emit('update:modelValue', tabs[next].key)
  void nextTick(() => tabRefs.value[next as number]?.focus())
}
</script>

<template>
  <nav
    class="main-tabs"
    :class="{ 'main-tabs--chips': variant === 'chips' }"
    role="tablist"
    :aria-label="ariaLabel"
  >
    <button
      v-for="(tab, index) in tabs"
      :key="tab.key"
      :ref="el => setTabRef(el, index)"
      type="button"
      role="tab"
      class="main-tabs__tab"
      :class="{ 'main-tabs__tab--active': modelValue === tab.key }"
      :aria-selected="modelValue === tab.key"
      :aria-controls="`${idPrefix}-panel-${tab.key}`"
      :id="`${idPrefix}-tab-${tab.key}`"
      :tabindex="modelValue === tab.key ? 0 : -1"
      @click="select(tab.key, modelValue)"
      @keydown="onKeydown($event, index, tabs)"
    >
      {{ tab.label }}
    </button>
  </nav>
</template>