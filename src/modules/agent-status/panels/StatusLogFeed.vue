<script setup lang="ts">
/**
 * 「记录详情」末尾那一项：转灯日志的折叠视图 + 打开整窗浮层的入口。
 *
 * 折叠态只有约一行高：一条结论 + 最近两行摘要，右边两个动作（导出本会话 / 展开）。
 * 数据走 `store.refreshPeek()` 的独立查询，而不是从浮层那份 `entries` 里筛 ——
 * 浮层的过滤条件（环节 / 只看被拒）会随时变，详情条目不该跟着变空。
 *
 * ⚠️ 关闭态按已批准的 mock 画法 ②：条目**留在原处**、内容说清「没在记」。
 * 整块不显示会让人分不清「没发生」与「没记录」，那正是排障时最需要一个答案的地方。
 */
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { save } from '@tauri-apps/plugin-dialog'
import { useStatusLogStore } from '../stores/statusLog'
import {
  STAGE_LABELS,
  formatLogClock,
  logCountLabel,
  logHeadline,
  logPhaseChange,
  logTone,
  type StatusLogEntry,
} from '../lib/status-log'
import { agentStatusSettings, logSwitchOn } from '../settings'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import BaseButton from '@/components/ui/BaseButton.vue'
import BaseLoadingOverlay from '@/components/ui/BaseLoadingOverlay.vue'
import StatusLogOverlay from './StatusLogOverlay.vue'

const props = defineProps<{
  /** 会话 key（`provider:session_id`）；空串 = 还没选中任何存档。 */
  sessionKey: string
}>()

const log = useStatusLogStore()
const modules = useFloatingModulesStore()

/** 日志开关是否打开（配置由本模块解释，见 `settings.ts`）。 */
const recording = computed(() => logSwitchOn(agentStatusSettings(modules.config).log))

/** 浮层是否展开。 */
const open = ref(false)

/** 行内反馈（导出结果 / 失败原因）；本项目禁止 alert/confirm，一律 `role="status"`。 */
const feedback = ref('')

/**
 * 导出是否在飞：后端要按条件扫完整个日志目录（可能被配到几百 MiB）才回。
 * 这段期间挂遮罩并挡住按钮 —— 重复点一次就是多扫一遍全目录。
 */
const exporting = ref(false)

/**
 * 条数文案：按盘精确数 + 「截至」时刻（后端 3 秒节流 + 缓存，所以跟着 head 前进重算不心疼）。
 *
 * 查询只带 `key`：这一行说的是「这个会话在盘上一共多少条」，不该被浮层那套过滤条件影响。
 */
const countLabel = computed(() =>
  logCountLabel('本会话', log.count, log.countState)
)

/** 折叠态露出的行：第一条做结论，后两条做摘要。 */
const shown = computed(() => log.peek.slice(0, 3))

/** 结论行：最新那条的头（没有记录时整块换成空态文案）。 */
const headline = computed<StatusLogEntry | null>(() => shown.value[0] ?? null)

/** 摘要行 = 结论之后的两条。 */
const preview = computed(() => shown.value.slice(1, 3))

/**
 * 换选中会话、或广播让 `head` 前进时重读这几行。
 *
 * ⚠️ 盯 `head` 而不是自己开定时器：head 前进就意味着盘上真有新行，没动静时一次请求都不发。
 */
watch(
  () => [props.sessionKey, log.head] as const,
  async () => {
    feedback.value = ''
    await log.refreshPeek(props.sessionKey)
    const query = { key: props.sessionKey }
    await log.refreshCount(query, props.sessionKey)
  },
  { immediate: true }
)

/** 展开浮层前先确保订阅在跑（页面卸载过就得重新 attach）。 */
function expand() {
  open.value = true
}

/** 关掉浮层：右栏那份过滤视图留着不动（再开不用重拉），只清行内反馈。 */
function collapse() {
  open.value = false
  feedback.value = ''
}

/**
 * 导出这个会话的**全部**记录（按盘扫，不受内存环形容量封顶）：路径由原生另存为给出，取消即不发命令。
 *
 * ⚠️ 不传 `limit`：这条命令的语义是「这个会话在盘上有多少条就写多少条」。
 */
async function exportSession() {
  if (exporting.value) return
  feedback.value = ''
  const target = await save({
    title: '导出这个会话的转灯日志',
    defaultPath: `ufo-status-log-${props.sessionKey.replace(/[^A-Za-z0-9._-]/g, '_').slice(0, 40)}.jsonl`,
    filters: [{ name: 'JSONL', extensions: ['jsonl', 'log', 'txt'] }],
  })
  if (!target) return
  exporting.value = true
  try {
    const result = await log.exportTo(target, { key: props.sessionKey })
    // 被预算截断时不许只报条数：那会让人以为拿到的是全部。
    feedback.value = result.stopped_by_budget
      ? `目录过大，导出被截断（只写了 ${result.entries} 条，不完整）`
      : `已导出 ${result.entries} 条（盘上命中全量）`
  } catch (error) {
    feedback.value = `导出失败：${error}`
  } finally {
    exporting.value = false
  }
}

/** 浮层开着时页面被切走要一起收掉，免得留一层看不见的面板。 */
onBeforeUnmount(() => {
  open.value = false
})

/** 灯色字：只染灯态那两个字，不染提问原文里的同名字。 */
const PHASE_CHARS: Record<string, string> = { 黄: 'lg-light--yellow', 绿: 'lg-light--green', 红: 'lg-light--red' }

function phaseParts(text: string) {
  return [...text].map(char => ({ char, cls: PHASE_CHARS[char] }))
}
</script>

<template>
  <div class="lg-inline" :class="{ 'lg-inline--off': !recording }">
    <div class="lg-inline__head">
      <h4 class="lg-inline__label">转灯日志</h4>
      <span class="lg-inline__count">
        {{ recording ? countLabel : '已关闭 · 零写盘' }}
      </span>
      <span class="lg-inline__acts">
        <BaseButton
          v-if="recording && headline"
          variant="ghost"
          size="sm"
          :disabled="exporting"
          @click="exportSession"
        >
          导出本会话
        </BaseButton>
        <button
          v-if="recording && headline"
          type="button"
          class="lg-inline__expand"
          :disabled="exporting"
          title="在整窗中查看完整链路"
          aria-label="在整窗中查看完整链路"
          @click="expand"
        >
          <!-- 内联同一枚「扩大」图标（与 `ScriptEditorFrame` 用的路径一致）：
               项目里没有 `expand.svg` 这个资源，为一枚按钮新增静态资源不值得。 -->
          <svg class="lg-inline__expand-icon" viewBox="0 0 24 24" aria-hidden="true">
            <path
              fill="currentColor"
              d="M10,21V19H6.41L10.91,14.5L9.5,13.09L5,17.59V14H3V21H10M14.5,10.91L19,6.41V10H21V3H14V5H17.59L13.09,9.5L14.5,10.91Z"
            />
          </svg>
        </button>
      </span>
    </div>

    <!-- 关着：一行说明就够，别把折叠态撑成一张卡 -->
    <p v-if="!recording" class="lg-inline__off">
      打开后这里会出现每次转灯裁决的链路；现在<b>什么都不记</b>，上面的会话存档不受影响。
    </p>

    <template v-else-if="headline">
      <p class="lg-inline__sum">
        <b>{{ formatLogClock(headline.ts_ms) }} </b>
        <b>{{ logHeadline(headline) }}</b>
        <template v-if="logPhaseChange(headline)">
          ·
          <template v-for="part in phaseParts(logPhaseChange(headline))" :key="part.char">
            <span :class="part.cls">{{ part.char }}</span>
          </template>
        </template>
      </p>
      <ul class="lg-inline__rows">
        <li v-for="entry in preview" :key="entry.id" :data-tone="logTone(entry)">
          <span class="lg-inline__time">{{ formatLogClock(entry.ts_ms) }}</span>
          <span class="lg-row__stage">{{ STAGE_LABELS[entry.stage] }}</span>
          <em>{{ entry.reason ?? STAGE_LABELS[entry.stage] }}</em>
        </li>
      </ul>
    </template>

    <p v-else-if="!log.loaded" class="lg-inline__off">正在读取转灯日志…</p>
    <p v-else-if="log.connection === 'degraded'" class="lg-inline__off">
      暂时读不到转灯日志（IPC 不可用），重启应用可恢复。
    </p>
    <p v-else-if="sessionKey" class="lg-inline__off">这个会话还没有留下任何裁决痕迹。</p>
    <p v-else class="lg-inline__off">先在左侧选择一条会话记录。</p>

    <p v-if="feedback" class="lg-inline__feedback" role="status">{{ feedback }}</p>

    <!--
      导出遮罩：这一块的祖先是 `.lg-inline`（CSS 里给它加了 `relative`），所以只盖这一条，
      不会把整张详情卡压住 —— 用户仍能看见右边的字段表在原地。
    -->
    <BaseLoadingOverlay :loading="exporting" text="正在扫描日志目录…" tone="solid" placement="inline" />

    <StatusLogOverlay v-if="open && sessionKey" :session-key="sessionKey" @close="collapse" />
  </div>
</template>
