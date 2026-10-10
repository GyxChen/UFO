<script setup lang="ts">
/**
 * 主窗口：两级导航 + 内容区。
 *
 * 一级 tab = 每个声明了设置页的组件各一页 + 右端的「悬浮窗设置」（齿轮图标页签，
 * 一页到底：组件列表 / 格子大小 / 三形态预览 / 版本）；
 * 二级 tab = 该组件自己的设置分区（切法由组件定义，外壳不参与）。
 *
 * ⚠️ 组件设置页只有一个分区时不渲染二级（只有一个 tab 的 tabbar 是纯噪音）。
 *
 * 内容容器按分区的 `fill` 选：`fill: true`（列表 / 编辑器）用 `.main-window--wide` —— 外层不滚、
 * 内层各列占满高度各自滚；否则 `.main-window--stack` —— 内容有多高算多高，滚外层页面。
 *
 * 两级导航都是 `BaseTabs`，靠 variant 区分外观（一级下划线 / 二级胶囊）；
 * 页面清单完全由模块注册表推出，新增组件不用改这个文件。
 * 打开某组件设置有两条路：悬浮窗设置页里组件列表行的齿轮，或直接点悬浮窗里的那个组件
 * （外壳发 `OPEN_MODULE_SETTINGS_EVENT` 过来）；两条都落到 `openModuleSettings`。
 *
 * 一级 tabbar 右端另有两格，**语义不同、所在容器也不同**：
 * - 「悬浮窗设置」（齿轮图标）= 一个**标签页**，进 tablist，因此参与方向键漫游；
 * - 主题切换 = 一个**动作**（切深浅色，没有对应页面），走 `BaseTabs` 的 `#actions` 插槽，
 *   刻意留在 tablist 之外。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { FLOATING_MODULE_DEFINITIONS } from '@/modules'
import FloatingModulesPanel from './floating/FloatingModulesPanel.vue'
import BaseTabs from './ui/BaseTabs.vue'
import { OPEN_MODULE_SETTINGS_EVENT, type OpenModuleSettingsPayload } from '@/lib/floating-bridge'
import { hasModuleSettings, type FloatingModuleSection } from '@/lib/floating-modules'
import { hideMainWindow, startCurrentWindowDrag } from '@/lib/window'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import { useThemeStore } from '@/stores/theme'
import closeIcon from '@/assets/icon/close.svg'
import gearIcon from '@/assets/icon/gear.svg'
import logoIcon from '@/assets/icon/ufo_logo.png'
import moonIcon from '@/assets/icon/moon.svg'
import sunIcon from '@/assets/icon/sun.svg'

/** 悬浮窗设置页的 tab key；用固定串而不是模块 id，免得将来某个组件真叫这个名字。 */
const MODULES_TAB = 'floating-window'

/** 一个一级页面：某个组件的设置页，或「悬浮窗设置」。 */
interface MainTab {
  /** tab 唯一 key（组件页 = 模块 id） */
  key: string
  /** tabbar 上的文字；齿轮页只有图标时为空串 */
  label: string
  /** 单色图标地址；给了就渲染在文字前，或单独占一格（齿轮页） */
  icon?: string
  /** 没有可见文字时必须给的读屏名称 */
  ariaLabel?: string
  /** 是否靠右（齿轮页），与左侧的内容 tab 视觉分区 */
  end?: boolean
  /** 该页的二级分区；空数组 = 这页没有二级 tabbar */
  sections: FloatingModuleSection[]
}

const modules = useFloatingModulesStore()
const theme = useThemeStore()

/** 一级页面清单：每个有设置页的组件一页（顺序 = 注册表顺序）+ 右端的「悬浮窗设置」（齿轮）。 */
const tabs = computed<MainTab[]>(() => [
  ...FLOATING_MODULE_DEFINITIONS.value.filter(hasModuleSettings).map(definition => ({
    key: definition.id,
    label: definition.label,
    sections: definition.settings?.sections ?? [],
  })),
  { key: MODULES_TAB, label: '', icon: gearIcon, ariaLabel: '悬浮窗设置', end: true, sections: [] },
])

/** 交给 `BaseTabs` 的精简清单（它只认 key / label / icon / 无障碍名 / 是否靠右）。 */
const tabItems = computed(() =>
  tabs.value.map(tab => ({
    key: tab.key,
    label: tab.label,
    icon: tab.icon,
    ariaLabel: tab.ariaLabel,
    end: tab.end,
  }))
)

/**
 * 默认一级页 = 第一个组件设置页；没有任何组件有设置页时退回「悬浮窗设置」。
 * 组件包是异步装载的，所以这里只是初值 —— 「当前页没了要回落」由下面那条 watch 负责。
 */
const DEFAULT_TAB = FLOATING_MODULE_DEFINITIONS.value.find(hasModuleSettings)?.id ?? MODULES_TAB

const activeTab = ref<string>(DEFAULT_TAB)

// 装完 / 卸完一个包，它那一页可能出现或消失：停在不存在的页上主窗就整块空白。
watch(tabs, list => {
  if (!list.some(tab => tab.key === activeTab.value)) {
    activeTab.value = list.find(tab => !tab.end)?.key ?? MODULES_TAB
  }
})
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
/**
 * 一级 tab 的名称，用作各页那个（视觉隐藏的）h1。
 * 工具页没有可见文字，退回它的读屏名称，免得 h1 变空串。
 */
const activeTabLabel = computed(() => {
  const tab = tabs.value.find(item => item.key === activeTab.value)
  return tab?.label || tab?.ariaLabel || ''
})

/** 主题按钮的文案：按**要切到**的主题说，而不是当前主题（当前主题已由图标本身表达）。 */
const themeActionLabel = computed(() => (theme.isDark ? '切换到浅色主题' : '切换到深色主题'))

// 换一级页就回到该页第一个分区：组件设置页每次都从默认分区开始，不会停在上次看的那一页。
watch(
  activeSections,
  sections => {
    activeSectionId.value = sections[0]?.id ?? ''
  },
  { immediate: true }
)

/**
 * 切到某个组件的设置页（列表行齿轮 / 点悬浮窗上该组件两条路共用）。
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
      <span class="main-titlebar__title">
        <!-- 可见文字已写出「UFO」，图标只作装饰，alt 留空避免读屏重复播报。 -->
        <img :src="logoIcon" alt="" class="main-titlebar__logo" aria-hidden="true" />
        <span class="main-titlebar__name">UFO</span>
      </span>
      <button
        type="button"
        class="main-titlebar__close"
        aria-label="隐藏主窗口"
        title="隐藏"
        @click.stop="onCloseMain"
        @pointerdown.stop
      >
        <img :src="closeIcon" alt="" class="main-titlebar__close-icon ufo-mono-icon" aria-hidden="true" />
      </button>
    </header>

    <!--
      浮层宿主（`#main-content`）：标题栏**以下**整块，含两级 tabbar。
      组件浮层（`StatusLogOverlay` 一类）`Teleport` 到这里再写 `absolute inset-0`，
      顶边正好落在关闭按钮下面 —— 展开时盖住 tabbar 是允许的（浮层自带「完成」与 Esc 两个出口），
      但绝不越出 `.main-shell` 的圆角边框，也不碰标题栏：拖窗与隐藏到托盘照常可用。
      外壳只提供这块区域，不认识任何在里面上演的组件。
    -->
    <div class="main-content" id="main-content">
    <!-- 一级：各组件设置页 / 悬浮窗设置（齿轮页签，下划线，见 .main-tabs 默认样式）。 -->
    <BaseTabs v-model="activeTab" id-prefix="main" aria-label="主窗口分区" :tabs="tabItems">
      <!-- 右端动作：切换深浅色。月亮 = 当前是深色，太阳 = 当前是浅色（点击切到另一个）。 -->
      <template #actions>
        <button
          type="button"
          class="main-tabs__action"
          :aria-label="themeActionLabel"
          :title="themeActionLabel"
          @click="theme.toggle()"
        >
          <img
            :src="theme.isDark ? moonIcon : sunIcon"
            alt=""
            class="main-tabs__action-icon ufo-mono-icon"
            aria-hidden="true"
          />
        </button>
      </template>
    </BaseTabs>

    <!-- 二级：当前组件自己的设置分区（胶囊，只在该组件给了多个分区时出现）。 -->
    <BaseTabs
      v-if="activeSections.length > 1"
      v-model="activeSectionId"
      variant="chips"
      id-prefix="module"
      :aria-label="`${activeTabLabel} 设置分区`"
      :tabs="activeSections.map(section => ({ key: section.id, label: section.label }))"
    />

    <!--
      悬浮窗设置（齿轮页签）：一页到底、没有二级 —— 列表 + 预览 + 版本纵向堆叠，
      内容有多高算多高，滚外层页面（整页比视口高，不能用不滚的 --wide）。
    -->
    <main
      v-if="activeTab === MODULES_TAB"
      class="main-window main-window--stack"
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
  </div>
</template>