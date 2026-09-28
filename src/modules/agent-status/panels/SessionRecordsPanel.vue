<script setup lang="ts">
/**
 * 「会话记录」面板：左侧 SQLite 存档列表，右侧选中会话的详情。
 *
 * 行来自跨重启持久化的会话存档（`session_store.rs`）；每行的「存活」标记按 key
 * join 内存快照（`agentStatus`）现算 —— 存活态不进存档。
 * 删除/清空都只动归档：⚠️ 仍在运行的会话会在下一个 hook 事件后被重新刷回列表，
 * 这是设计行为，面板文案与提示都按此口径写。
 *
 * 不使用 window.alert / window.confirm：本应用装了 tauri-plugin-prevent-default，
 * 原生对话框可能被拦掉，所以反馈一律走行内提示（role="status"）。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { formatTokenCount, formatTokenMillions, tokenTotal } from '../lib/agent-status'
import { providerFallbackMark, providerMeta } from '../lib/provider-registry'
import { formatRecordTime, recordEndpoint, recordLocalNote, type SessionRecord } from '../lib/session-records'
import { useAgentStatusStore } from '../stores/agentStatus'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import { useSessionRecordsStore } from '../stores/sessionRecords'
import { useStatusLogStore } from '../stores/statusLog'
import { agentStatusSettings } from '../settings'
import StatusLogFeed from './StatusLogFeed.vue'
import StatusLogSettingsCard from './StatusLogSettingsCard.vue'
import BaseButton from '@/components/ui/BaseButton.vue'
import closeIcon from '@/assets/icon/close.svg'

const records = useSessionRecordsStore()
const agentStatus = useAgentStatusStore()
const modules = useFloatingModulesStore()
const log = useStatusLogStore()

/** 当前选中的存档 key；null 表示还没有记录可选。 */
const selectedKey = ref<string | null>(null)

/** 待二次确认删除的存档 key（点一次移除 → 按钮变「确认」，再点才真删）。 */
const pendingRemoval = ref<string | null>(null)

/** 是否已进入「确认清空全部」态（按钮文案变化，再点才真清）。 */
const pendingClearAll = ref(false)

/** 面板内的行内错误/提示文案；空串表示无。 */
const feedback = ref('')

/** 「清除全部」成功后的提示原文；行又长回来时要据此识别并撤掉这句已过时的成功提示。 */
const CLEARED_FEEDBACK = '已清除全部会话记录'

/** 此刻还活着的会话 key 集合：按它给存档行打「存活」标记。 */
const liveKeys = computed(() => new Set(agentStatus.sessions.map(session => session.key)))

/** 存活提醒原文：显示在详情标题旁一行，放不下走省略号，完整原文留作 `title`。 */
const LIVE_NOTE = '该会话此刻仍在运行：清除/移除只删除归档，它会在下一个事件后被重新记录。'

/** 选中项；列表会因后端刷盘而变化，故按 key 现算而非缓存对象。 */
const selected = computed<SessionRecord | null>(
  () => records.records.find(record => record.key === selectedKey.value) ?? null
)

// 选中项被删除、或首次采集到数据时，把选中态落到第一条 —— 右侧不应停在已消失的 key 上。
// 同时清掉两处待确认态：否则「确认」按钮可能指着一个已经不存在的行，等同一 key 将来被
// 重新刷回时会变成一次点击就删。
watch(
  () => records.records,
  list => {
    pendingRemoval.value = null
    // 列表一变（含自己清空 / 后端刷盘），旧的「确认清除全部」armed 态即失效。
    pendingClearAll.value = false
    // 行又长回来了（存活会话被重新记录）：清掉「已清除全部」这句已经过时的成功提示。
    if (list.length && feedback.value === CLEARED_FEEDBACK) {
      feedback.value = ''
    }
    if (!list.some(record => record.key === selectedKey.value)) {
      selectedKey.value = list[0]?.key ?? null
    }
  },
  { immediate: true }
)

onMounted(() => {
  // 本面板自带一份订阅，避免依赖 App.vue 的启动顺序；store 内部幂等。
  // 存活标记要的 agentStatus 快照由 App.vue 对三窗口统一启动，这里不再重复。
  void records.start()
  // 转灯日志只在这一页挂着的时候才让后端广播（`set_status_log_view`），离开就停。
  void log.start()
})

onBeforeUnmount(() => {
  log.stop()
})

/**
 * 列表行 + 各自的平台元信息，一次算好。
 *
 * 未登记平台要在「图标」与「首字符徽标」两种节点间分流，模板里现查两次注册表拿不到类型收窄。
 */
const recordRows = computed(() => records.records.map(record => ({ record, meta: providerMeta(record.provider) })))

/** 按本组件配置里的单位格式化 token 总量；无统计值返回空串，由调用方显 `—` 占位。 */
function tokenDisplay(tokens: SessionRecord['tokens']): string {
  if (!tokens) return ''
  const total = tokenTotal(tokens)
  return agentStatusSettings(modules.config).token_unit === 'm_tokens'
    ? formatTokenMillions(total)
    : formatTokenCount(total)
}

/**
 * Endpoint 行的完整原文（反查出的上游 + 经手的本地代理），给该行的 `title`。
 *
 * 行本身只占一行（见 `.sr-detail__fields dd` 的 `truncate`）：字段表多出一行，
 * `flex-1` 的「最近提问」就少一行 —— 详情卡里切换会话时整块跳高的成因之一。
 */
function endpointTitle(record: SessionRecord): string {
  const endpoint = recordEndpoint(record)
  const note = recordLocalNote(record)
  if (!endpoint) return '—'
  return note ? `${endpoint}（经本地代理 ${note}）` : endpoint
}

/**
 * 移除一条存档：第一次点击只进入待确认态，第二次才真删 —— 破坏性操作不一步到位。
 * 点别的行会自动把 armed 态转移到那一行；删除失败时错误进 `feedback`。
 */
function onRemove(record: SessionRecord) {
  if (pendingRemoval.value !== record.key) {
    pendingRemoval.value = record.key
    feedback.value = `再点一次「确认」以移除 ${record.session_id.slice(0, 8)} 的记录`
    return
  }
  pendingRemoval.value = null
  void records
    .remove(record.key)
    .then(() => {
      feedback.value = ''
    })
    .catch(error => {
      feedback.value = `移除失败：${error}`
    })
}

/** 两步确认的「清除全部」；只删归档，仍在运行的会话之后会重新出现（文案已言明）。 */
function onClearAll() {
  if (!pendingClearAll.value) {
    pendingClearAll.value = true
    feedback.value = '再点一次「确认清除」以删除全部会话记录（仍在运行的会话之后会重新出现）'
    return
  }
  pendingClearAll.value = false
  void records
    .clear()
    .then(() => {
      feedback.value = CLEARED_FEEDBACK
    })
    .catch(error => {
      feedback.value = `清除失败：${error}`
    })
}
</script>

<template>
  <div class="sr-page">
    <!-- 顶部：占满宽度的「转灯日志」设置卡（只有一个 switch）-->
    <StatusLogSettingsCard />

    <div class="sr-panel">
    <section class="ufo-card sr-list-card" aria-labelledby="sr-list-title">
      <div class="ufo-card__header">
        <h2 id="sr-list-title" class="ufo-card__title">会话记录</h2>
        <span class="ufo-card__hint">{{ records.records.length }} 条</span>
      </div>

      <p v-if="!records.loaded" class="sr-list__empty">正在读取会话记录…</p>
      <p v-else-if="records.connection === 'degraded'" class="sr-list__empty">
        暂时读不到本地会话存档（IPC 或数据库不可用），请重启应用。
      </p>
      <p v-else-if="!records.records.length" class="sr-list__empty">
        还没有会话记录。会话活动后会自动存档，重启或 --resume 后也能恢复。
      </p>
      <ul v-else class="sr-list">
        <li v-for="{ record, meta } in recordRows" :key="record.key" class="sr-list__item">
          <button
            type="button"
            class="sr-list__row"
            :class="{ 'sr-list__row--active': record.key === selectedKey }"
            :aria-current="record.key === selectedKey ? 'true' : undefined"
            @click="selectedKey = record.key"
          >
            <span class="sr-list__line">
              <!-- 平台图标与是否反相来自 provider 注册表；未登记平台退化成首字符徽标。 -->
              <img
                v-if="meta.icon"
                :src="meta.icon"
                :alt="meta.label"
                class="sr-list__icon"
                :class="{ 'ufo-mono-icon': meta.mono }"
              />
              <span v-else class="sr-list__icon sr-list__icon--fallback" aria-hidden="true">{{
                providerFallbackMark(record.provider)
              }}</span>
              <span v-if="liveKeys.has(record.key)" class="sr-list__live" aria-hidden="true" />
              <span class="sr-list__id">{{ record.session_id.slice(0, 8) }}</span>
              <span class="sr-list__time">{{ formatRecordTime(record.last_seen_at_ms) }}</span>
            </span>
            <span class="sr-list__prompt">{{ record.prompt || '（无提问记录）' }}</span>
          </button>
          <button
            type="button"
            class="ep-list__remove"
            :class="{ 'ep-list__remove--armed': pendingRemoval === record.key }"
            :aria-label="
              pendingRemoval === record.key
                ? `确认移除 ${record.session_id.slice(0, 8)} 的记录`
                : `移除 ${record.session_id.slice(0, 8)} 的记录`
            "
            @click="onRemove(record)"
          >
            <span v-if="pendingRemoval === record.key" class="ep-list__remove-text">确认</span>
            <img v-else :src="closeIcon" alt="" class="ufo-mono-icon" aria-hidden="true" />
          </button>
        </li>
      </ul>

      <div class="sr-actions">
        <BaseButton variant="danger" :armed="pendingClearAll" :disabled="!records.records.length" @click="onClearAll">
          {{ pendingClearAll ? '确认清除' : '清除全部' }}
        </BaseButton>
      </div>
      <p v-if="feedback" class="sr-list__feedback" role="status">{{ feedback }}</p>
    </section>

    <section class="ufo-card sr-detail" aria-labelledby="sr-detail-title">
      <div class="ufo-card__header">
        <h2 id="sr-detail-title" class="ufo-card__title">记录详情</h2>
        <!-- 存活提醒挂在标题旁，不放正文：正文里多出一行会把 flex-1 的提问块压矮，
             切换选中会话（存活 ↔ 已归档）时整卡元素跟着跳高。
             ⚠️ 它是标题行的直接子节点，不能包进 `.ufo-heading-row` —— 那个容器允许换行，
             提醒一掉到第二行，标题就长高一行，等于把跳高从正文搬到了卡头。 -->
        <span v-if="selected && liveKeys.has(selected.key)" class="sr-detail__live" :title="LIVE_NOTE">
          {{ LIVE_NOTE }}
        </span>
        <span v-if="selected" class="ufo-card__hint">
          {{ providerMeta(selected.provider).label }} · {{ selected.session_id.slice(0, 12) }}
        </span>
      </div>
      <template v-if="selected">
        <dl class="sr-detail__fields">
          <div>
            <dt>状态</dt>
            <dd>{{ liveKeys.has(selected.key) ? '存活（灯亮着）' : '已归档' }}</dd>
          </div>
          <div>
            <dt>Endpoint</dt>
            <!-- ⚠️ 值只占一行（`truncate`）：字段表多一行 = 提问块少一行。
                 完整原文（上游 + 本地代理地址）留在 `title` 里。 -->
            <dd :title="endpointTitle(selected)">
              {{ recordEndpoint(selected) || '—' }}
              <span
                v-if="recordLocalNote(selected)"
                class="sr-detail__endpoint-local"
                :title="`经本地代理 ${recordLocalNote(selected)}`"
              >
                {{ recordLocalNote(selected) }}
              </span>
            </dd>
          </div>
          <!-- Token 行常驻、没值时显 `—`：以前这里是 v-if，有/无 token 的会话之间字段表会差一行，
               把 flex-1 的提问块顶得忽高忽低。 -->
          <div>
            <dt>Token 累计</dt>
            <dd>{{ tokenDisplay(selected.tokens) || '—' }}</dd>
          </div>
          <div>
            <dt>最后活动</dt>
            <dd>{{ formatRecordTime(selected.last_seen_at_ms) || '—' }}</dd>
          </div>
          <div>
            <dt>创建时间</dt>
            <dd>{{ formatRecordTime(selected.started_at_ms) || '—' }}</dd>
          </div>
        </dl>
        <div class="sr-detail__prompt">
          <span class="sr-detail__prompt-label">最近提问</span>
          <p class="sr-detail__prompt-text">{{ selected.prompt || '（无提问记录）' }}</p>
        </div>
      </template>
      <p v-else class="sr-detail__placeholder">先在左侧选择一条会话记录。</p>

      <!-- 末尾一项：转灯日志（折叠态 + 导出本会话 + 展开整窗浮层）。
           上面那些存档字段一个都没动 —— 关掉日志它们也照旧显示。 -->
      <StatusLogFeed :session-key="selectedKey ?? ''" />
    </section>
    </div>
  </div>
</template>
