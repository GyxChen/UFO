<script setup lang="ts">
import { reactive, ref, watch } from 'vue'
import GreetComponent from './GreetComponent.vue'
import type { FloatingLayout, MaxCellsByLayout } from '@/lib/floating-modules'
import { useAgentStatusStore } from '@/stores/agentStatus'
import { useFloatingModulesStore } from '@/stores/floatingModules'

const store = useStore()
const agentStatus = useAgentStatusStore()
const modules = useFloatingModulesStore()
store.initApp()

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

async function toggleAgentStatus(event: Event) {
  const input = event.target as HTMLInputElement
  try {
    await modules.updateAgentStatus(input.checked)
  } catch {
    input.checked = modules.agentStatusEnabled
  }
}

async function updateRetention() {
  try {
    await modules.updateRetentionSeconds(retentionInput.value)
  } catch {
    retentionInput.value = modules.retentionSeconds
  }
}

async function updateMaxCells(layout: FloatingLayout) {
  try {
    await modules.updateMaxCellsByLayout({ [layout]: maxCellsInput[layout] })
  } catch {
    Object.assign(maxCellsInput, modules.maxCellsByLayout)
  }
}
</script>

<template>
  <main class="main-window">
    <h1>Welcome to Tauri 2 + Vue</h1>

    <div class="flex flex-row">
      <a href="https://vitejs.dev" target="_blank">
        <img src="/vite.svg" class="logo vite" alt="Vite logo" />
      </a>
      <a href="https://tauri.app" target="_blank">
        <img src="/tauri.svg" class="logo tauri" alt="Tauri logo" />
      </a>
      <a href="https://vuejs.org/" target="_blank">
        <img src="../assets/vue.svg" class="logo vue" alt="Vue logo" />
      </a>
    </div>
    <p>Click the floating launcher or tray icon to open this window.</p>

    <section class="module-settings" aria-labelledby="module-settings-title">
      <h2 id="module-settings-title">悬浮窗模块</h2>
      <label class="module-settings__toggle">
        <input
          type="checkbox"
          :checked="modules.agentStatusEnabled"
          @change="toggleAgentStatus"
        />
        <span>Agent 状态灯</span>
      </label>
      <label class="module-settings__retention">
        <span>完成灯保留（秒）</span>
        <input
          v-model.number="retentionInput"
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
          class="module-settings__retention"
        >
          <span>{{ item.label }}</span>
          <input
            v-model.number="maxCellsInput[item.key]"
            type="number"
            min="4"
            max="8"
            step="1"
            @change="updateMaxCells(item.key)"
          />
        </label>
      </div>
    </section>

    <p class="agent-session-list__meta">
      每个 CLI 会话对应一盏独立状态灯；窗口在默认 4 格之上按数量放大，上限由各形态配置决定。
    </p>
    <section class="agent-session-list" aria-labelledby="agent-session-list-title">
      <h2 id="agent-session-list-title">Agent 会话</h2>
      <p v-if="!agentStatus.sessions.length" class="agent-session-list__empty">当前没有已登记的会话</p>
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

    <GreetComponent />
  </main>
</template>
