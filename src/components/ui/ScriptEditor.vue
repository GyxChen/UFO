<script setup lang="ts">
/**
 * 脚本编辑器公开组件：内嵌代码框 + 「在独立窗口里编辑」的入口与来回同步。
 *
 * 独立窗口开着时内嵌框转**只读**：同一份草稿不能有两个写入方（两个窗口会互相顶掉光标与选区），
 * 所以写只在窗口那一侧，窗口每输入一次把整份正文回传，主窗的保存 / 运行测试因此一直拿到最新文本。
 *
 * 窗口是**按需创建、关窗即销毁**的，于是每次打开都新造一枚 `token`：回传只认当前这枚 ——
 * 调用方换了草稿（`scope` 一变）或切走那一页，窗口就此失联，迟到的正文不会被写进新草稿。
 * 本组件不认识 `scope` 是什么，调用方用它表达「这段脚本属于谁」，这里只做字符串比较。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { closePanelWindow, listenPanelWindowClosed } from '@/lib/panel-window'
import {
  SCRIPT_EDITOR_WINDOW_LABEL,
  listenScriptEditorSync,
  openScriptEditorWindow,
  type ScriptEditorSession,
} from '@/lib/script-editor-window'
import ScriptEditorFrame from './ScriptEditorFrame.vue'

const props = withDefaults(
  defineProps<{
    /** 脚本源码（v-model）。 */
    modelValue: string
    /** textarea 的无障碍名称。 */
    ariaLabel?: string
    /** 空内容时的灰字提示。 */
    placeholder?: string
    /** 独立窗口的标题栏文案。 */
    title?: string
    /** 这份草稿的身份（由调用方决定它是什么，例如「哪一份配置」的 id）；一变就收起窗口并作废回传。 */
    scope?: string
  }>(),
  { ariaLabel: '脚本源码', placeholder: '', title: '编辑脚本', scope: '' }
)

const emit = defineEmits<{
  /** 文本变化（v-model）：内嵌框的输入，或独立窗口回传的整份正文。 */
  'update:modelValue': [value: string]
}>()

/** 内嵌代码框实例：开窗前从它这里取光标位置。 */
const inlineFrame = ref<InstanceType<typeof ScriptEditorFrame> | null>(null)

/**
 * 当前这次编辑的令牌；空串 = 窗口没开着。
 *
 * 窗口的「没了」由宿主在销毁时回报（× / Esc / Alt+F4 三条路都算），所以这里跟着回报清空，
 * 它就是可靠的判据；`token` 非空时重复点按钮只会把已有的那扇窗唤到前台并换上这一份草稿。
 */
const token = ref('')
/** 编辑框下面那行常驻说明：正在窗口里编辑 / 开窗失败，都写这里（本项目禁止 alert/confirm）。 */
const note = ref('')
/** 令牌序号：每次打开换一枚，与 `scope` 拼成最终令牌。 */
let openSeq = 0

/** 内嵌框是否因为「窗口在写」而转只读。 */
const editingInWindow = computed(() => token.value !== '')

/** 收起：只清主窗这一侧的状态，不动窗口里的正文（正文早就逐条同步过来了）。 */
function collapse() {
  token.value = ''
  note.value = ''
}

/** 打开（或唤起）独立窗口，并把当前草稿与光标递进去。 */
async function open() {
  openSeq += 1
  const next = `${props.scope || 'script'}#${openSeq}`
  const caret = inlineFrame.value?.readCaret() ?? {
    selectionStart: 0,
    selectionEnd: 0,
    scrollTop: 0,
    scrollLeft: 0,
  }
  const session: ScriptEditorSession = {
    token: next,
    title: props.title,
    script: props.modelValue,
    placeholder: props.placeholder,
    ariaLabel: props.ariaLabel,
    caret,
  }
  token.value = next
  note.value = '正在独立窗口中编辑，这里先转只读；改完点窗口右上角的 × 回来保存。'
  try {
    // 建窗失败一律抛出（label 非法、同时在开的面板窗超额）：界面要说清，不能安静地什么都不发生。
    // 返回 false 只是「窗口已经开着，唤到前台」，那算成功。
    await openScriptEditorWindow(session)
  } catch (error) {
    collapse()
    note.value = `打不开独立窗口：${error}`
    console.warn('failed to open the script editor window', error)
  }
}

let unlistenSync: UnlistenFn | undefined
let unlistenClosed: UnlistenFn | undefined

onMounted(async () => {
  try {
    unlistenSync = await listenScriptEditorSync(value => {
      // 只认当前这枚令牌：切了草稿或已收起之后，迟到的正文不许写进现在这份 modelValue。
      if (value.token !== token.value) return
      emit('update:modelValue', value.script)
    })
    unlistenClosed = await listenPanelWindowClosed(SCRIPT_EDITOR_WINDOW_LABEL, collapse)
  } catch (error) {
    // 事件通道起不来就根本开不了窗：把内嵌框留在可编辑状态，界面照常能用。
    console.warn('script editor window channel unavailable', error)
  }
})

onBeforeUnmount(() => {
  unlistenSync?.()
  unlistenClosed?.()
  // 这一页要没了，没人再接收同步：把窗口一起关掉（销毁，连带释放它那个 renderer），
  // 别留一块写着草稿的窗在桌面上。
  if (token.value) void closePanelWindow(SCRIPT_EDITOR_WINDOW_LABEL)
})

// 换草稿（`scope` 一变）：窗口对应的是上一份，收起 + 作废令牌。
watch(
  () => props.scope,
  () => {
    if (!token.value) return
    collapse()
    void closePanelWindow(SCRIPT_EDITOR_WINDOW_LABEL)
  }
)
</script>

<template>
  <ScriptEditorFrame
    ref="inlineFrame"
    :model-value="modelValue"
    :aria-label="ariaLabel"
    :placeholder="placeholder"
    :readonly="editingInWindow"
    expandable
    @update:model-value="emit('update:modelValue', $event)"
    @expand="open"
  />

  <!-- 常驻读数：这一份草稿正在哪一侧写、为什么这里点不动，一眼看得见（不藏进 hover / title）。 -->
  <p v-if="note" class="jse__editing" role="status">{{ note }}</p>
</template>
