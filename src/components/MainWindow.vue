<script setup lang="ts">
/**
 * 主窗口：两级导航 + 内容区。
 *
 * 一级 tab = 「悬浮窗设置」（悬浮窗整体：组件列表 + 三形态预览）+
 * 每个声明了设置页的组件各一页；二级 tab = 组件自己的设置分区
 * （行为 / 窗口格子 / 当前会话 / 信息卡拓展 / 会话记录）。
 *
 * ⚠️ 「悬浮窗设置」是**一页到底**，没有二级：它本身就是一块整体（列表 + 预览并排），
 * 再切一刀反而把「改组件」和「看效果」拆开了。二级只服务组件设置页。
 * 组件只给一个分区时同样不渲染二级（只有一个 tab 的 tabbar 是纯噪音）。
 *
 * 内容容器按分区的 `fill` 选：`fill: true`（列表 / 编辑器）用 `.main-window--wide` —— 外层不滚、
 * 内层各列占满高度各自滚；否则 `.main-window--stack` —— 内容有多高算多高，滚外层页面。
 *
 * 两级导航都是 `BaseTabs`，靠 variant 区分外观（一级下划线 / 二级胶囊）；
 * 页面清单完全由模块注册表推出，新增组件不用改这个文件。
 * 打开某组件设置有两条路：组件列表行上的齿轮，或直接点悬浮窗里的那个组件
 * （外壳发 `OPEN_MODULE_SETTINGS_EVENT` 过来）。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { FLOATING_MODULE_DEFINITIONS } from './floating/modules'
import FloatingModulesPanel from './floating/FloatingModulesPanel.vue'
import BaseTabs from './ui/BaseTabs.vue'
import { OPEN_MODULE_SETTINGS_EVENT, type OpenModuleSettingsPayload } from '@/lib/floating-bridge'
import { hasModuleSettings, type FloatingModuleSection } from '@/lib/floating-modules'
import { hideMainWindow, startCurrentWindowDrag } from '@/lib/window'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import closeIcon from '@/assets/close.svg'

/** 悬浮窗设置页的 tab key；用固定串而不是模块 id，免得将来某个组件真叫这个名字。 */
const MODULES_TAB = 'floating-window'

/** 一个一级页面：悬浮窗设置，或某个组件的设置页。 */
interface MainTab {
  /** tab 唯一 key（组件页 = 模块 id） */
  key: string
  /** tabbar 上的文字 */
  label: string
  /** 该页的二级分区；空数组 = 这页没有二级 tabbar */
  sections: FloatingModuleSection[]
}

const modules = useFloatingModulesStore()

/** 一级页面清单：悬浮窗设置 + 每个有设置页的组件一页（顺序 = 注册表顺序）。 */
const tabs = computed<MainTab[]>(() => [
  { key: MODULES_TAB, label: '悬浮窗设置', sections: [] },
  ...FLOATING_MODULE_DEFINITIONS.filter(hasModuleSettings).map(definition => ({
    key: definition.id,
    label: definition.label,
    sections: definition.settings?.sections ?? [],
  })),
])

const activeTab = ref<string>(MODULES_TAB)
/** 当前一级页的分区列表。 */
const activeSections = computed(() => tabs.value.find(tab => tab.key === activeTab.value)?.sections ?? [])
/** 当前二级分区的 id；空串 = 还没选（只有单分区的组件会出现）。 */
const activeSectionId = ref<string>('')
/** 当前该渲染的分区内容；这页没有分区时为 null。 */
const activeSection = computed(
  () =>
    activeSections.value.find(section => section.id === activeSectionId.value) ??
    activeSections.value[0] ??
    null
)
/** 一级 tab 的文字，用作组件页里那个（视觉隐藏的）h1。 */
const activeTabLabel = computed(() => tabs.value.find(tab => tab.key === activeTab.value)?.label ?? '')

// 换一级页就回到该页第一个分区：组件设置页每次都从默认分区开始，不会停在上次看的那一页。
watch(
  activeSections,
  sections => {
    activeSectionId.value = sections[0]?.id ?? ''
  },
  { immediate: true }
)

/**
 * 切到某个组件的设置页。
 * 组件没有设置页（或没传 id）时回到悬浮窗设置页 —— 悬浮窗上点空白处就是这个语义。
 */
function openModuleSettings(moduleId: string | null) {
  activeTab.value = tabs.value.some(tab => tab.key === moduleId) ? (moduleId as string) : MODULES_TAB
}

/** 自绘标题栏拖拽：左键按下后交给系统移动无边框窗口。 */
async function onTitlebarPointerDown(event: PointerEvent) {
  if (event.button !== 0) return
  // 标题栏按钮自身不启动拖拽。
  const target = event.target as HTMLElement | null
  if (target?.closest('button')) return
  try {
    await startCurrentWindowDrag()
  } catch (error) {
    console.error('failed to start dragging main window', error)
  }
}

/** 关闭按钮：隐藏到托盘，不退出进程。 */
async function onCloseMain() {
  try {
    await hideMainWindow()
  } catch (error) {
    console.error('failed to hide main window', error)
  }
}

let unlistenOpenSettings: UnlistenFn | undefined

onMounted(async () => {
  // 悬浮窗广播运行时快照（组件徽标 + 三形态预览），并主动要一次避免错过启动时的广播。
  void modules.startRuntimeBridge()
  try {
    unlistenOpenSettings = await listen<OpenModuleSettingsPayload>(OPEN_MODULE_SETTINGS_EVENT, event => {
      openModuleSettings(event.payload?.moduleId ?? null)
    })
  } catch (error) {
    console.warn('open-module-settings channel unavailable', error)
  }
})

onBeforeUnmount(() => {
  unlistenOpenSettings?.()
  unlistenOpenSettings = undefined
  modules.stopRuntimeBridge()
})
</script>

<template>
  <div class="main-shell">
    <header class="main-titlebar" @pointerdown="onTitlebarPointerDown">
      <span class="main-titlebar__title">UFO · 主窗口</span>
      <button
        type="button"
        class="main-titlebar__close"
        aria-label="隐藏主窗口"
        title="隐藏"
        @click.stop="onCloseMain"
        @pointerdown.stop
      >
        <img :src="closeIcon" alt="" class="main-titlebar__close-icon" aria-hidden="true" />
      </button>
    </header>

    <!-- 一级：悬浮窗设置 / 各组件设置页（下划线，见 .main-tabs 默认样式）。 -->
    <BaseTabs
      v-model="activeTab"
      id-prefix="main"
      aria-label="主窗口分区"
      :tabs="tabs.map(tab => ({ key: tab.key, label: tab.label }))"
    />

    <!-- 二级：当前组件自己的设置分区（胶囊，只在该组件给了多个分区时出现）。 -->
    <BaseTabs
      v-if="activeSections.length > 1"
      v-model="activeSectionId"
      variant="chips"
      id-prefix="module"
      :aria-label="`${activeTabLabel} 设置分区`"
      :tabs="activeSections.map(section => ({ key: section.id, label: section.label }))"
    />

    <main
      v-if="activeTab === MODULES_TAB"
      class="main-window main-window--wide"
      role="tabpanel"
      tabindex="0"
      :id="`main-panel-${MODULES_TAB}`"
      :aria-labelledby="`main-tab-${MODULES_TAB}`"
    >
      <FloatingModulesPanel @open-settings="openModuleSettings" />
    </main>

    <main
      v-else
      class="main-window"
      :class="activeSection?.fill ? 'main-window--wide' : 'main-window--stack'"
      role="tabpanel"
      tabindex="0"
      :id="`main-panel-${activeTab}`"
      :aria-labelledby="`main-tab-${activeTab}`"
    >
      <!-- 组件名只在无障碍树里出现：屏幕上的一级 tab + 二级 tab 已经说清是哪个组件的哪一块。 -->
      <h1 class="sr-only">{{ activeTabLabel }}</h1>
      <section
        v-if="activeSection"
        class="module-page"
        :class="{ 'module-page--fill': activeSection.fill }"
        role="tabpanel"
        :id="`module-panel-${activeSection.id}`"
        :aria-labelledby="`module-tab-${activeSection.id}`"
      >
        <component :is="activeSection.component" />
      </section>
    </main>
  </div>
</template>