<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { onBeforeUnmount, onMounted } from 'vue'
import { FLOATING_MODULE_DEFINITIONS } from './modules'
import { PANEL_WINDOW_VIEWS } from './windows'
import { activateModules } from './lib/floating-modules'
import { useFloatingModulesStore } from './stores/floatingModules'
import { useThemeStore } from './stores/theme'
import FloatingWindow from './components/floating/FloatingWindow.vue'
import MainWindow from './components/MainWindow.vue'
import TooltipWindow from './components/TooltipWindow.vue'

/**
 * 应用外壳：按窗口 label 分派窗口，并把所有窗口共用的东西拉起来。
 *
 * ⚠️ 这里**只认两样东西**：窗口 label 与注册表。任何具体组件的数据源
 * 都在组件自己的 `activate` 里启动，由 `activateModules` 统一调用 ——
 * 新增一个悬浮窗组件不需要改这个文件。
 * 主窗派出去的独立面板窗同理：它们登记在 `src/windows/index.ts`，
 * 这里只按 label 取那一格组件，不点名任何模块。
 */
const windowLabel = getCurrentWindow().label
const isFloatingWindow = windowLabel === 'floating'
const isTooltipWindow = windowLabel === 'tooltip'
/** 这块 label 是不是某个独立面板窗；null = 主窗（或未知窗口，退回主窗那套渲染）。 */
const panelWindowView = PANEL_WINDOW_VIEWS[windowLabel] ?? null
// 悬浮窗配置（组件开关 / 上限）是窗口级数据，所有窗口都读，属于外壳自己的 store。
const floatingModules = useFloatingModulesStore()
// 界面主题同样是窗口级数据：每个窗口都要跟同一份取值（见 stores/theme.ts）。
const theme = useThemeStore()

// 原生透明窗口要求 html/body/#app 也透明；按窗口类型打标记给 CSS 用。
if (isFloatingWindow) {
  document.documentElement.classList.add('is-floating-window')
} else if (isTooltipWindow) {
  document.documentElement.classList.add('is-tooltip-window')
} else if (panelWindowView) {
  // 独立面板窗与主窗同一套实色外壳与滚动口径，但标记单独一个：见 main.css 的 `.panel-shell`。
  document.documentElement.classList.add('is-panel-window')
} else {
  document.documentElement.classList.add('is-main-window')
}

/** 模块生命周期的清理函数（外壳卸载时调用）。 */
let stopModules: (() => void) | undefined

onMounted(() => {
  void floatingModules.start()
  // 主题：首帧已由 main.ts 的 hydrate() 落地，这里只挂上跟随其它窗口的广播。
  void theme.start()
  // 模块自己决定要订阅什么（提示窗可能要用其中某几路数据），外壳不参与。
  // ⚠️ 只有**挂模块视图**的窗口才激活数据源：悬浮窗 / 提示窗 / 主窗三个。主窗派出去的
  // 独立面板窗一块模块格子都没有，跟着激活就是白白多跑几路轮询（天气、待办都是定时出网）。
  if (!panelWindowView) stopModules = activateModules(FLOATING_MODULE_DEFINITIONS)
})

onBeforeUnmount(() => {
  stopModules?.()
  stopModules = undefined
  floatingModules.stop()
  theme.stop()
})
</script>

<template>
  <FloatingWindow v-if="isFloatingWindow" />
  <TooltipWindow v-else-if="isTooltipWindow" />
  <!-- 独立面板窗：这一格组件由 `src/windows/index.ts` 按 label 给，外壳不认识它是谁的窗口。 -->
  <component :is="panelWindowView" v-else-if="panelWindowView" />
  <MainWindow v-else />
</template>
