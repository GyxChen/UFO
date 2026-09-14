<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { onBeforeUnmount, onMounted } from 'vue'
import { useAgentStatusStore } from './stores/agentStatus'
import { useFloatingModulesStore } from './stores/floatingModules'
import FloatingWindow from './components/FloatingWindow.vue'
import MainWindow from './components/MainWindow.vue'
import TooltipWindow from './components/TooltipWindow.vue'

const windowLabel = getCurrentWindow().label
const isFloatingWindow = windowLabel === 'floating'
const isTooltipWindow = windowLabel === 'tooltip'
const agentStatus = useAgentStatusStore()
const floatingModules = useFloatingModulesStore()

// 原生透明窗口要求 html/body/#app 也透明；按窗口类型打标记给 CSS 用。
if (isFloatingWindow) {
  document.documentElement.classList.add('is-floating-window')
} else if (isTooltipWindow) {
  document.documentElement.classList.add('is-tooltip-window')
} else {
  document.documentElement.classList.add('is-main-window')
}

onMounted(() => {
  // 提示窗既要用状态快照（会话数据），也要用模块配置（token 展示单位），两者都必须订阅，
  // 否则主窗口切换单位后提示窗仍停留在默认值。
  void agentStatus.start()
  void floatingModules.start()
})

onBeforeUnmount(() => {
  agentStatus.stop()
  floatingModules.stop()
})
</script>

<template>
  <FloatingWindow v-if="isFloatingWindow" />
  <TooltipWindow v-else-if="isTooltipWindow" />
  <MainWindow v-else />
</template>
