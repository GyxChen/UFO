<script setup lang="ts">
/**
 * 「整窗编辑脚本」独立窗口：一块自己的窗，正文每输入一次就同步回发起方。
 *
 * 为什么不再是主窗里的弹层：弹层铺满的是主窗内容区，编辑时既看不见「运行测试 / 保存」那两枚
 * 按钮，也占不住第二块屏；独立窗口可以和主窗并排放，改完不必来回切。
 *
 * ⚠️ 这扇窗**按需创建、关窗即销毁**（不占常驻 renderer 进程），但同一 label 会复用：
 * 二次 open 只是把它唤到前台，所以每次 open 载荷都必须整份换掉 `session`，不能只补几个字段。
 * ⚠️ 每次输入即时整份回传、不做防抖：延迟落地会让主窗的「保存」写出旧文本，那种错最难查。
 */
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import PanelWindow from '@/components/ui/PanelWindow.vue'
import ScriptEditorFrame from '@/components/ui/ScriptEditorFrame.vue'
import {
  closeScriptEditorWindow,
  syncScriptEditorTo,
  useScriptEditorOpen,
  type ScriptEditorSession,
} from '@/lib/script-editor-window'

/** 当前这次编辑会话；主窗还没递进来时为 null（新建那一瞬的空窗）。 */
const session = ref<ScriptEditorSession | null>(null)
/** 编辑框实例：接手光标时用它的 `restoreCaret`。 */
const frame = ref<InstanceType<typeof ScriptEditorFrame> | null>(null)
/** 递进这一次的那扇窗（正文要回传给谁）。 */
let requester = ''
/** 这一次编辑是否还开着：`finish()` 靠它把重复的收起信号丢掉。 */
const active = ref(false)

let unlistenOpen: UnlistenFn | undefined

/**
 * 收起：销毁自己。
 *
 * × 与 Esc 都走这里；Alt+F4 由系统直接销毁 —— 三条路的「我关了」都由宿主在 `Destroyed`
 * 时代为回报发起方，窗口侧不必也来不及自己发那条消息。
 */
async function finish() {
  if (!active.value) return
  active.value = false
  await closeScriptEditorWindow()
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
  unlistenOpen = await useScriptEditorOpen((data, from) => {
    requester = from
    active.value = true
    // 整份换掉：见文件头那条 —— 窗口复用，残留的上一份正文会顶掉这一次的草稿。
    session.value = { ...data }
    void nextTick(() => frame.value?.restoreCaret(data.caret))
  })
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
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
