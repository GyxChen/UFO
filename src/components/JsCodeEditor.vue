<script setup lang="ts">
/**
 * 脚本编辑器：内嵌编辑框 + 点「扩大」后的整窗编辑弹窗。
 *
 * 两层用的是同一个 `v-model`（也就是草稿本身），所以弹窗里的修改**即时**写回，
 * 关闭后直接运行测试 / 保存即可 —— 不存在「弹窗副本」与「面板副本」两份真相，
 * 也就没有合并时机的问题。
 *
 * ⚠️ 弹窗 Teleport 到 body 并铺满整个窗口（`position: fixed; inset: 0`）：
 * 面板本身在主窗滚动区里，留在原地做遮罩会被滚动与圆角裁切影响。
 * ⚠️ 弹窗内的 Tab 由 `JsCodeFrame` 接管（缩进），因此键盘用户的出口是 Esc / 「完成」。
 */
import { onBeforeUnmount, nextTick, ref, watch } from 'vue'
import type { EditorCaret } from '@/lib/js-editor'
import JsCodeFrame from './JsCodeFrame.vue'

// 不在 script 里读 props（模板里直接用名字），因此不接返回值，避免落一个死变量。
withDefaults(
  defineProps<{
    /** 脚本源码（v-model，与内嵌编辑框共用同一份草稿） */
    modelValue: string
    /** 无障碍名称；透传给 textarea */
    ariaLabel?: string
    /** 空内容时的灰字提示 */
    placeholder?: string
    /** 弹窗标题栏文案（一般是「编辑脚本 · <endpoint>」） */
    title?: string
  }>(),
  { ariaLabel: '脚本源码', placeholder: '', title: '编辑脚本' },
)

const emit = defineEmits<{
  /** 文本变化（v-model） */
  'update:modelValue': [value: string]
}>()

/** 弹窗是否展开。 */
const expanded = ref(false)
/** 内嵌编辑框实例：打开弹窗前从它这里取光标位置。 */
const inlineFrame = ref<InstanceType<typeof JsCodeFrame> | null>(null)
/** 弹窗里的编辑框实例：拿到光标后由它接着编辑。 */
const modalFrame = ref<InstanceType<typeof JsCodeFrame> | null>(null)
/** 打开前的焦点元素（那个扩大按钮），关闭后还回去。 */
let restoreFocus: HTMLElement | null = null

/** 兜底光标：内嵌框还没挂载时按「文首」起步。 */
const EMPTY_CARET: EditorCaret = { selectionStart: 0, selectionEnd: 0, scrollTop: 0, scrollLeft: 0 }

/**
 * 打开整窗编辑。
 *
 * ⚠️ 光标必须显式交接：弹窗里的编辑框是另一个 DOM 实例，Vue 写 value 后浏览器的光标停在
 * 文末，不灌回来的话「点扩大」会莫名跳到脚本结尾。
 */
function open() {
  restoreFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
  const caret = inlineFrame.value?.readCaret() ?? EMPTY_CARET
  expanded.value = true
  void nextTick(() => modalFrame.value?.restoreCaret(caret))
}

/** 关闭整窗编辑：内容早已写回草稿，这里只需收起弹窗并归还焦点。 */
function close() {
  // 先取出焦点目标：nextTick 回调运行时 restoreFocus 已被清空，闭包里拿到的会是 null。
  const target = restoreFocus
  restoreFocus = null
  expanded.value = false
  // 归还焦点：整窗编辑只是临时放大，关掉后不该把键盘用户丢回文档开头。
  void nextTick(() => target?.focus())
}

/** Esc 关闭弹窗。 */
function onKeydown(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  event.preventDefault()
  close()
}

// 只在弹窗打开期间监听 Esc：常驻全局监听会与其它面板的快捷键互相干扰。
watch(expanded, (value) => {
  if (value) window.addEventListener('keydown', onKeydown)
  else window.removeEventListener('keydown', onKeydown)
})

// 组件卸载（切页 / 切 endpoint）时若弹窗还开着，必须摘掉监听。
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <!-- 内嵌编辑框：右上角的扩大按钮由它渲染，点击后交给这里弹窗。 -->
  <JsCodeFrame
    ref="inlineFrame"
    :model-value="modelValue"
    :aria-label="ariaLabel"
    :placeholder="placeholder"
    expandable
    @update:model-value="emit('update:modelValue', $event)"
    @expand="open"
  />

  <Teleport to="body">
    <!--
      整窗弹窗：铺满窗口的实色哑光面板（与主窗同 token，不用毛玻璃）。
      @wheel.stop 拦住滚轮冒泡：否则光标停在标题栏上滚动时，滚的是背后的主窗内容。
    -->
    <div
      v-if="expanded"
      class="jse-modal"
      role="dialog"
      aria-modal="true"
      aria-label="整窗编辑脚本"
      @wheel.stop
    >
      <header class="jse-modal__bar">
        <span class="jse-modal__title">{{ title }}</span>
        <span class="jse-modal__hint">改了脚本点「完成」返回，再运行测试或保存。</span>
        <button class="ufo-btn ufo-btn--primary" type="button" @click="close">完成</button>
      </header>

      <div class="jse-modal__body">
        <JsCodeFrame
          ref="modalFrame"
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