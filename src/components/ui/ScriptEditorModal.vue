<script setup lang="ts">
/**
 * 脚本编辑器整窗弹层：负责铺满窗口的实色 Matte 面板、Esc 关闭和完成按钮。
 *
 * 它不保存第二份草稿：`modelValue` 始终来自父组件，里面的修改即时回传，避免内嵌框
 * 与弹窗各有一份文本后还要做合并。
 */
import { onBeforeUnmount, ref, watch } from 'vue'
import type { EditorCaret } from '@/lib/js-editor'
import BaseButton from './BaseButton.vue'
import ScriptEditorFrame from './ScriptEditorFrame.vue'

interface Props {
  /** 是否展开整窗编辑。 */
  open: boolean
  /** 当前脚本源码。 */
  modelValue: string
  /** 标题栏文案。 */
  title?: string
  /** textarea 的无障碍名称。 */
  ariaLabel?: string
  /** 空内容时的灰字提示。 */
  placeholder?: string
}

const props = withDefaults(defineProps<Props>(), {
  title: '编辑脚本',
  ariaLabel: '脚本源码',
  placeholder: '',
})

const emit = defineEmits<{
  /** 文本变化（v-model）。 */
  'update:modelValue': [value: string]
  /** 请求关闭弹层。 */
  close: []
}>()

/** 弹窗里的代码框实例，用于接手内嵌框的光标与滚动位置。 */
const frame = ref<InstanceType<typeof ScriptEditorFrame> | null>(null)

/** Esc 关闭弹层；键盘用户的另一个出口是标题栏「完成」。 */
function onKeydown(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  event.preventDefault()
  emit('close')
}

watch(
  () => props.open,
  value => {
    if (value) window.addEventListener('keydown', onKeydown)
    else window.removeEventListener('keydown', onKeydown)
  }
)

onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))

/** 聚焦弹窗代码框并恢复光标与滚动位置。 */
function restoreCaret(caret: EditorCaret) {
  frame.value?.restoreCaret(caret)
}

defineExpose({ restoreCaret })
</script>

<template>
  <!--
    宿主是主窗的浮层区 `#main-content`（标题栏以下那一整块），不是 `body`：
    挂在 body 上时铺的是视口 —— 既压住标题栏（拖窗 / 隐藏到托盘点不到），方角又伸出
    `.main-shell` 的圆角之外（弹层看起来比窗口还大）。与 `StatusLogOverlay` 同一口径。
    只有主窗用得到整窗编辑（`EndpointFlowEditor`），所以这个目标一定存在。
  -->
  <Teleport to="#main-content">
    <!--
      整窗弹窗：铺满窗口的实色哑光面板（与主窗同 token，不用毛玻璃）。
      @wheel.stop 拦住滚轮冒泡：否则光标停在标题栏上滚动时，滚的是背后的主窗内容。
    -->
    <div v-if="open" class="jse-modal" role="dialog" aria-modal="true" aria-label="整窗编辑脚本" @wheel.stop>
      <header class="jse-modal__bar">
        <span class="jse-modal__title">{{ title }}</span>
        <span class="jse-modal__hint">改了脚本点「完成」返回，再运行测试或保存。</span>
        <BaseButton variant="primary" @click="emit('close')">完成</BaseButton>
      </header>

      <div class="jse-modal__body">
        <ScriptEditorFrame
          ref="frame"
          fill
          :model-value="modelValue"
          :aria-label="ariaLabel"
          :placeholder="placeholder"
          @update:model-value="emit('update:modelValue', $event)"
        />
      </div>
    </div>
  </Teleport>
</template>
