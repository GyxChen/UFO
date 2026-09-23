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
import { computed, onMounted, ref, watch } from 'vue'
import { formatTokenCount, formatTokenMillions, tokenTotal, type Provider } from '../lib/agent-status'
import { formatRecordTime, recordEndpoint, recordLocalNote, type SessionRecord } from '../lib/session-records'
import { useAgentStatusStore } from '../stores/agentStatus'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import { useSessionRecordsStore } from '../stores/sessionRecords'
import { agentStatusSettings } from '../settings'
import BaseButton from '@/components/ui/BaseButton.vue'
import claudeIcon from '@/assets/claude.svg'
import codexIcon from '@/assets/codex.svg'
import closeIcon from '@/assets/close.svg'

const records = useSessionRecordsStore()
const agentStatus = useAgentStatusStore()
const modules = useFloatingModulesStore()

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
})

/** provider 的展示名。 */
function providerLabel(provider: Provider): string {
  return provider === 'claude' ? 'Claude' : 'Codex'
}

/** 按本组件配置里的单位格式化 token 总量；无统计值时返回空串（调用方省略该行）。 */
function tokenDisplay(tokens: SessionRecord['tokens']): string {
  if (!tokens) return ''
  const total = tokenTotal(tokens)
  return agentStatusSettings(modules.config).token_unit === 'm_tokens'
    ? formatTokenMillions(total)
    : formatTokenCount(total)
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
        <li v-for="record in records.records" :key="record.key" class="sr-list__item">
          <button
            type="button"
            class="sr-list__row"
            :class="{ 'sr-list__row--active': record.key === selectedKey }"
            :aria-current="record.key === selectedKey ? 'true' : undefined"
            @click="selectedKey = record.key"
          >
            <span class="sr-list__line">
              <!-- Codex 的图标是纯白单色，浅色主题下要反相；Claude 的橙不套。 -->
              <img
                :src="record.provider === 'claude' ? claudeIcon : codexIcon"
                :alt="providerLabel(record.provider)"
                class="sr-list__icon"
                :class="{ 'ufo-mono-icon': record.provider === 'codex' }"
              />
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
        <span v-if="selected" class="ufo-card__hint">
          {{ providerLabel(selected.provider) }} · {{ selected.session_id.slice(0, 12) }}
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
            <dd>
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
          <div v-if="tokenDisplay(selected.tokens)">
            <dt>Token 累计</dt>
            <dd>{{ tokenDisplay(selected.tokens) }}</dd>
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
        <p v-if="liveKeys.has(selected.key)" class="sr-detail__note">
          该会话此刻仍在运行：清除/移除只删除归档，它会在下一个事件后被重新记录。
        </p>
      </template>
      <p v-else class="sr-detail__placeholder">先在左侧选择一条会话记录。</p>
    </section>
  </div>
</template>
