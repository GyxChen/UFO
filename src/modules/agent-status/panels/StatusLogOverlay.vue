<script setup lang="ts">
/**
 * 转灯日志的整窗浮层：左栏本会话流水，右栏选中那条记录的完整信息。
 *
 * 外壳照 `.jse-modal` 那一层（实色铺满、44px 标题栏、「完成」+ Esc），但**没有复用**
 * `ScriptEditorModal.vue` —— 它模板里写死了 `ScriptEditorFrame` 与 `aria-label`，
 * 塞不进两栏内容；加 slot 会动到正在用的脚本编辑器，所以这里另写一个薄组件。
 *
 * 主从布局与「信息卡拓展 / 会话记录」同一套语言：左选一条，右看全字段。
 * ⚠️ 两处滚动都靠 `min-height: 0` 的高度链，少一环就退化成整页滚。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { save } from '@tauri-apps/plugin-dialog'
import { useStatusLogStore, EXPORT_LIMIT } from '../stores/statusLog'
import {
  STAGE_EXPLAIN,
  STAGE_LABELS,
  STAGE_OPTIONS,
  formatLogClock,
  formatLogClockMillis,
  logDetailNote,
  logHeadline,
  logPhaseChange,
  logTone,
  logWireLine,
  reasonExplain,
  LOG_FIELD_ORDER,
  type LogStageWire,
  type StatusLogEntry,
  type StatusLogQuery,
} from '../lib/status-log'
import BaseButton from '@/components/ui/BaseButton.vue'

const props = defineProps<{
  /** 会话 key（`provider:session_id`）：浮层只看这一条会话。 */
  sessionKey: string
}>()

const emit = defineEmits<{
  /** 请求关闭浮层（「完成」或 Esc）。 */
  close: []
}>()

const log = useStatusLogStore()

/** 环节过滤（null = 全部环节）。 */
const stage = ref<LogStageWire | null>(null)

/** 只看「没落成灯变化」的那些（被拒、被丢、被摘、只记账）。 */
const onlyRejected = ref(false)

/** 右栏当前展示的记录游标号（列表行的唯一键）。 */
const selectedSeq = ref<number | null>(null)

/** 行内反馈（导出 / 清空的后果）；一律 `role="status"`，本项目禁止 alert/confirm。 */
const feedback = ref('')

/** 「清空日志」的两步确认态。 */
const pendingClear = ref(false)

/** 当前过滤条件（导出与刷新共用一份，避免两处各拼一套）。 */
const query = computed<StatusLogQuery>(() => ({
  key: props.sessionKey,
  limit: 200,
  stage: stage.value,
  only_rejected: onlyRejected.value,
}))

/** 左栏的行（store 里已是新→旧）。 */
const rows = computed(() => log.entries)

/** 选中的那条：按游标现算，列表会因广播而变化。 */
const selected = computed<StatusLogEntry | null>(
  () => rows.value.find(entry => entry.seq === selectedSeq.value) ?? null
)

/** 同一条信号（同 `trace_id`）的其它行：后端给的是新→旧，这里翻成正序好顺着读。 */
const traceSiblings = computed(() => {
  const trace = selected.value?.trace_id
  if (!trace) return []
  return rows.value.filter(entry => entry.trace_id === trace && entry.seq !== selected.value?.seq).reverse()
})

/** 右栏字段表：只列有值的字段（缺省字段在后端是 `skip_serializing_if`，显示成空反而误导）。 */
const fields = computed(() => {
  const entry = selected.value
  if (!entry) return []
  return LOG_FIELD_ORDER.map(({ key, label }) => ({ label, value: entry[key] })).filter(
    item => item.value !== undefined && item.value !== null && item.value !== ''
  )
})

/** 按当前条件重新拉一页，并把选中落到第一行（换过滤后旧选中可能已经不在结果里）。 */
async function reload() {
  await log.refresh(query.value)
  selectedSeq.value = log.entries[0]?.seq ?? null
}

// 选中会话变化 = 换一条链路看，重新拉并清空反馈。
watch(
  () => props.sessionKey,
  () => {
    feedback.value = ''
    pendingClear.value = false
    void reload()
  },
  { immediate: true }
)

/** 点一行：只改右栏，不动列表（列表跟着跳会把人正看的那一段挤走）。 */
function pick(entry: StatusLogEntry) {
  selectedSeq.value = entry.seq
  feedback.value = ''
}

/** 恢复跟随：把暂停期间攒的行一次补上。 */
async function resume() {
  await log.resumeFollowing(query.value)
}

/**
 * 「暂停跟随」这个勾是 `following` 的反面：勾上 = 停下不动。
 *
 * ⚠️ 不能直接把 `log.following` 绑到复选框上 —— 那样「勾着」反而表示在跟随，
 * 点一下界面就停、再点一下才动，语义正好倒过来。取消勾选时顺手把攒的行补回来。
 */
const paused = computed({
  get: () => !log.following,
  set: async (value: boolean) => {
    if (value) {
      log.following = false
      return
    }
    await resume()
  },
})

/**
 * 导出：先问用户要写到哪（原生另存为），取消就不发命令。
 *
 * ⚠️ 导出内容含提问摘录（明细级），所以这里给一次明示；风险常驻文案在设置卡上已经写过，
 * 但「要出网/交给别人」这个动作发生在这一刻。
 */
async function exportNow() {
  feedback.value = ''
  const target = await save({
    title: '导出转灯日志',
    defaultPath: `ufo-status-log-${new Date().toISOString().slice(0, 10)}.jsonl`,
    filters: [{ name: 'JSONL', extensions: ['jsonl', 'log', 'txt'] }],
  })
  if (!target) return
  try {
    const result = await log.exportTo(target, { ...query.value, limit: EXPORT_LIMIT })
    feedback.value = `已导出 ${result.entries} 条 → ${result.path}`
  } catch (error) {
    feedback.value = `导出失败：${error}`
  }
}

/** 清空日志：两步确认，且只删日志 —— 会话存档不受它影响（文案要写明，否则会被当成删记录）。 */
async function clearNow() {
  if (!pendingClear.value) {
    pendingClear.value = true
    feedback.value = '再点一次「确认清空」以删除转灯日志（不影响会话存档）'
    return
  }
  pendingClear.value = false
  try {
    await log.clear()
    selectedSeq.value = null
    feedback.value = '已清空转灯日志'
  } catch (error) {
    feedback.value = `清空失败：${error}`
  }
}

/** Esc 关闭；键盘用户的另一个出口是标题栏「完成」。 */
function onKeydown(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  event.preventDefault()
  emit('close')
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  // 浮层关掉不等于这一页关掉：条目还在，所以不动 `set_status_log_view`（由页面管）。
  pendingClear.value = false
})

/** 灯色字：「黄」「绿」这两个字染成状态灯色，但只染灯态，不染提问原文里的字。 */
const PHASE_CHARS: Record<string, string> = { 黄: 'lg-light--yellow', 绿: 'lg-light--green', 红: 'lg-light--red' }

/** 把「绿→黄」这种灯态串渲染成带类的片段；不在表里的字原样输出，不猜颜色。 */
function phaseParts(text: string) {
  return [...text].map(char => ({ char, cls: PHASE_CHARS[char] }))
}

/** 行的时间列（列表用秒，右侧详情用毫秒避免同秒内看不出先后）。 */
function clock(tsMs: number) {
  return formatLogClock(tsMs)
}
</script>

<template>
  <Teleport to="body">
    <div class="lg-modal" role="dialog" aria-modal="true" aria-label="转灯日志完整链路" @wheel.stop>
      <header class="lg-modal__bar">
        <h2 class="lg-modal__title">转灯日志 · {{ sessionKey }}</h2>
        <p class="lg-modal__hint">
          左列本会话的裁决流水（新→旧），点一行在右列看它的全部字段
        </p>
        <BaseButton variant="primary" size="sm" @click="emit('close')">完成</BaseButton>
      </header>

      <div class="lg-modal__body">
        <div class="lg-filters">
          <label class="lg-filters__label">
            环节
            <select v-model="stage" class="lg-filters__select" @change="reload">
              <option v-for="option in STAGE_OPTIONS" :key="String(option.value)" :value="option.value">
                {{ option.label }}
              </option>
            </select>
          </label>
          <label class="lg-check"><input v-model="onlyRejected" type="checkbox" @change="reload" />只看被拒 / 被丢</label>
          <label class="lg-check"><input v-model="paused" type="checkbox" />暂停跟随</label>
          <button v-if="!log.following && log.pending" type="button" class="lg-chip" @click="resume">
            攒了 {{ log.pending }} 条，跟回最新
          </button>
          <span class="lg-filters__count">
            {{ rows.length }} 条{{ log.dropped ? ` · 环形缓冲已挤掉 ${log.dropped} 条` : '' }}
          </span>
        </div>

        <div class="lg-split">
          <!-- 左：流水 -->
          <ol class="lg-rows">
            <li v-for="entry in rows" :key="entry.seq">
              <button
                type="button"
                class="lg-row"
                :class="[`lg-row--${logTone(entry)}`, { 'lg-row--current': entry.seq === selectedSeq }]"
                :aria-current="entry.seq === selectedSeq ? 'true' : undefined"
                @click="pick(entry)"
              >
                <span class="lg-row__line">
                  <span class="lg-row__time">{{ clock(entry.ts_ms) }}</span>
                  <span class="lg-row__stage">{{ STAGE_LABELS[entry.stage] }}</span>
                  <span v-if="entry.seq" class="lg-row__seq">seq {{ entry.seq }}</span>
                </span>
                <span class="lg-row__msg">
                  <b>{{ entry.reason ?? STAGE_LABELS[entry.stage] }}</b>
                  · {{ logHeadline(entry) }}
                  <template v-if="logPhaseChange(entry)">
                    ·
                    <template v-for="part in phaseParts(logPhaseChange(entry))" :key="part.char">
                      <span :class="part.cls">{{ part.char }}</span>
                    </template>
                  </template>
                  <span v-if="entry.trace_id" class="lg-row__trace">{{ entry.trace_id }}</span>
                </span>
              </button>
            </li>
            <li v-if="!rows.length" class="lg-rows__empty">
              {{ log.loaded ? '这个会话在当前过滤条件下没有记录。摘掉过滤看看全量。' : '正在读取转灯日志…' }}
            </li>
            <li v-else-if="log.canPageBack">
              <BaseButton variant="ghost" size="sm" :disabled="log.loadingMore" @click="log.loadOlder(query)">
                {{ log.loadingMore ? '读取中…' : '看更早的' }}
              </BaseButton>
            </li>
          </ol>

          <!-- 右：选中那条的完整信息 -->
          <section class="lg-recard" aria-label="选中记录的完整信息">
            <template v-if="selected">
              <div class="lg-recard__head">
                <span class="lg-row__stage">{{ STAGE_LABELS[selected.stage] }}</span>
                <b>{{ selected.reason ?? '—' }}</b>
                <span class="lg-recard__clock">{{ formatLogClockMillis(selected.ts_ms) }}</span>
              </div>
              <p class="lg-recard__lead">{{ reasonExplain(selected.reason) || STAGE_EXPLAIN[selected.stage] }}</p>

              <dl class="lg-rec__fields">
                <template v-for="field in fields" :key="field.label">
                  <dt>{{ field.label }}</dt>
                  <dd>{{ field.value }}</dd>
                </template>
                <dt>环节说明</dt>
                <dd>{{ STAGE_EXPLAIN[selected.stage] }}</dd>
              </dl>

              <template v-if="selected.detail">
                <p class="lg-rec__sub">detail（白名单标量）</p>
                <pre class="lg-rec__raw">{{ JSON.stringify(selected.detail) }}</pre>
              </template>

              <template v-if="traceSiblings.length">
                <p class="lg-rec__sub">同一条信号（{{ selected.trace_id }}）的其它 {{ traceSiblings.length }} 行 · 正序</p>
                <ol class="lg-chain">
                  <li v-for="sibling in traceSiblings" :key="sibling.seq" class="lg-chain__item">
                    <span class="lg-row__time">{{ formatLogClockMillis(sibling.ts_ms) }}</span>
                    <span class="lg-row__stage">{{ STAGE_LABELS[sibling.stage] }}</span>
                    <span>
                      <b>{{ sibling.reason ?? '—' }}</b>
                      <span class="lg-chain__why">{{ logDetailNote(sibling) }}</span>
                    </span>
                  </li>
                </ol>
              </template>

              <p class="lg-rec__sub">线协议一行（内存游标 seq 不在磁盘行里，这里按字段重建）</p>
              <pre class="lg-rec__raw">{{ logWireLine(selected) }}</pre>
            </template>
            <p v-else class="lg-recard__placeholder">先在左列选一条记录。</p>
          </section>
        </div>

        <div class="lg-foot">
          <BaseButton variant="ghost" size="sm" @click="exportNow">导出当前条件</BaseButton>
          <BaseButton variant="danger" size="sm" :armed="pendingClear" @click="clearNow">
            {{ pendingClear ? '确认清空' : '清空日志' }}
          </BaseButton>
          <p v-if="feedback" class="lg-foot__status" role="status">{{ feedback }}</p>
          <p class="lg-dir">{{ log.dir }}</p>
        </div>
      </div>
    </div>
  </Teleport>
</template>
