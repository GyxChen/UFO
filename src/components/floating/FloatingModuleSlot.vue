<script setup lang="ts">
import { computed, provide } from 'vue'
import { floatingModuleKey, type FloatingHoverTarget, type FloatingModuleApi } from '@/lib/floating-module-context'
import type { FloatingLayout } from '@/lib/floating-layout'
import type { FloatingModuleDefinition } from '@/lib/floating-modules'

/**
 * 单个模块的槽位：承载模块视图，并给它注入 {@link FloatingModuleApi}。
 *
 * 上报的悬停对象在这里补上模块 id 再转给宿主，因此模块组件不需要知道自己的 id；
 * 根节点带 `data-module-id`，外壳据此把「点在我这块区域」翻译成「打开这个组件的设置」。
 */
const props = defineProps<{
  /** 本槽位承载的模块定义（含视图组件） */
  definition: FloatingModuleDefinition
  /** 当前悬浮窗形态 */
  layoutMode: FloatingLayout
  /** 宿主分配给本模块的格子数 */
  allocated: number
}>()

const emit = defineEmits<{
  /** 模块上报的悬停目标 */
  hover: [target: FloatingHoverTarget | null]
}>()

/** 分给本模块的格子数，模块可据此决定网格列行。 */
const allocatedCells = computed(() => props.allocated)

provide(floatingModuleKey, {
  allocatedCells,
  reportHover: (key: string | null) => emit('hover', key === null ? null : { moduleId: props.definition.id, key }),
} satisfies FloatingModuleApi)
</script>

<template>
  <div class="floating-module-slot" :data-module-id="props.definition.id">
    <component :is="props.definition.component" :layout-mode="props.layoutMode" />
  </div>
</template>