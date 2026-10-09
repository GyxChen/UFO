<script setup lang="ts">
/**
 * 「整窗编辑脚本」独立窗口：一块自己的窗，正文每输入一次就同步回发起方。
 *
 * 为什么不再是主窗里的弹层：弹层铺满的是主窗内容区，编辑时既看不见「运行测试 / 保存」那两枚
 * 按钮，也占不住第二块屏；独立窗口可以和主窗并排放，改完不必来回切。
 *
 * ⚠️ 窗口自己不存第二份真相：它是**隐藏而非销毁**的，上一次的正文会留在内存里，
 * 所以每次 open 载荷都必须整份换掉 `session`，不能只补其中几个字段。
 * ⚠️ 每次输入即时整份回传、不做防抖：延迟落地会让主窗的「保存」写出旧文本，那种错最难查。
 */
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import PanelWindow from '@/components/ui/PanelWindow.vue'
import ScriptEditorFrame from '@/components/ui/ScriptEditorFrame.vue'
import { listenPanelWindowOpen, onPanelWindowHidden } from '@/lib/panel-window'
import {
  closeScriptEditorWindow,
  isScriptEditorSession,
  syncScriptEditorTo,
  type ScriptEditorSession,
} from '@/lib/script-editor-window'

/** 当前这次编辑会话；主窗还没递进来时为 null（那一刻窗口还藏着）。 */
const session = ref<ScriptEditorSession | null>(null)
/** 编辑框实例：接手光标时用它的 `restoreCaret`。 */
const frame = ref<InstanceType<typeof ScriptEditorFrame> | null>(null)
/** 递进这一次的那扇窗（收起时要回报给它）。 */
let requester = ''
/** 这一次编辑是否还开着：`finish()` 靠它把重复的收起信号丢掉。 */
const active = ref(false)

let unlistenOpen: UnlistenFn | undefined
let stopHiddenWatch: (() => void) | undefined

/**
 * 收起：隐藏自己 + 回报发起方。
 *
 * ×、Esc、以及 Alt+F4 都走这里 —— Rust 侧把关窗统一改成隐藏（进程不退），
 * 于是键盘与系统关闭这两条只能靠 `onPanelWindowHidden` 那条兜底送进来。
 */
async function finish() {
  if (!active.value) return
  active.value = false
  await closeScriptEditorWindow(requester)
}

/** Esc 与标题栏的 × 是同一个出口。 */
function onKeydown(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  event.preventDefault()
  void finish()
}

/** 每次输入整份回传给发起方（发起方按令牌核对后才落进草稿）。 */
function onInput(script: string) {
  const current = session.value
  if (!current) return
  current.script = script
  void syncScriptEditorTo(requester, { token: current.token, script }).catch(() => {})
}

onMounted(async () => {
  window.addEventListener('keydown', onKeydown)
  stopHiddenWatch = onPanelWindowHidden(() => void finish())
  unlistenOpen = await listenPanelWindowOpen<ScriptEditorSession>(isScriptEditorSession, (data, from) => {
    requester = from
    active.value = true
    // 整份换掉：见文件头那条 —— 窗口不销毁，残留的上一份正文会顶掉这一次的草稿。
    session.value = { ...data }
    void nextTick(() => frame.value?.restoreCaret(data.caret))
  })
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  stopHiddenWatch?.()
  unlistenOpen?.()
})
</script>

<template>
  <PanelWindow
    :title="session?.title ?? '编辑脚本'"
    hint="改了脚本点右上角 × 返回，再运行测试或保存。"
    close-label="完成并返回"
    @close="finish"
  >
    <div class="jse-window__body">
      <ScriptEditorFrame
        v-if="session"
        ref="frame"
        fill
        :model-value="session.script"
        :aria-label="session.ariaLabel"
        :placeholder="session.placeholder"
        @update:model-value="onInput"
      />
      <p v-else class="jse-window__empty">正在等主窗把脚本递过来…</p>
    </div>
  </PanelWindow>
</template>
