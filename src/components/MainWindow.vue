<script setup lang="ts">
import { reactive, ref, watch } from 'vue'
import type { FloatingLayout, MaxCellsByLayout } from '@/lib/floating-modules'
import { hideMainWindow, startCurrentWindowDrag } from '@/lib/window'
import { useAgentStatusStore } from '@/stores/agentStatus'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import closeIcon from '@/assets/close.svg'

const agentStatus = useAgentStatusStore()
const modules = useFloatingModulesStore()

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

const retentionInput = ref(modules.retentionSeconds)
const maxCellsInput = reactive<MaxCellsByLayout>({ ...modules.maxCellsByLayout })

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
  () => modules.maxCellsByLayout,
  (value) => {
    Object.assign(maxCellsInput, value)
  },
  { immediate: true, deep: true },
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

/** 更新指定悬浮窗形态的格子上限。 */
async function updateMaxCells(layout: FloatingLayout) {
  try {
    await modules.updateMaxCellsByLayout({ [layout]: maxCellsInput[layout] })
  } catch {
    Object.assign(maxCellsInput, modules.maxCellsByLayout)
  }
}

/** 将各形态上限重置为当前 store 中的值。 */
function resetMaxCells() {
  Object.assign(maxCellsInput, modules.maxCellsByLayout)
  retentionInput.value = modules.retentionSeconds
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

    <main class="main-window">
    <header class="main-window__hero">
      <div class="main-window__badge">Agent Status</div>
      <h1 class="main-window__title">悬浮窗设置</h1>
      <p class="main-window__subtitle">
        深色哑光面板。配置 Agent 状态灯、完成保留时长与各形态格子上限。
      </p>
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
        <button class="ufo-btn ufo-btn--primary" type="button" @click="updateRetention">
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
  </div>
</template>
