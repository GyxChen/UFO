<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import FloatingWindow from './components/FloatingWindow.vue'
import MainWindow from './components/MainWindow.vue'

const isFloatingWindow = getCurrentWindow().label === 'floating'
</script>

<template>
  <FloatingWindow v-if="isFloatingWindow" />
  <MainWindow v-else />
</template>
