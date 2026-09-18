<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { onBeforeUnmount, onMounted } from 'vue'
import { FLOATING_MODULE_DEFINITIONS } from './modules'
import { activateModules } from './lib/floating-modules'
import { useFloatingModulesStore } from './stores/floatingModules'
import FloatingWindow from './components/floating/FloatingWindow.vue'
import MainWindow from './components/MainWindow.vue'
import TooltipWindow from './components/TooltipWindow.vue'

/**
 * 应用外壳：按窗口 label 分派三个窗口，并把三个窗口共用的东西拉起来。
 *
 * ⚠️ 这里**只认两样东西**：窗口 label 与模块注册表。任何具体组件的数据源
 * 都在组件自己的 `activate` 里启动，由 `activateModules` 统一调用 ——
 * 新增一个悬浮窗组件不需要改这个文件。
 */
const windowLabel = getCurrentWindow().label
const isFloatingWindow = windowLabel === 'floating'
const isTooltipWindow = windowLabel === 'tooltip'
// 悬浮窗配置（组件开关 / 上限）是窗口级数据，三个窗口都读，属于外壳自己的 store。
const floatingModules = useFloatingModulesStore()

// 原生透明窗口要求 html/body/#app 也透明；按窗口类型打标记给 CSS 用。
if (isFloatingWindow) {
  document.documentElement.classList.add('is-floating-window')
} else if (isTooltipWindow) {
  document.documentElement.classList.add('is-tooltip-window')
} else {
  document.documentElement.classList.add('is-main-window')
}

/** 模块生命周期的清理函数（外壳卸载时调用）。 */
let stopModules: (() => void) | undefined

onMounted(() => {
  void floatingModules.start()
  // 模块自己决定要订阅什么（提示窗可能要用其中某几路数据），外壳不参与。
  stopModules = activateModules(FLOATING_MODULE_DEFINITIONS)
})

onBeforeUnmount(() => {
  stopModules?.()
  stopModules = undefined
  floatingModules.stop()
})
</script>

<template>
  <FloatingWindow v-if="isFloatingWindow" />
  <TooltipWindow v-else-if="isTooltipWindow" />
  <MainWindow v-else />
</template>