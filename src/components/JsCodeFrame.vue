<script setup lang="ts">
/**
 * 轻量 JS 代码编辑框（高亮 + Tab 缩进），零依赖。
 *
 * 结构是业界最常见的「透明 textarea 叠在带高亮的 `<pre>` 上」两层：
 * textarea 负责输入 / 选区 / 光标 / 滚动，`<pre>` 只负责把同一份文本着色显示。
 * 之所以不引 CodeMirror / Monaco：那段脚本最多几十行，而它们会带进编辑器状态机、
 * 主题系统与几百 KB 体积，代价远大于「给 Keywords 上色」这个需求。
 *
 * ⚠️ 两层文本必须逐字符一致（`renderJsHighlight` 补的尾换行是刻意的，见其注释），
 * 且字体 / 字号 / 行高 / 内边距必须完全一致，否则光标与字符会错位。
 * ⚠️ 滚动同步用 `transform: translate()` 而不是 `scrollTop` 对齐：textarea 的滚动条会
 * 吃掉它的内容宽度，两者可滚到的最大值不同，横向滚到底会错开一个滚动条宽度。
 */
import { computed, nextTick, ref, watch } from 'vue'
import { renderJsHighlight } from '@/lib/js-highlight'
import { indentLines, outdentLines, type EditorCaret } from '@/lib/js-editor'

const props = withDefaults(
  defineProps<{
    /** 脚本源码（v-model） */
    modelValue: string
    /** 无障碍名称；textarea 没有可见 label，靠它说明用途 */
    ariaLabel?: string
    /** 空内容时的灰字提示 */
    placeholder?: string
    /** 是否在编辑框内部右上角显示「扩大」按钮 */
    expandable?: boolean
    /** 撑满父容器（整窗弹窗里用），默认是固定高度的小编辑框 */
    fill?: boolean
  }>(),
  { ariaLabel: '脚本源码', placeholder: '', expandable: false, fill: false },
)

const emit = defineEmits<{
  /** 文本变化（v-model） */
  'update:modelValue': [value: string]
  /** 点了右上角「扩大」按钮，由父组件决定弹窗形态 */
  expand: []
}>()

/** 输入层：真正的可编辑元素。 */
const inputEl = ref<HTMLTextAreaElement | null>(null)
/** 高亮层：只读展示，靠 transform 跟随输入层滚动。 */
const highlightEl = ref<HTMLElement | null>(null)

/** 着色后的 HTML；只在源码变化时重算。 */
const highlightHtml = computed(() => renderJsHighlight(props.modelValue))

/**
 * 让高亮层跟随输入层的滚动。
 * 用负向位移而非 scrollTop：两层可滚动的最大距离不同，位移量则永远一致。
 */
function syncScroll() {
  const input = inputEl.value
  const highlight = highlightEl.value
  if (!input || !highlight) return
  highlight.style.transform = `translate(${-input.scrollLeft}px, ${-input.scrollTop}px)`
}

/** 输入：把新文本抛给父组件，高亮层在下一帧随 `modelValue` 重算。 */
function onInput(event: Event) {
  emit('update:modelValue', (event.target as HTMLTextAreaElement).value)
}

/**
 * 接管 Tab / Shift+Tab：浏览器默认把 Tab 当「切换焦点」，写代码时不可用。
 * 缩进算法抽在 `lib/js-editor`（纯函数、有单测），这里只负责把结果写回并把光标摆正。
 */
function onKeydown(event: KeyboardEvent) {
  if (event.key !== 'Tab') return
  // 先挡掉默认行为：否则 Tab 会把焦点移出编辑框。
  event.preventDefault()

  const input = event.target as HTMLTextAreaElement
  const start = input.selectionStart ?? 0
  const end = input.selectionEnd ?? 0
  const edit = event.shiftKey
    ? outdentLines(input.value, start, end)
    : indentLines(input.value, start, end)

  if (edit.value === input.value) return
  emit('update:modelValue', edit.value)
  // 选区靠 DOM 属性设置，必须等 v-model 把新值渲染进 textarea 之后。
  void nextTick(() => {
    const el = inputEl.value
    if (!el) return
    el.selectionStart = edit.selectionStart
    el.selectionEnd = edit.selectionEnd
    syncScroll()
  })
}

// 外部改值（弹窗与内嵌框共用一份草稿、切换 endpoint 回填）后重算滚动位置：
// 值变短时 textarea 会把 scrollTop 夹回可见范围，高亮层要跟上，否则两层错位。
watch(
  () => props.modelValue,
  () => {
    void nextTick(syncScroll)
  },
)

/** 读当前光标与滚动位置（整窗弹窗要靠它接着编辑，否则另一个实例会停在文末）。 */
function readCaret(): EditorCaret {
  const input = inputEl.value
  if (!input) return { selectionStart: 0, selectionEnd: 0, scrollTop: 0, scrollLeft: 0 }
  return {
    selectionStart: input.selectionStart ?? 0,
    selectionEnd: input.selectionEnd ?? 0,
    scrollTop: input.scrollTop,
    scrollLeft: input.scrollLeft,
  }
}

/**
 * 聚焦并恢复光标与滚动位置。
 * ⚠️ 顺序不能换：先 focus、再设选区、最后设滚动 —— focus 会把光标滚进视口，先设滚动会被它覆盖。
 */
function restoreCaret(caret: EditorCaret) {
  const input = inputEl.value
  if (!input) return
  input.focus()
  input.selectionStart = caret.selectionStart
  input.selectionEnd = caret.selectionEnd
  input.scrollTop = caret.scrollTop
  input.scrollLeft = caret.scrollLeft
  // 滚动是手动改的，高亮层要跟着挪（浏览器也会补一次 scroll 事件，这里不依赖它）。
  syncScroll()
}

defineExpose({ readCaret, restoreCaret })
</script>

<template>
  <div class="jse" :class="{ 'jse--fill': fill }">
    <!-- 高亮层：aria-hidden，读屏与表单语义只认下面的 textarea，避免同一段文本被读两遍。 -->
    <pre
      ref="highlightEl"
      class="jse__highlight"
      aria-hidden="true"
      v-html="highlightHtml"
    ></pre>

    <!--
      输入层：文本透明（否则与高亮层叠字加粗），只显示光标与选区。
      wrap="off" 换行交给横向滚动，也让两层不会因软换行宽度不同而错位。
    -->
    <textarea
      ref="inputEl"
      class="jse__input"
      :value="modelValue"
      :aria-label="ariaLabel"
      :placeholder="placeholder"
      spellcheck="false"
      autocomplete="off"
      autocapitalize="off"
      autocorrect="off"
      wrap="off"
      @input="onInput"
      @keydown="onKeydown"
      @scroll="syncScroll"
    ></textarea>

    <!-- 扩大按钮：贴在编辑框内部右上角，位于两层之上，不被文本遮挡点击。 -->
    <button
      v-if="expandable"
      class="jse__expand"
      type="button"
      title="在整窗中编辑"
      aria-label="在整窗中编辑脚本"
      @click="emit('expand')"
    >
      <svg class="jse__expand-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="currentColor"
          d="M10,21V19H6.41L10.91,14.5L9.5,13.09L5,17.59V14H3V21H10M14.5,10.91L19,6.41V10H21V3H14V5H17.59L13.09,9.5L14.5,10.91Z"
        />
      </svg>
    </button>
  </div>
</template>