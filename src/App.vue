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

if (isFloatingWindow) {
  document.documentElement.classList.add('is-floating-window')
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
