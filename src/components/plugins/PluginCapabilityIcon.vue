<script setup lang="ts">
/**
 * 权限徽标的图标：按图标键画一枚 24×24 的单色描边图形。
 *
 * 它只管图形，不管徽标的文字与配色 —— 那两件事在管理区（`tone` 决定颜色档）。
 * 键名来自 Rust 注册表的 `icon` 字段，路径表见 `lib/plugin-icons.ts`（有护栏锁住两边一致）。
 *
 * ⚠️ `aria-hidden`：徽标旁边一定有文字（或 `title` 之外另有读屏文案），
 * 图标进了无障碍树只会让读屏多念一句没意义的东西。
 */
import { computed } from 'vue'
import { pluginIconPath } from '@/lib/plugin-icons'

const props = withDefaults(
  defineProps<{
    /** 图标键（`grid` / `card` / `globe` / …）。 */
    icon: string
    /** 边长（CSS 像素）；默认 13，与徽标文字 10.5–11px 的档位对齐。 */
    size?: number
  }>(),
  { size: 13 }
)

/** 该键的路径；未知键画不出来（空 `d`），但不影响相邻文字徽标可读。 */
const path = computed(() => pluginIconPath(props.icon))
</script>

<template>
  <svg
    :width="size"
    :height="size"
    viewBox="0 0 24 24"
    class="plugin-capability-icon"
    data-plugin-icon
    :data-icon="icon"
    aria-hidden="true"
    focusable="false"
    fill="none"
    stroke="currentColor"
    stroke-width="1.7"
    stroke-linecap="round"
    stroke-linejoin="round"
  >
    <path :d="path" />
  </svg>
</template>

<style scoped>
/* 图标跟随徽标自身的颜色档（父级给 `color`），因此这里不写任何颜色。 */
.plugin-capability-icon {
  display: block;
  flex: none;
}
</style>
