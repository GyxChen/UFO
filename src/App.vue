<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { onBeforeUnmount, onMounted } from 'vue'
import { useAgentStatusStore } from './stores/agentStatus'
import { useFloatingModulesStore } from './stores/floatingModules'
import FloatingWindow from './components/FloatingWindow.vue'
import MainWindow from './components/MainWindow.vue'

const isFloatingWindow = getCurrentWindow().label === 'floating'
const agentStatus = useAgentStatusStore()
const floatingModules = useFloatingModulesStore()

// 原生透明窗口要求 html/body/#app 也透明；按窗口类型打标记给 CSS 用。
if (isFloatingWindow) {
  document.documentElement.classList.add('is-floating-window')
} else {
  document.documentElement.classList.add('is-main-window')
}

onMounted(() => {
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
  <MainWindow v-else />
</template>
