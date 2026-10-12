<script setup lang="ts">
/**
 * 齿轮页下方的「插件管理」：满宽一张卡，内部左右两栏。
 *
 * 左栏回答「装了哪些、开没开」，右栏回答「这个是什么、能碰什么、现在在不在调试」。
 * ⚠️ 权限徽标与调试读数**只出现在右栏**：左栏要在窄列里能扫读，塞徽标就会把名字挤成省略号。
 *
 * 安装是两步：`preview_plugin` 先校验并拿到逐条披露 → 自绘确认弹窗 → 用户点头才 `install_plugin`。
 * 卸载同样两步（原生 `confirm` 被插件拦掉了，也不该用）。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import BaseButton from '@/components/ui/BaseButton.vue'
import BaseSwitch from '@/components/ui/BaseSwitch.vue'
import SettingsHint from '@/components/ui/SettingsHint.vue'
import PluginCapabilityIcon from './PluginCapabilityIcon.vue'
import PluginInstallConfirm from './PluginInstallConfirm.vue'
import { formatBytes, formatDuration } from '@/lib/plugin-format'
import { openPluginLogWindow } from '@/lib/plugin-log-window'
import { PLUGIN_LOG_EVENT } from '@/lib/plugin-events'
import { pluginLoadErrors } from '@/lib/plugin-loader'
import { RECENT_LOG_LINES, type PluginLogLineJson } from '@/lib/plugin-types'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import { usePluginsStore } from '@/stores/plugins'
import type { PluginRecordJson } from '@/lib/plugin-types'

const plugins = usePluginsStore()
const modules = useFloatingModulesStore()

/** 已装包按外壳配置的 `order` 排（与悬浮窗里的先后是同一个字段）。 */
const records = computed(() => {
  const orderOf = (id: string) =>
    modules.config?.modules.find(entry => entry.id === id)?.order ?? Number.MAX_SAFE_INTEGER
  return [...plugins.records].sort((left, right) => orderOf(left.id) - orderOf(right.id) || left.id.localeCompare(right.id))
})

/** 右栏现在讲的是哪个包（安装记录一变就跟着落回第一条）。 */
const selected = computed<PluginRecordJson | null>(
  () => records.value.find(record => record.id === plugins.selectedId) ?? records.value[0] ?? null
)

/** 卡头读数：装了几个、几个在调试、盘上占了多少。 */
const installedCount = computed(() => records.value.length)
const debugCount = computed(() => plugins.debugCount)
const totalBytes = computed(() => plugins.totalBytes)

/** 选中包的徽标（挂载点 + 能力）。 */
const badges = computed(() => plugins.badgesOf(selected.value))

/** 选中包的装载失败原因（列表行与详情都说它，别说两套话）。 */
function reasonOf(record: PluginRecordJson): string {
  return record.error ?? pluginLoadErrors.value[record.id] ?? ''
}

/** 启用开关走的是**外壳配置**（与内置组件同一个字段、同一套挂载判定），安装记录不另存一份。 */
function isEnabled(record: PluginRecordJson): boolean {
  return modules.moduleEnabled(record.id, true)
}

/** 切启用；失败时开关自己会停在旧值（配置没变，列表自然回原样）。 */
async function onToggle(record: PluginRecordJson, event: Event) {
  const input = event.target as HTMLInputElement
  try {
    await modules.updateModuleEnabled(record.id, input.checked)
  } catch {
    input.checked = isEnabled(record)
  }
}

/** 待确认安装的那份 preview 结果；null = 弹窗没开。 */
const pending = ref<PluginRecordJson | null>(null)
/** 正在写盘（安装 / 卸载 / 重扫）：按钮期间禁用，避免连点两次。 */
const busy = ref(false)

/** 选系统里的包目录 → preview → 开确认弹窗（不写任何东西）。 */
async function pickAndPreview() {
  if (busy.value) return
  const picked = await open({ directory: true, multiple: false, title: '选择插件包目录' })
  if (typeof picked !== 'string' || !picked) return
  busy.value = true
  try {
    pending.value = await plugins.preview(picked)
  } catch {
    // 原因已经落在 plugins.actionError，行内提示会显示；这里只负责不再往下走。
  } finally {
    busy.value = false
  }
}

/** 用户在确认弹窗里点头：这才写入安装记录。 */
async function confirmInstall() {
  const record = pending.value
  if (!record) return
  pending.value = null
  busy.value = true
  try {
    await plugins.install(record.dir)
    plugins.select(record.id)
  } finally {
    busy.value = false
  }
}

/**
 * 调试卡里的「最近日志」（最多 `RECENT_LOG_LINES` 行）。
 *
 * 读的是内存环形缓冲（`get_recent_plugin_logs`），所以关着调试时它就该是空的 ——
 * 那是语义而不是缺数据。要翻历史走「拓展」开独立窗（那条读盘）。
 */
const recent = ref<PluginLogLineJson[]>([])

/** 取最近几行（选中包变了、或刚写完一条日志时）。 */
async function fetchRecent() {
  const record = selected.value
  recent.value = []
  if (!record?.devReload) return
  try {
    recent.value = await invoke<PluginLogLineJson[]>('get_recent_plugin_logs', {
      id: record.id,
      limit: RECENT_LOG_LINES,
    })
  } catch {
    // 没挂上就不打扰用户：这一格本来只在开着调试时才有内容。
  }
}

/** 本窗口对这个包的实时跟随（摘掉后独立窗的跟随不受影响：那边按自己的 label 记账）。 */
async function attachView(id: string, attached: boolean) {
  await invoke('set_plugin_log_view', { id, attached }).catch(() => {})
}

let unlistenLog: UnlistenFn | undefined
/** 当前挂着的跟随目标（换包 / 卸载时要先摘旧的）。 */
let attachedId = ''

/** 把跟随换到选中的包：只在它开着调试时挂。 */
async function syncView() {
  const record = selected.value
  const next = record?.devReload ? record.id : ''
  if (next === attachedId) return
  if (attachedId) await attachView(attachedId, false)
  attachedId = next
  if (next) await attachView(next, true)
  await fetchRecent()
}

watch(() => [selected.value?.id, selected.value?.devReload], syncView, { immediate: true })

onBeforeUnmount(() => {
  unlistenLog?.()
  if (attachedId) void attachView(attachedId, false)
})

/** 新到的行直接顶到列表头（这一格只留最近几行，历史在独立窗里翻）。 */
async function onLogEvent(payload: { pluginId?: string; lines?: PluginLogLineJson[] }) {
  const record = selected.value
  if (!record || payload.pluginId !== record.id) return
  const incoming = payload.lines ?? []
  if (!incoming.length) return
  recent.value = [...incoming.slice().reverse(), ...recent.value].slice(0, RECENT_LOG_LINES)
}

onMounted(async () => {
  try {
    unlistenLog = await listen<{ pluginId?: string; lines?: PluginLogLineJson[] }>(PLUGIN_LOG_EVENT, event => {
      void onLogEvent(event.payload)
    })
  } catch (error) {
    console.warn('plugin log channel unavailable', error)
  }
})

/** 「拓展」：开独立日志窗（与脚本编辑器 / 转灯日志同一条 open 协议）。 */
async function openLogWindow() {
  const record = selected.value
  if (!record) return
  const opened = await openPluginLogWindow({
    pluginId: record.id,
    pluginName: record.name || record.id,
  })
  if (!opened) {
    plugins.actionError = '插件日志窗没在 tauri.conf.json 里声明，开不出来'
  }
}

/** 时间戳说成 `HH:MM:SS`（一行里只放这个，秒级足够定位是哪一次轮询）。 */
function clockOf(ts: number): string {
  const date = new Date(ts)
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

/** 卸载的两步确认状态：第一下只是「armed」，第二下才真删。 */
const armedUninstall = ref(false)

/** 卸载当前选中的包（第一步）。 */
function armUninstall() {
  armedUninstall.value = true
}

/** 第二步：真卸载；成功与失败都退出 armed 态，免得按钮停在「确认卸载？」等下一次误点。 */
async function confirmUninstall() {
  const record = selected.value
  armedUninstall.value = false
  if (!record) return
  busy.value = true
  try {
    await plugins.uninstall(record.id)
  } finally {
    busy.value = false
  }
}

/** 重新加载当前选中的包（作者改完清单存盘后点它）。 */
async function reload() {
  if (busy.value) return
  busy.value = true
  try {
    await plugins.reload()
  } finally {
    busy.value = false
  }
}

/** 逐包热重载开关：只有本地目录源的包能开（Rust 侧同一条判据会拒别人）。 */
async function onDevReload(record: PluginRecordJson, event: Event) {
  const input = event.target as HTMLInputElement
  try {
    await plugins.setDevReload(record.id, input.checked)
  } catch {
    input.checked = record.devReload
  }
}
</script>

<template>
  <section class="ufo-card plugin-manager" aria-labelledby="plugin-manager-title">
    <div class="ufo-card__header">
      <div class="ufo-heading-row">
        <h2 id="plugin-manager-title" class="ufo-card__title">插件管理</h2>
        <SettingsHint
          text="装一个组件包 = 让你主动运行别人写的程序。徽标是知情同意书，不是沙盒：界面侧的组件代码与宿主同一个进程，只有后台脚本那侧的注入是真闸。"
        />
        <span v-if="debugCount" class="plugin-manager__badge">{{ debugCount }} 个在调试</span>
      </div>
      <div class="plugin-manager__head-actions">
        <span class="ufo-card__hint">
          {{ installedCount }} 个已安装 · 盘上 {{ formatBytes(totalBytes) }}
        </span>
        <BaseButton variant="primary" size="sm" :disabled="busy" @click="pickAndPreview">
          安装插件…
        </BaseButton>
      </div>
    </div>

    <div class="plugin-manager__grid">
      <!-- 左栏：装了哪些、开没开。名字之外的细节一律不占这一行。 -->
      <div class="plugin-manager__column">
        <ul class="module-catalog__rows">
          <li
            v-for="record in records"
            :key="record.id"
            class="module-row"
            :class="{
              'module-row--off': !isEnabled(record),
              'plugin-manager__row--sel': selected?.id === record.id,
            }"
          >
            <button type="button" class="plugin-manager__pick" @click="plugins.select(record.id)">
              <span class="plugin-manager__order" aria-hidden="true">{{ isEnabled(record) ? '●' : '–' }}</span>
              <span class="module-row__copy">
                <span class="module-row__label">
                  {{ record.name || record.id }}
                  <span v-if="record.version" class="plugin-manager__badge">{{ record.version }}</span>
                  <span v-if="record.devReload" class="plugin-manager__badge plugin-manager__badge--debug">调试</span>
                </span>
                <span class="module-row__hint">{{ reasonOf(record) || record.manifest?.description || record.id }}</span>
              </span>
            </button>

            <span class="module-row__demand">
              {{ record.manifest?.surfaces.cell ? `${record.manifest.surfaces.cell} 格` : '—' }}
            </span>

            <BaseSwitch
              :model-value="isEnabled(record)"
              :aria-label="`启用 ${record.name || record.id}`"
              :title="`启用 ${record.name || record.id}`"
              @change="onToggle(record, $event)"
            />
          </li>
        </ul>

        <p v-if="!records.length" class="plugin-manager__empty">
          还没有装任何组件包。
        </p>

        <button type="button" class="plugin-manager__drop" :disabled="busy" @click="pickAndPreview">
          选一个本地目录装上（目录里要有 plugin.json）
          <span class="plugin-manager__drop-sub">就地装载：改完存盘点「重新加载」就生效</span>
        </button>

        <p class="plugin-manager__status" role="status">{{ plugins.actionError }}</p>
      </div>

      <!-- 右栏：这是什么 + 能碰什么 + 调试。 -->
      <div class="plugin-manager__column">
        <template v-if="selected">
          <section class="plugin-manager__detail">
            <div class="plugin-manager__detail-head">
              <h3 class="plugin-manager__detail-title">{{ selected.name || selected.id }}</h3>
              <span class="plugin-manager__meta">
                id <code>{{ selected.id }}</code> · v{{ selected.version }}
              </span>
            </div>
            <p class="plugin-manager__desc">
              {{ selected.manifest?.description || '（清单没给说明）' }}
            </p>
            <p v-if="reasonOf(selected)" class="plugin-manager__reason" role="status">
              {{ reasonOf(selected) }}
            </p>

            <h4 class="plugin-manager__sub">权限（装时从清单读出，只读）</h4>
            <ul class="plugin-manager__perms">
              <li v-for="badge in badges" :key="badge.id" :data-tone="badge.tone" :title="badge.disclosure">
                <PluginCapabilityIcon :icon="badge.icon" />
                <span>{{ badge.label }}</span>
              </li>
            </ul>
            <p v-if="selected.manifest?.hosts.length" class="plugin-manager__hosts">
              出网只放行：{{ selected.manifest.hosts.join(' · ') }}
            </p>
            <p class="plugin-manager__meta">
              {{ selected.files }} 个文件 · {{ formatBytes(selected.bytes) }} · 装载
              {{ formatDuration(selected.loadMs) }}
            </p>
            <ul v-if="selected.warnings.length" class="plugin-manager__warns">
              <li v-for="warning in selected.warnings" :key="warning">{{ warning }}</li>
            </ul>
          </section>

          <section class="plugin-manager__debug">
            <div class="plugin-manager__debug-head">
              <h3 class="plugin-manager__detail-title">调试</h3>
              <SettingsHint
                text="开着才监听文件变化、才留日志；关掉就什么都不做，组件按上次加载的版本跑。只有本地目录安装的包能开。"
              />
              <BaseSwitch
                :model-value="selected.devReload"
                :disabled="selected.source !== 'local'"
                :aria-label="`热重载 ${selected.name || selected.id}`"
                :title="selected.source === 'local' ? '热重载' : '只有本地目录安装的包能开热重载'"
                @change="onDevReload(selected, $event)"
              />
            </div>
            <p class="plugin-manager__status" :class="{ 'plugin-manager__status--ok': selected.devReload }" role="status">
              {{
                selected.devReload
                  ? '已开：入口文件改动 2 秒内自动生效，清单改动要「重新加载」'
                  : '关着：不监听文件、不留日志'
              }}
            </p>

            <div class="plugin-manager__log-head">
              <span>最近日志（最多 {{ RECENT_LOG_LINES }} 行）</span>
              <BaseButton size="sm" :disabled="!selected.devReload" title="打开完整流水" @click="openLogWindow">
                拓展
              </BaseButton>
            </div>
            <ul v-if="recent.length" class="plugin-manager__plog">
              <li v-for="(line, index) in recent" :key="`${line.traceId ?? ''}-${index}`">
                <span class="plugin-manager__plog-time">{{ clockOf(line.ts) }}</span>
                <span class="plugin-manager__plog-level" :data-level="line.level">{{ line.level }}</span>
                <span class="plugin-manager__plog-msg">{{ line.msg }}</span>
              </li>
            </ul>
            <p v-else class="plugin-manager__plog-empty">
              {{ selected.devReload ? '还没有这个包写过的日志（它自己调 ufo.log 才会有）' : '关着调试不留日志行' }}
            </p>

            <div class="plugin-manager__actions">
              <BaseButton size="sm" :disabled="busy" @click="reload">重新加载</BaseButton>
              <BaseButton
                v-if="!armedUninstall"
                variant="danger"
                size="sm"
                :disabled="busy"
                @click="armUninstall"
              >
                卸载
              </BaseButton>
              <BaseButton v-else variant="danger" size="sm" @click="confirmUninstall">确认卸载？</BaseButton>
            </div>
          </section>
        </template>

        <p v-else class="plugin-manager__empty">左边选一个已安装的包看它的权限与调试。</p>
      </div>
    </div>

    <PluginInstallConfirm
      v-if="pending"
      :record="pending"
      :fact-of="plugins.factOf"
      @confirm="confirmInstall"
      @cancel="pending = null"
    />
  </section>
</template>
