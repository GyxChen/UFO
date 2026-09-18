<script setup lang="ts">
/**
 * 脚本编辑器公开组件：内嵌代码框 + 可展开的整窗编辑弹层。
 *
 * 内外两层共用同一个 `v-model`，修改即时写回；打开弹窗时显式交接光标，避免 Vue
 * 重写 textarea 值后光标跳到文末。面板只绑定脚本草稿，不关心高亮或弹层细节。
 */
import { nextTick, ref } from 'vue'
import type { EditorCaret } from '@/lib/js-editor'
import ScriptEditorFrame from './ScriptEditorFrame.vue'
import ScriptEditorModal from './ScriptEditorModal.vue'

withDefaults(
  defineProps<{
    /** 脚本源码（v-model）。 */
    modelValue: string
    /** textarea 的无障碍名称。 */
    ariaLabel?: string
    /** 空内容时的灰字提示。 */
    placeholder?: string
    /** 弹窗标题栏文案。 */
    title?: string
  }>(),
  { ariaLabel: '脚本源码', placeholder: '', title: '编辑脚本' }
)

const emit = defineEmits<{
  /** 文本变化（v-model）。 */
  'update:modelValue': [value: string]
}>()

/** 内嵌代码框实例：打开弹窗前从它这里取光标位置。 */
const inlineFrame = ref<InstanceType<typeof ScriptEditorFrame> | null>(null)
/** 整窗弹层实例：打开后把光标交给弹窗里的代码框。 */
const modal = ref<InstanceType<typeof ScriptEditorModal> | null>(null)
/** 弹层是否展开。 */
const expanded = ref(false)
/** 打开前的焦点元素，关闭后归还。 */
let restoreFocus: HTMLElement | null = null

/** 兜底光标：内嵌框还没挂载时按文首起步。 */
const EMPTY_CARET: EditorCaret = { selectionStart: 0, selectionEnd: 0, scrollTop: 0, scrollLeft: 0 }

/** 打开整窗编辑并把当前光标交接给弹窗。 */
function open() {
  restoreFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
  const caret = inlineFrame.value?.readCaret() ?? EMPTY_CARET
  expanded.value = true
  void nextTick(() => modal.value?.restoreCaret(caret))
}

/** 关闭整窗编辑并归还焦点。 */
function close() {
  const target = restoreFocus
  restoreFocus = null
  expanded.value = false
  void nextTick(() => target?.focus())
}
</script>

<template>
  <ScriptEditorFrame
    ref="inlineFrame"
    :model-value="modelValue"
    :aria-label="ariaLabel"
    :placeholder="placeholder"
    expandable
    @update:model-value="emit('update:modelValue', $event)"
    @expand="open"
  />

  <ScriptEditorModal
    ref="modal"
    :open="expanded"
    :model-value="modelValue"
    :title="title"
    :aria-label="ariaLabel"
    :placeholder="placeholder"
    @update:model-value="emit('update:modelValue', $event)"
    @close="close"
  />
</template>
