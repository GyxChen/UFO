<script setup lang="ts">
/**
 * 「转灯日志」独立窗口：主窗「记录详情」里那条折叠条目看不下的完整链路，在这一扇窗里看。
 *
 * 为什么不再是主窗里的一层浮层：两栏（流水 + 详情）挤在 1000×750 的内容区里永远读不完整，
 * 而浮层一开就把两级标签页全盖住。独立窗口能拉大、能挪到第二块屏，主窗那侧还能同时开着。
 *
 * ⚠️ 窗口是**隐藏而非销毁**的，于是两条约定必须守住：
 *   - 每次 open 整份换掉 `view`，正文组件靠 `v-if` 重挂 —— 过滤条件与选中不跨次残留；
 *   - 收起必须 `log.stop()`：`set_status_log_view` 按窗口 label 记账，漏掉这一步就等于
 *     「人早走了，monitor 还在每轮广播」（那批 IPC 的意义就是没人看时为零）。
 */
import { onBeforeUnmount, onMounted, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import PanelWindow from '@/components/ui/PanelWindow.vue'
import { onPanelWindowHidden } from '@/lib/panel-window'
import { useStatusLogStore } from '../stores/statusLog'
import { closeStatusLogWindow, listenStatusLogOpen, type StatusLogWindowView } from '../lib/status-log-window'
import StatusLogPanel from './StatusLogPanel.vue'

/** 要看的那一条会话；null = 主窗还没递过来（那一刻窗口还藏着）。 */
const view = ref<StatusLogWindowView | null>(null)
const log = useStatusLogStore()

/** 递进这一次的那扇窗（收起时回报给它）。 */
let requester = ''
/** 这一次查看是否还开着：`finish()` 靠它把重复的收起信号丢掉。 */
const active = ref(false)

let unlistenOpen: UnlistenFn | undefined
let stopHiddenWatch: (() => void) | undefined

/**
 * 收起：停掉实时订阅、卸掉正文，然后隐藏自己并回报发起方。
 *
 * ×、Esc、以及 Alt+F4 都走这里 —— Rust 侧把关窗统一改成隐藏（进程不退），
 * 于是键盘与系统关闭这两条只能靠 `onPanelWindowHidden` 那条兜底送进来。
 */
async function finish() {
  if (!active.value) return
  active.value = false
  log.stop()
  view.value = null
  await closeStatusLogWindow(requester)
}

/** Esc 与标题栏的 × 是同一个出口。 */
function onKeydown(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  event.preventDefault()
  void finish()
}

onMounted(async () => {
  window.addEventListener('keydown', onKeydown)
  stopHiddenWatch = onPanelWindowHidden(() => void finish())
  unlistenOpen = await listenStatusLogOpen(async (data, from) => {
    requester = from
    active.value = true
    // ⚠️ 先 attach 再挂正文：`start()` 的首拉与正文那次按会话过滤的 reload 都是「整份 replace」，
    // 并发下去谁后到谁说话，列表可能停在没过滤的那一页上。
    await log.start()
    view.value = { ...data }
  })
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  stopHiddenWatch?.()
  unlistenOpen?.()
  log.stop()
})
</script>

<template>
  <PanelWindow
    :title="view ? `转灯日志 · ${view.sessionKey}` : '转灯日志'"
    hint="左列本会话的裁决流水（新→旧），点一行在右列看它的全部字段"
    close-label="关闭转灯日志"
    @close="finish"
  >
    <StatusLogPanel v-if="view" :session-key="view.sessionKey" />
    <p v-else class="lg-window__empty">正在等主窗选好要看的那条会话…</p>
  </PanelWindow>
</template>
