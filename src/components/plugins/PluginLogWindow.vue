<script setup lang="ts">
/**
 * 插件日志独立窗（mock 的 05 段）：左列这个包的调用流水，右列选中那条的全部字段。
 *
 * 三条口径与转灯日志同源：
 * - 翻页读**盘**（内存环只有 200 行，历史都在 `plugins/<id>/logs/` 的按日 JSONL 里）；
 * - 实时跟随按「包 + 本窗口 label」挂摘：主窗那一页收起不会关掉这扇窗的跟随；
 * - 敏感数据出口要说清：正文可能含提问原文与密钥，所以没有「复制全部」这种一键外带动作，
 *   清空走两步确认。
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import PanelWindow from '@/components/ui/PanelWindow.vue'
import BaseButton from '@/components/ui/BaseButton.vue'
import { closePluginLogWindow, usePluginLogOpen, type PluginLogWindowView } from '@/lib/plugin-log-window'
import { PLUGIN_LOG_EVENT } from '@/lib/plugin-events'
import type { PluginLogLineJson } from '@/lib/plugin-types'

/** 这一窗正在看的包（由主窗递进来；没递到之前整窗是空的）。 */
const view = ref<PluginLogWindowView | null>(null)
/** 已载入的行（新 → 旧）。 */
const lines = ref<PluginLogLineJson[]>([])
/** 命中总数（含还没载入的部分）。 */
const total = ref(0)
/** 已载入到的偏移。 */
const loaded = ref(0)
/** 级别过滤：空串 = 全部。 */
const level = ref('')
/** 暂停跟随：看着某条时被新行顶走很难受，所以给一个明确的停住开关。 */
const paused = ref(false)
/** 暂停期间攒下的新行数（点「跟回最新」才吃掉）。 */
const pending = ref(0)
/** 选中那条（右列展开它的全部字段）。 */
const selected = ref<PluginLogLineJson | null>(null)
/** 一句常驻状态（禁了 alert，反馈一律走行内 role=status）。 */
const status = ref('')
/** 清空的两步确认态。 */
const armedClear = ref(false)

/** 一屏取多少行：与后端 `LOG_PAGE_LINES` 同一量级，这里刻意取小一点保证首屏快。 */
const PAGE = 200

/** 标题栏文案：要说清这是哪个包的日志。 */
const title = computed(() =>
  view.value ? `插件日志 · ${view.value.pluginName || view.value.pluginId}` : '插件日志'
)

/** 副标题：读数常驻，不许把「还有多少没载入」藏进 hover。 */
const hint = computed(() =>
  view.value
    ? `共 ${total.value} 条 · 已载入 ${loaded.value} 行（新→旧）· 关掉这个包的调试就不再写新行`
    : '从管理区点「拓展」打开'
)

/** 把毫秒时间戳说成 `HH:MM:SS`（列表里只放这个，秒级足够定位）。 */
function clockOf(ts: number): string {
  const date = new Date(ts)
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

/** 毫秒精度那一段：右列要能看出同一秒内的先后。 */
function clockWithMs(ts: number): string {
  return `${clockOf(ts)}.${String(ts % 1000).padStart(3, '0')}`
}

/** fields 的可读形态（后端已只留标量）。 */
const fieldEntries = computed<[string, unknown][]>(() =>
  Object.entries(selected.value?.fields ?? {})
)

/** 取一页：offset 从 0 开始，新行总是排在前面。 */
async function fetchPage(offset: number, replace: boolean) {
  const current = view.value
  if (!current) return
  try {
    const page = await invoke<{ lines: PluginLogLineJson[]; total: number; offset: number }>('get_plugin_logs', {
      id: current.pluginId,
      query: { level: level.value, offset, limit: PAGE },
    })
    lines.value = replace ? page.lines : [...lines.value, ...page.lines]
    total.value = page.total
    loaded.value = lines.value.length
    if (replace) selected.value = page.lines[0] ?? null
  } catch (error) {
    status.value = asText(error)
  }
}

/** 换过滤条件就从头翻（偏移与过滤混用会得到一条说不清的流水）。 */
async function reload() {
  lines.value = []
  loaded.value = 0
  total.value = 0
  await fetchPage(0, true)
}

/** 跟回最新：丢弃暂停期间攒的偏移，重新取首屏。 */
async function followLatest() {
  paused.value = false
  pending.value = 0
  await reload()
}

/** 看更早的：往旧方向再取一页。 */
async function loadMore() {
  await fetchPage(loaded.value, false)
}

/** 点一行 → 右列展开它的全部字段。 */
function pick(line: PluginLogLineJson) {
  selected.value = line
}

/** 清空这个包的日志（第二步才真删）。 */
async function clearLogs() {
  const current = view.value
  if (!current) return
  if (!armedClear.value) {
    armedClear.value = true
    status.value = '再点一次「确认清空」会删掉盘上的日志文件，删了就翻不回来'
    return
  }
  armedClear.value = false
  try {
    const removed = await invoke<number>('clear_plugin_logs', { id: current.pluginId })
    status.value = `已清空 ${removed} 行`
    await reload()
  } catch (error) {
    status.value = asText(error)
  }
}

/** 实时跟随：新行插到列表头（暂停时只攒数，不动列表）。 */
async function onLogEvent(payload: { pluginId?: string; lines?: PluginLogLineJson[] }) {
  const current = view.value
  if (!current || payload.pluginId !== current.pluginId) return
  const incoming = payload.lines ?? []
  if (!incoming.length) return
  if (paused.value) {
    pending.value += incoming.length
    return
  }
  lines.value = [...incoming.reverse(), ...lines.value]
  total.value += incoming.length
  loaded.value = lines.value.length
}

/** 把 invoke 的失败值变成一行可读中文。 */
function asText(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return String(error)
}

let unlistenOpen: UnlistenFn | undefined
let unlistenLog: UnlistenFn | undefined

onMounted(async () => {
  try {
    unlistenOpen = await usePluginLogOpen(async next => {
      view.value = next
      paused.value = false
      pending.value = 0
      armedClear.value = false
      status.value = ''
      await reload()
      // 挂上实时跟随（label 由 Rust 注入，本窗口只能挂自己的）。
      if (next) await invoke('set_plugin_log_view', { id: next.pluginId, attached: true }).catch(() => {})
    })
    unlistenLog = await listen<{ pluginId?: string; lines?: PluginLogLineJson[] }>(PLUGIN_LOG_EVENT, event => {
      void onLogEvent(event.payload)
    })
  } catch (error) {
    console.warn('plugin log channel unavailable', error)
  }
})

onBeforeUnmount(() => {
  const current = view.value
  if (current) {
    void invoke('set_plugin_log_view', { id: current.pluginId, attached: false }).catch(() => {})
  }
  unlistenOpen?.()
  unlistenLog?.()
})

/** × 与 Esc 同一条路：先摘跟随，再销毁自己（「我关了」由宿主代为回报发起方）。 */
async function dismiss() {
  const current = view.value
  if (current) {
    await invoke('set_plugin_log_view', { id: current.pluginId, attached: false }).catch(() => {})
  }
  await closePluginLogWindow()
}
</script>

<template>
  <PanelWindow :title="title" :hint="hint" close-label="完成并返回" @close="dismiss">
    <div class="plugin-log">
      <div class="plugin-log__filters">
        <label class="plugin-log__field">
          级别
          <select v-model="level" class="plugin-log__select" @change="reload">
            <option value="">全部</option>
            <option value="error">error</option>
            <option value="warn">warn</option>
            <option value="info">info</option>
          </select>
        </label>
        <label class="plugin-log__check">
          <input v-model="paused" type="checkbox" />
          暂停跟随
        </label>
        <BaseButton v-if="pending" size="sm" @click="followLatest">
          攒了 {{ pending }} 条，跟回最新
        </BaseButton>
        <span class="plugin-log__count">已载入 {{ loaded }} 行 / 共 {{ total }} 条</span>
      </div>

      <div class="plugin-log__split">
        <div class="plugin-log__list" role="list">
          <p v-if="!view" class="plugin-log__empty">从管理区的调试卡点「拓展」打开一个包的日志。</p>
          <button
            v-for="(line, index) in lines"
            :key="`${line.traceId ?? ''}-${index}`"
            type="button"
            class="plugin-log__row"
            :class="{ 'plugin-log__row--cur': selected === line }"
            role="listitem"
            @click="pick(line)"
          >
            <span class="plugin-log__time">{{ clockOf(line.ts) }}</span>
            <span class="plugin-log__level" :data-level="line.level">{{ line.level }}</span>
            <span class="plugin-log__msg">{{ line.msg }}</span>
            <span v-if="line.traceId" class="plugin-log__trace">{{ line.traceId }}</span>
          </button>
          <div v-if="lines.length" class="plugin-log__foot">
            <BaseButton size="sm" :disabled="loaded >= total" @click="loadMore">看更早的</BaseButton>
          </div>
        </div>

        <section class="plugin-log__detail" aria-label="选中记录的完整信息">
          <template v-if="selected">
            <div class="plugin-log__detail-head">
              <span class="plugin-log__level" :data-level="selected.level">{{ selected.level }}</span>
              <strong>{{ selected.msg }}</strong>
              <span class="plugin-log__time">{{ clockWithMs(selected.ts) }}</span>
            </div>
            <dl class="plugin-log__kv">
              <dt>包</dt>
              <dd>{{ view?.pluginName }} <code>{{ view?.pluginId }}</code></dd>
              <dt>调用</dt>
              <dd>ufo.log('{{ selected.level }}', …)</dd>
              <dt>盘上坐标</dt>
              <dd>{{ selected.traceId ?? '—' }}</dd>
            </dl>
            <template v-if="fieldEntries.length">
              <h2 class="plugin-log__sub">fields（结构化参数，只留标量）</h2>
              <dl class="plugin-log__kv">
                <dt v-for="[key] in fieldEntries" :key="key">{{ key }}</dt>
                <dd v-for="[, value] in fieldEntries" :key="`v-${String(value)}`">{{ String(value) }}</dd>
              </dl>
            </template>
            <p v-else class="plugin-log__sub">这条没带 fields。</p>
          </template>
          <p v-else class="plugin-log__empty">左边点一行，这里展开它的全部字段。</p>
        </section>
      </div>

      <div class="plugin-log__foot-bar">
        <BaseButton variant="danger" size="sm" :disabled="!view" @click="clearLogs">
          {{ armedClear ? '确认清空？' : '清空该插件日志' }}
        </BaseButton>
        <p class="plugin-log__status" role="status">{{ status }}</p>
        <p class="plugin-log__dir">
          %APPDATA%/com.ufo.app/plugins/{{ view?.pluginId ?? '&lt;id&gt;' }}/logs
        </p>
      </div>
    </div>
  </PanelWindow>
</template>

<style scoped>
/* 独立窗口自己的样式：普通 CSS（`@apply` 只在入口 CSS 或带 `@reference` 的文件里可用）。 */
.plugin-log {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
  gap: 10px;
}

.plugin-log__filters {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
  font-size: 11.5px;
  color: var(--ufo-muted);
}

.plugin-log__field {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.plugin-log__check {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}

.plugin-log__select {
  font: inherit;
  padding: 3px 6px;
  color: var(--ufo-text);
  background: var(--ufo-input);
  border: 1px solid var(--ufo-border-strong);
  border-radius: 8px;
}

.plugin-log__count {
  margin-left: auto;
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}

.plugin-log__split {
  display: grid;
  grid-template-columns: 1.8fr 1fr;
  gap: 10px;
  flex: 1;
  min-height: 0;
}

.plugin-log__list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-height: 0;
  overflow-y: auto;
  padding-right: 4px;
}

.plugin-log__row {
  display: grid;
  grid-template-columns: 62px 44px 1fr auto;
  align-items: baseline;
  gap: 8px;
  width: 100%;
  padding: 5px 8px;
  font: inherit;
  font-size: 11.5px;
  text-align: left;
  color: var(--ufo-text);
  background: var(--ufo-surface-2);
  border: 1px solid transparent;
  border-radius: 8px;
  cursor: pointer;
}

.plugin-log__row--cur {
  border-color: var(--ufo-accent);
}

.plugin-log__time {
  font-size: 10.5px;
  font-variant-numeric: tabular-nums;
  color: var(--ufo-muted);
}

.plugin-log__level {
  font-size: 10.5px;
  font-weight: 700;
}

.plugin-log__level[data-level='error'] {
  color: var(--ufo-status-red-text);
}

.plugin-log__level[data-level='warn'] {
  color: var(--ufo-status-yellow-text);
}

.plugin-log__level[data-level='info'] {
  color: var(--ufo-status-blue-text);
}

.plugin-log__msg {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.plugin-log__trace {
  font-size: 10px;
  color: var(--ufo-muted);
}

.plugin-log__foot,
.plugin-log__foot-bar {
  display: flex;
  align-items: center;
  gap: 8px;
}

.plugin-log__detail {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  overflow-y: auto;
  padding: 10px;
  background: var(--ufo-surface-2);
  border-radius: 10px;
}

.plugin-log__detail-head {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 8px;
  font-size: 12px;
}

.plugin-log__kv {
  display: grid;
  grid-template-columns: auto 1fr;
  gap: 2px 10px;
  margin: 0;
  font-size: 11px;
}

.plugin-log__kv dt {
  color: var(--ufo-muted);
}

.plugin-log__kv dd {
  margin: 0;
  overflow-wrap: anywhere;
}

.plugin-log__sub {
  font-size: 11px;
  color: var(--ufo-muted);
}

.plugin-log__empty {
  font-size: 11.5px;
  color: var(--ufo-muted);
}

.plugin-log__status {
  font-size: 11.5px;
  color: var(--ufo-status-green-text);
}

.plugin-log__dir {
  margin-left: auto;
  font-size: 10.5px;
  color: var(--ufo-muted);
}
</style>
