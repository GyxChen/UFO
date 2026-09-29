<script setup lang="ts">
/**
 * 转灯日志的整窗浮层：左栏本会话流水，右栏选中那条记录的完整信息。
 *
 * 外壳照 `.jse-modal` 那一层（实色铺满、44px 标题栏、「完成」+ Esc），但**没有复用**
 * `ScriptEditorModal.vue` —— 它模板里写死了 `ScriptEditorFrame` 与 `aria-label`，
 * 塞不进两栏内容；加 slot 会动到正在用的脚本编辑器，所以这里另写一个薄组件。
 *
 * ⚠️ 浮层的宿主是主窗的 `#main-content`（不是 `body`）：那是标题栏以下、含两级 tabbar 的
 * 一整块，所以顶边落在关闭按钮下面 —— 展开时盖住 tabbar 是允许的，圆角由 `.main-shell` 裁
 * （为什么不能挂 body，见模板里的注释）。
 *
 * 主从布局与「信息卡拓展 / 会话记录」同一套语言：左选一条，右看全字段。
 * ⚠️ 两处滚动都靠 `min-height: 0` 的高度链，少一环就退化成整页滚。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { save } from '@tauri-apps/plugin-dialog'
import { useVirtualizer } from '@tanstack/vue-virtual'
import { useStatusLogStore } from '../stores/statusLog'
import {
  STAGE_EXPLAIN,
  STAGE_LABELS,
  STAGE_OPTIONS,
  formatLogClock,
  formatLogClockMillis,
  logCountLabel,
  logDetailNote,
  logHeadline,
  logPhaseChange,
  logTone,
  logWireLine,
  reasonExplain,
  LOG_OVERSCAN_ROWS,
  LOG_ROW_HEIGHT_PX,
  LOG_FIELD_ORDER,
  type LogStageWire,
  type StatusLogEntry,
  type StatusLogQuery,
} from '../lib/status-log'
import BaseButton from '@/components/ui/BaseButton.vue'
import BaseLoadingOverlay from '@/components/ui/BaseLoadingOverlay.vue'

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

/** 右栏当前展示的那条的盘上身份（列表行的唯一键）。 */
const selectedId = ref<string | null>(null)

/** 行内反馈（导出 / 清空的后果）；一律 `role="status"`，本项目禁止 alert/confirm。 */
const feedback = ref('')

/** 「清空日志」的两步确认态。 */
const pendingClear = ref(false)

/**
 * 导出是否在飞。
 *
 * 后端现在要扫整个日志目录（可以被配到几百 MiB）再逐行写出，秒级到十几秒都可能 ——
 * 所以这段期间挂遮罩、按钮 disable：重复点同一个导出钮只会多打一次全目录扫描。
 */
const exporting = ref(false)

/** 一页多少条：与 Rust `default_limit` 同值（那边缺省也是 200），翻页每次取这么多。 */
const PAGE_LIMIT = 200

/** 当前过滤条件（刷新、计数与导出共用一份，避免两处各拼一套）。 */
const filters = computed<StatusLogQuery>(() => ({
  key: props.sessionKey,
  stage: stage.value,
  only_rejected: onlyRejected.value,
}))

/** 分页查询：过滤条件 + 一页大小（导出与计数不吃 `limit`）。 */
const query = computed<StatusLogQuery>(() => ({ ...filters.value, limit: PAGE_LIMIT }))

/** 左栏的行（store 里已是新→旧，且只包含「用户真翻过的页」）。 */
const rows = computed(() => log.entries)

/** 滚动容器：既是虚拟列表的测量对象，也是「是否钉顶」的判据来源。 */
const scroller = ref<HTMLElement | null>(null)

/**
 * 左栏流水的虚拟列表用 `@tanstack/vue-virtual`（无样式、保留我们的 `role=list` 结构）。
 *
 * 为什么不自己算：上一版把「该渲染哪几行」写成一次除法 + `<ol>` 的 padding 垫高，
 * ⚠️ 而 Tailwind 全局 `box-sizing: border-box` —— 垫高一旦超过可视高，滚动容器会被
 * 自己的 padding 撑到整表高度（实测 `clientHeight` = 5910px），内部滚动当场失效，
 * 「看更早的」和整窗底栏被整页滚走。库把条目做成绝对定位 + `translateY`，垫高是一个
 * 独立的总高元素，容器高度只由版面决定，这类问题从根上不存在。
 * 另外它用 ResizeObserver 观察滚动元素，容器一变高就重算 —— 不再有「只量一次」。
 */
const rowVirtualizer = useVirtualizer(
  computed(() => ({
    count: rows.value.length,
    getScrollElement: () => scroller.value,
    // 行由 CSS 钉成等高（`--lg-row-h`，与这里的常量由 CSS 护栏对撞），所以只需要估算值。
    estimateSize: () => LOG_ROW_HEIGHT_PX,
    overscan: LOG_OVERSCAN_ROWS,
  }))
)

/** 该渲染的那几行（含每行的偏移与高度）与占位总高：都是库的读数。 */
const virtualRows = computed(() => rowVirtualizer.value.getVirtualItems())
const totalSize = computed(() => rowVirtualizer.value.getTotalSize())

/** 渲染窗口 = 库给的区间配上 store 里的那条记录（库只管算术，数据仍由 store 说话）。 */
const renderedRows = computed(() =>
  virtualRows.value
    .map(row => ({ top: row.start, height: row.size, key: row.key, entry: rows.value[row.index] }))
    .filter(item => item.entry !== undefined)
)

/** 滚动：只同步「钉顶」状态，偏移与可视区间由库自己监听。 */
function onScroll(event: Event) {
  const el = event.target as HTMLElement
  log.pinned = el.scrollTop <= 2
}

/** 回到顶部：换过滤条件与「跟回最新」都要真的把滚动位置带回去，否则视图停在旧偏移上。 */
function scrollToTop() {
  const el = scroller.value
  if (el) el.scrollTop = 0
  log.pinned = true
}

/** 顶栏计数：按盘精确数 + 它是截至哪一刻的数；不再拿「已载入多少行」冒充总数。 */
const countLabel = computed(() => logCountLabel('共', log.count, log.countState))

/** 选中的那条：按盘上身份从已载入的行里现找（列表会因广播与翻页而变化）。 */
const selected = computed<StatusLogEntry | null>(
  () => rows.value.find(entry => entry.id === selectedId.value) ?? null
)

/**
 * 同一条信号（同 `trace_id`）的其它行：**独立按 trace_id 查盘**，不从 `rows` 里筛。
 *
 * 两个理由：① 按盘分页后 `rows` 只是已载入的那一段，筛不出完整链路；
 * ② 这边勾了「只看被拒」时，链路里被过滤掉的兄弟行也该照给 —— 这一栏的意义就是看完整链路。
 */
const traceSiblings = ref<StatusLogEntry[]>([])

/** 重读选中那条的完整链路（读不到就静默清空：链路栏空着不影响主视图）。 */
async function loadTrace(entry: StatusLogEntry | null) {
  const trace = entry?.trace_id
  if (!trace) {
    traceSiblings.value = []
    return
  }
  const found = await log.queryTrace(trace)
  // 后端给的是新→旧，右栏要正序顺着读；选中那条自己不算「其它行」。
  traceSiblings.value = found.filter(item => item.id !== entry.id).reverse()
}

/** 右栏字段表：只列有值的字段（缺省字段在后端是 `skip_serializing_if`，显示成空反而误导）。 */
const fields = computed(() => {
  const entry = selected.value
  if (!entry) return []
  return LOG_FIELD_ORDER.map(({ key, label }) => ({ label, value: entry[key] })).filter(
    item => item.value !== undefined && item.value !== null && item.value !== ''
  )
})

/** 按当前条件重新拉一页 + 重算计数，并把选中落到第一行（换过滤后旧选中可能已不在结果里）。 */
async function reload() {
  await log.refresh(query.value)
  scrollToTop()
  selectedId.value = log.entries[0]?.id ?? null
  await loadTrace(selected.value)
  // 计数与列表同一套判据（同一条会话 + 同一组过滤），否则两处数字会互相打脸。
  await log.refreshCount(query.value, JSON.stringify(query.value))
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
  selectedId.value = entry.id
  feedback.value = ''
}

/** 恢复跟随：把暂停期间攒的行一次补上。 */
async function resume() {
  await log.resumeFollowing(query.value)
  scrollToTop()
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
 * ⚠️ 查询里**不传 limit**：导出的语义是「按当前条件把盘上命中的全部带走」，条数上限由
 * 目录的保留策略决定，而不是由内存环形容量决定。
 */
async function exportNow() {
  if (exporting.value) return
  feedback.value = ''
  const target = await save({
    title: '导出转灯日志',
    defaultPath: `ufo-status-log-${new Date().toISOString().slice(0, 10)}.jsonl`,
    filters: [{ name: 'JSONL', extensions: ['jsonl', 'log', 'txt'] }],
  })
  if (!target) return
  exporting.value = true
  try {
    const result = await log.exportTo(target, {
      key: filters.value.key,
      stage: filters.value.stage,
      only_rejected: filters.value.only_rejected,
    })
    // 被预算截断时不许只报条数：那会让人以为拿到的是全部。
    feedback.value = result.stopped_by_budget
      ? `目录过大，导出被扫描预算截断（只写了 ${result.entries} 条，不完整）→ ${result.path}`
      : `已导出 ${result.entries} 条（按当前条件扫过盘上全部命中）→ ${result.path}`
  } catch (error) {
    feedback.value = `导出失败：${error}`
  } finally {
    exporting.value = false
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
    selectedId.value = null
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

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
})
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

/** 行身份 `日.序号.偏移` → 只留「文件序号@偏移」这一小段，标在行尾当坐标提示。 */
function positionOf(id: string): string {
  const parts = id.split('.')
  return parts.length >= 3 ? `${parts[1]}@${parts[2]}` : id
}
</script>

<template>
  <!--
    宿主是主窗的浮层区 `#main-content`（标题栏以下、含两级 tabbar 那一整块），不是 `body`：
    挂在 body 上时铺的是视口 —— 实测既压住标题栏，方角又伸出窗口圆角之外。
    挂进宿主后顶边正好落在关闭按钮下面：展开时盖住 tabbar 是允许的（本层有「完成」与 Esc），
    但标题栏要留着 —— 拖窗与「隐藏到托盘」在浮层开着时也得能用。
  -->
  <Teleport to="#main-content">
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
          <span class="lg-filters__count">{{ countLabel }}</span>
        </div>

        <div class="lg-split">
          <!--
            左：流水。虚拟列表只渲染「视口 ±overscan」那几十行，总高由 `.lg-rows__sizer`
            这一个占位元素撑出来，条目绝对定位 + `translateY` —— 容器自己的高度只由版面决定，
            不会被内容或垫高顶大（这是上一版用 padding 垫高踩的坑，见 `rowVirtualizer` 上方注释）。
          -->
          <div class="lg-pane">
            <div
              ref="scroller"
              class="lg-rows"
              role="list"
              aria-label="本会话的转灯裁决流水"
              @scroll="onScroll"
            >
              <div class="lg-rows__sizer" :style="{ height: totalSize + 'px' }">
                <div
                  v-for="item in renderedRows"
                  :key="item.key"
                  class="lg-rows__cell"
                  role="listitem"
                  :style="{ transform: 'translateY(' + item.top + 'px)', height: item.height + 'px' }"
                >
                  <button
                    type="button"
                    class="lg-row"
                    :class="[`lg-row--${logTone(item.entry)}`, { 'lg-row--current': item.entry.id === selectedId }]"
                    :aria-current="item.entry.id === selectedId ? 'true' : undefined"
                    @click="pick(item.entry)"
                  >
                    <span class="lg-row__line">
                      <span class="lg-row__time">{{ clock(item.entry.ts_ms) }}</span>
                      <span class="lg-row__stage">{{ STAGE_LABELS[item.entry.stage] }}</span>
                      <span class="lg-row__seq">@{{ positionOf(item.entry.id) }}</span>
                    </span>
                    <span class="lg-row__msg">
                      <b>{{ item.entry.reason ?? STAGE_LABELS[item.entry.stage] }}</b>
                      · {{ logHeadline(item.entry) }}
                      <template v-if="logPhaseChange(item.entry)">
                        ·
                        <template v-for="part in phaseParts(logPhaseChange(item.entry))" :key="part.char">
                          <span :class="part.cls">{{ part.char }}</span>
                        </template>
                      </template>
                      <span v-if="item.entry.trace_id" class="lg-row__trace">{{ item.entry.trace_id }}</span>
                    </span>
                  </button>
                </div>
              </div>
            </div>

            <!-- 空态与「看更早的」都在滚动区**外面**：混进去就成了没计入高度模型的一行。 -->
            <p v-if="!rows.length" class="lg-pane__note">
              {{ log.loaded ? '这个会话在当前过滤条件下没有记录。摘掉过滤看看全量。' : '正在读取转灯日志…' }}
            </p>
            <div v-else class="lg-pane__foot">
              <BaseButton
                v-if="log.canPageBack"
                variant="ghost"
                size="sm"
                :disabled="log.loadingMore"
                @click="log.loadOlder(query)"
              >
                {{ log.loadingMore ? '读取中…' : '看更早的' }}
              </BaseButton>
              <span v-else class="lg-pane__note">已经翻到这批日志的头部。</span>
              <span class="lg-filters__loaded">已载入 {{ rows.length }} 行</span>
            </div>
          </div>

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

              <p class="lg-rec__sub">线协议一行（按字段重建；@{{ positionOf(selected.id) }} 是它在盘上的字节坐标）</p>
              <pre class="lg-rec__raw">{{ logWireLine(selected) }}</pre>
            </template>
            <p v-else class="lg-recard__placeholder">先在左列选一条记录。</p>
          </section>
        </div>

        <div class="lg-foot">
          <BaseButton variant="ghost" size="sm" :disabled="exporting" @click="exportNow">导出当前条件</BaseButton>
          <BaseButton variant="danger" size="sm" :armed="pendingClear" :disabled="exporting" @click="clearNow">
            {{ pendingClear ? '确认清空' : '清空日志' }}
          </BaseButton>
          <p v-if="feedback" class="lg-foot__status" role="status">{{ feedback }}</p>
          <p class="lg-dir">{{ log.dir }}</p>
        </div>
      </div>

      <!--
        导出遮罩：后端要扫完整个日志目录才回，秒级到十几秒都可能。挂在这里（`.lg-modal` 是
        定位祖先）能盖住整窗，包括那两个钮 —— 扫描在飞的时候再点一次只会多打一次全目录扫描。
      -->
      <BaseLoadingOverlay :loading="exporting" text="正在扫描日志目录…" tone="solid" placement="inline" />
    </div>
  </Teleport>
</template>
