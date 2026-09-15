<script setup lang="ts">
import { nextTick, reactive, ref, watch } from 'vue'
import type { FloatingLayout, MaxCellsByLayout, TokenUnit } from '@/lib/floating-modules'
import { hideMainWindow, startCurrentWindowDrag } from '@/lib/window'
import { useAgentStatusStore } from '@/stores/agentStatus'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import EndpointExtensionPanel from './EndpointExtensionPanel.vue'
import closeIcon from '@/assets/close.svg'

const agentStatus = useAgentStatusStore()
const modules = useFloatingModulesStore()

/** 主窗口顶部的两个页面；`settings` 沿用原有设置页，`extension` 是按 endpoint 的拓展配置。 */
type MainTab = 'settings' | 'extension'
const activeTab = ref<MainTab>('settings')
const tabs: { key: MainTab; label: string }[] = [
  { key: 'settings', label: '悬浮窗设置' },
  { key: 'extension', label: '信息卡拓展' },
]

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

/** tab 按钮元素，用于方向键切换后把焦点移到新选中项（roving tabindex）。 */
const tabRefs = ref<HTMLElement[]>([])
function setTabRef(el: unknown, index: number) {
  if (el) tabRefs.value[index] = el as HTMLElement
}

/**
 * tablist 键盘导航：←/→ 在两个页面间循环，Home/End 跳到首尾。
 * 只有选中项可 Tab 到达，因此方向键是唯一的横向移动方式（WAI-ARIA Tabs 模式）。
 */
function onTabKeydown(event: KeyboardEvent, index: number) {
  const count = tabs.length
  let next: number | null = null
  if (event.key === 'ArrowRight') next = (index + 1) % count
  else if (event.key === 'ArrowLeft') next = (index - 1 + count) % count
  else if (event.key === 'Home') next = 0
  else if (event.key === 'End') next = count - 1
  if (next === null) return
  event.preventDefault()
  activeTab.value = tabs[next].key
  void nextTick(() => tabRefs.value[next]?.focus())
}

/** 关闭按钮：隐藏到托盘，不退出进程。 */
async function onCloseMain() {
  try {
    await hideMainWindow()
  } catch (error) {
    console.error('failed to hide main window', error)
  }
}

const retentionInput = ref(modules.retentionSeconds)
const runningTimeoutInput = ref(modules.runningTimeoutSeconds)
const maxCellsInput = reactive<MaxCellsByLayout>({ ...modules.maxCellsByLayout })
const tokenUnitInput = ref<TokenUnit>(modules.tokenUnit)
const proxyResolverInput = ref(modules.proxyResolver)

/** token 单位选项；`tokens` 显示原始计数，`m_tokens` 显示百万。 */
const tokenUnitOptions: { value: TokenUnit; label: string }[] = [
  { value: 'tokens', label: 'Tokens' },
  { value: 'm_tokens', label: 'M tokens' },
]

const layoutLabels: { key: FloatingLayout; label: string }[] = [
  { key: 'square', label: '方形上限' },
  { key: 'corner', label: '四角上限' },
  { key: 'horizontal', label: '横条上限' },
  { key: 'vertical', label: '竖条上限' },
]

watch(
  () => modules.retentionSeconds,
  (seconds) => {
    retentionInput.value = seconds
  },
  { immediate: true },
)

watch(
  () => modules.runningTimeoutSeconds,
  (seconds) => {
    runningTimeoutInput.value = seconds
  },
  { immediate: true },
)

watch(
  () => modules.maxCellsByLayout,
  (value) => {
    Object.assign(maxCellsInput, value)
  },
  { immediate: true, deep: true },
)

watch(
  () => modules.tokenUnit,
  (unit) => {
    tokenUnitInput.value = unit
  },
  { immediate: true },
)

watch(
  () => modules.proxyResolver,
  (id) => {
    proxyResolverInput.value = id
  },
  { immediate: true },
)

/** 切换 Agent 状态灯模块启用状态；失败时回滚 checkbox。 */
async function toggleAgentStatus(event: Event) {
  const input = event.target as HTMLInputElement
  try {
    await modules.updateAgentStatus(input.checked)
  } catch {
    input.checked = modules.agentStatusEnabled
  }
}

/** 更新完成灯保留时长（秒）。 */
async function updateRetention() {
  try {
    await modules.updateRetentionSeconds(retentionInput.value)
  } catch {
    retentionInput.value = modules.retentionSeconds
  }
}

/** 更新 Running 无新 hook 超时（秒）；慢 agent 可拉长避免绿灯被误清。 */
async function updateRunningTimeout() {
  try {
    await modules.updateRunningTimeoutSeconds(runningTimeoutInput.value)
  } catch {
    runningTimeoutInput.value = modules.runningTimeoutSeconds
  }
}

/** 更新指定悬浮窗形态的格子上限。 */
async function updateMaxCells(layout: FloatingLayout) {
  try {
    await modules.updateMaxCellsByLayout({ [layout]: maxCellsInput[layout] })
  } catch {
    Object.assign(maxCellsInput, modules.maxCellsByLayout)
  }
}

/** 更新 token 展示单位；失败时回滚到 store 中的值。 */
async function updateTokenUnit() {
  try {
    await modules.updateTokenUnit(tokenUnitInput.value)
  } catch {
    tokenUnitInput.value = modules.tokenUnit
  }
}

/** 更新选中的代理解析器；失败时回滚到 store 中的值。 */
async function updateProxyResolver() {
  try {
    await modules.updateProxyResolver(proxyResolverInput.value)
  } catch {
    proxyResolverInput.value = modules.proxyResolver
  }
}

/** 将各形态上限与超时设置重置为当前 store 中的值。 */
function resetMaxCells() {
  Object.assign(maxCellsInput, modules.maxCellsByLayout)
  retentionInput.value = modules.retentionSeconds
  runningTimeoutInput.value = modules.runningTimeoutSeconds
  tokenUnitInput.value = modules.tokenUnit
  proxyResolverInput.value = modules.proxyResolver
}

/** 保存完成灯保留 + Running 超时。 */
async function saveTimeouts() {
  await updateRetention()
  await updateRunningTimeout()
}
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

    <nav class="main-tabs" role="tablist" aria-label="主窗口分区">
      <button
        v-for="(tab, index) in tabs"
        :key="tab.key"
        :ref="(el) => setTabRef(el, index)"
        type="button"
        role="tab"
        class="main-tabs__tab"
        :class="{ 'main-tabs__tab--active': activeTab === tab.key }"
        :aria-selected="activeTab === tab.key"
        :aria-controls="`main-tab-panel-${tab.key}`"
        :id="`main-tab-${tab.key}`"
        :tabindex="activeTab === tab.key ? 0 : -1"
        @click="activeTab = tab.key"
        @keydown="onTabKeydown($event, index)"
      >
        {{ tab.label }}
      </button>
    </nav>

    <main
      v-if="activeTab === 'settings'"
      class="main-window"
      role="tabpanel"
      tabindex="0"
      id="main-tab-panel-settings"
      aria-labelledby="main-tab-settings"
    >
    <header class="main-window__hero">
      <h1 class="main-window__title">悬浮窗设置</h1>
      <div class="main-window__badge">Agent Status</div>
    </header>

    <section class="ufo-card module-settings" aria-labelledby="module-settings-title">
      <div class="ufo-card__header">
        <h2 id="module-settings-title" class="ufo-card__title">悬浮窗模块</h2>
        <span class="ufo-card__hint">即时生效</span>
      </div>

      <div class="module-settings__toggle">
        <div class="module-settings__toggle-copy">
          <span class="module-settings__toggle-label">Agent 状态灯</span>
          <span class="module-settings__toggle-hint">在悬浮窗显示会话指示灯</span>
        </div>
        <label class="ufo-switch" title="启用 Agent 状态灯">
          <input
            type="checkbox"
            :checked="modules.agentStatusEnabled"
            aria-label="启用 Agent 状态灯"
            @change="toggleAgentStatus"
          />
          <span class="ufo-switch__track" aria-hidden="true" />
        </label>
      </div>

      <label class="module-settings__retention">
        <span>完成灯保留（秒）</span>
        <input
          v-model.number="retentionInput"
          class="ufo-input"
          type="number"
          min="1"
          max="86400"
          step="1"
          @change="updateRetention"
        />
      </label>

      <label class="module-settings__retention">
        <span>执行中超时（秒）</span>
        <input
          v-model.number="runningTimeoutInput"
          class="ufo-input"
          type="number"
          min="60"
          max="3600"
          step="30"
          title="Running 无新 hook 时移除绿灯；慢 agent 可拉长"
          @change="updateRunningTimeout"
        />
      </label>
      <p class="module-settings__field-hint">
        绿灯（Running）超过该时长无新事件会被移除；默认 60 秒，范围 60–3600。
      </p>

      <div class="module-settings__retention">
        <span id="token-unit-label">Token 单位</span>
        <div class="ufo-segmented" role="radiogroup" aria-labelledby="token-unit-label">
          <label
            v-for="option in tokenUnitOptions"
            :key="option.value"
            class="ufo-segmented__option"
          >
            <input
              v-model="tokenUnitInput"
              type="radio"
              name="token-unit"
              :value="option.value"
              @change="updateTokenUnit"
            />
            {{ option.label }}
          </label>
        </div>
      </div>
      <p class="module-settings__field-hint">
        悬浮窗提示行里的 token 总量写法：Tokens 显示原始计数，M tokens 以百万为单位。
      </p>

      <div class="module-settings__retention">
        <span id="proxy-resolver-label">代理解析器</span>
        <select
          v-model="proxyResolverInput"
          class="ufo-input ufo-select"
          aria-labelledby="proxy-resolver-label"
          @change="updateProxyResolver"
        >
          <option v-for="option in modules.resolvers" :key="option.id" :value="option.id">
            {{ option.label }}
          </option>
        </select>
      </div>
      <p class="module-settings__field-hint">
        选了代理工具后，会把它监听的本地地址（如 127.0.0.1）反查成真正的上游 endpoint 显示在提示窗；
        不解析时按原样显示。
      </p>

      <div class="module-settings__caps" aria-label="各形态格子上限">
        <label
          v-for="item in layoutLabels"
          :key="item.key"
          class="module-settings__field"
        >
          <span>{{ item.label }}</span>
          <input
            v-model.number="maxCellsInput[item.key]"
            class="ufo-input"
            type="number"
            min="4"
            max="8"
            step="1"
            @change="updateMaxCells(item.key)"
          />
        </label>
      </div>

      <div class="module-settings__actions">
        <button class="ufo-btn ufo-btn--ghost" type="button" @click="resetMaxCells">
          重置
        </button>
        <button class="ufo-btn ufo-btn--primary" type="button" @click="saveTimeouts">
          保存
        </button>
      </div>
    </section>

    <p class="agent-session-list__meta">
      每个 CLI 会话对应一盏独立状态灯；窗口在默认 4 格之上按数量放大，上限由各形态配置决定。
    </p>

    <section class="ufo-card agent-session-list" aria-labelledby="agent-session-list-title">
      <div class="ufo-card__header">
        <h2 id="agent-session-list-title" class="ufo-card__title">Agent 会话</h2>
        <span class="ufo-card__hint">
          {{ agentStatus.sessions.length ? `${agentStatus.sessions.length} 个活跃` : '暂无' }}
        </span>
      </div>
      <p v-if="!agentStatus.sessions.length" class="agent-session-list__empty">
        当前没有已登记的会话
      </p>
      <ul v-else>
        <li v-for="session in agentStatus.sessions" :key="session.key">
          <span
            class="agent-session-list__dot"
            :class="`agent-session-list__dot--${agentStatus.sessionColor(session)}`"
            aria-hidden="true"
          />
          <span>{{ session.provider }} · {{ session.session_id.slice(0, 12) }}</span>
          <span>{{ session.phase }}</span>
        </li>
      </ul>
    </section>
    </main>

    <main
      v-else
      class="main-window main-window--wide"
      role="tabpanel"
      tabindex="0"
      id="main-tab-panel-extension"
      aria-labelledby="main-tab-extension"
    >
      <EndpointExtensionPanel />
    </main>
  </div>
</template>
