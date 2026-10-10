<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { onBeforeUnmount, onMounted, watch } from 'vue'
import { FLOATING_MODULE_DEFINITIONS } from './modules'
import { PANEL_WINDOW_VIEWS } from './windows'
import { activateModules } from './lib/floating-modules'
import { syncPluginDefinitions } from '@/lib/plugin-loader'
import { startPluginDevReload } from '@/lib/plugin-dev-reload'
import { useFloatingModulesStore } from './stores/floatingModules'
import { usePluginsStore } from './stores/plugins'
import { useThemeStore } from './stores/theme'
import FloatingWindow from './components/floating/FloatingWindow.vue'
import MainWindow from './components/MainWindow.vue'
import TooltipWindow from './components/TooltipWindow.vue'

/**
 * 应用外壳：按窗口 label 分派窗口，并把所有窗口共用的东西拉起来。
 *
 * ⚠️ 这里**只认两样东西**：窗口 label 与注册表。任何具体组件的数据源
 * 都在组件自己的 `activate` 里启动，由 `activateModules` 统一调用 ——
 * 新增一个悬浮窗组件（内置的或组件包）不需要改这个文件。
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
// 组件包安装记录同样是窗口级数据：每个 webview 都要自己装载一遍产物（不共享 JS realm）。
const plugins = usePluginsStore()
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

/**
 * 已激活模块的清理函数，按 id 记账。
 *
 * 增量而不是「整表重跑」：装/卸一个包会换掉整份注册表，
 * 全量重跑会把内置模块的订阅也拆了重建（天气与待办的轮询当场抖一下，灯也可能闪）。
 */
const activated = new Map<string, () => void>()

/** 让「该活着的模块」真的活着：新出现的 activate，消失的 dispose。 */
function syncActivations() {
  // 面板窗一块模块格子都不挂：跟着激活就是白白多跑几路定时出网。
  if (panelWindowView) return
  const definitions = FLOATING_MODULE_DEFINITIONS.value
  const alive = new Set(definitions.map(definition => definition.id))
  for (const [id, dispose] of activated) {
    if (alive.has(id)) continue
    try {
      dispose()
    } catch (error) {
      console.warn(`failed to dispose floating module ${id}`, error)
    }
    activated.delete(id)
  }
  for (const definition of definitions) {
    if (activated.has(definition.id)) continue
    activated.set(definition.id, activateModules([definition]))
  }
}

let stopRecordWatch: (() => void) | undefined
let stopDefinitionWatch: (() => void) | undefined
let stopDevReload: (() => void) | undefined

/** 重算一次组件包定义（安装记录变更与开发期热重载都走这里，装载本身是异步的）。 */
function resyncPlugins() {
  return syncPluginDefinitions(plugins.records, {
    reportLoadMs: (id, loadMs) => plugins.reportLoad(id, loadMs),
  })
}

onMounted(() => {
  void floatingModules.start()
  // 主题：首帧已由 main.ts 的 hydrate() 落地，这里只挂上跟随其它窗口的广播。
  void theme.start()
  void plugins.start()
  // 安装记录一变就重算组件包定义（装载是异步的，完成后会换掉注册表里那半截）。
  stopRecordWatch = watch(() => plugins.records, () => void resyncPlugins(), { immediate: true })
  // 开发期热重载：作者 `yarn dev` 改完文件，靠指纹问到就重挂一次（读命令，每个窗口都能问）。
  stopDevReload = startPluginDevReload({
    hasDevPackages: () => plugins.records.some(record => record.devReload),
    reload: () => resyncPlugins(),
  })
  // 注册表一变更就补激活、清停掉：内置模块在这里只被 activate 一次。
  stopDefinitionWatch = watch(FLOATING_MODULE_DEFINITIONS, syncActivations, { immediate: true })
})

onBeforeUnmount(() => {
  stopRecordWatch?.()
  stopDefinitionWatch?.()
  stopDevReload?.()
  stopRecordWatch = undefined
  stopDefinitionWatch = undefined
  stopDevReload = undefined
  // 逆序停：后装的组件包可能依赖先起的内置数据源。
  for (const dispose of [...activated.values()].reverse()) {
    try {
      dispose()
    } catch (error) {
      console.warn('failed to dispose floating module', error)
    }
  }
  activated.clear()
  plugins.stop()
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
