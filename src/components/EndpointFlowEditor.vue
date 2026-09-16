<script setup lang="ts">
/**
 * 「信息卡拓展」右侧：某个 endpoint 的流程脚本编辑器。
 *
 * 用户在这里写一段 JS（跑在后端 QuickJS 沙盒里），返回值作为 tooltip 信息卡的拓展字段。
 * 草稿从选中档案的 `extensions.flow` 派生；保存/测试/密钥操作都打到 flow 命令，成功后依赖
 * endpoint-profiles / flow-results 事件回流刷新（不自己维护副本，避免两处真相）。
 *
 * ⚠️ 反馈一律走行内 `role="status"`，不用 window.alert / window.confirm：
 * 本应用装了 tauri-plugin-prevent-default，原生对话框可能被拦掉。
 * ⚠️ 面板在主窗口内，不是悬浮窗，按钮无需 @click.stop（该约定只约束悬浮窗内的独立点击元素）。
 */
import { computed, reactive, ref, watch } from 'vue'
import type { EndpointProfile } from '@/lib/endpoint-profiles'
import { useEndpointProfilesStore } from '@/stores/endpointProfiles'
import {
  deleteFlowSecret,
  fetchFlowResults,
  flowColorClass,
  flowFromExtensions,
  runFlowTest,
  saveEndpointFlow,
  setFlowSecret,
  type FlowOutcome,
  type FlowTestResult,
} from '@/lib/flow'

const props = defineProps<{ profile: EndpointProfile | null }>()

const profiles = useEndpointProfilesStore()

/** 间隔/超时的取值范围，与后端 config.rs 的 clamp 对齐（前端只做输入引导，后端兜底）。 */
const MIN_INTERVAL_SEC = 10
const MAX_INTERVAL_SEC = 3600

/** 编辑器草稿；随选中档案切换而重置。 */
const draft = reactive({
  enabled: false,
  intervalSec: 60,
  timeoutMs: 8000,
  script: '',
})

/** 当前档案已配置的密钥名（来自 extensions，保存后由事件回流刷新）。 */
const secretNames = computed<string[]>(() => flowFromExtensions(props.profile?.extensions)?.secret_names ?? [])

/** 该 endpoint 最近一次真实运行产物（轮询结果 / 上次失败原因）；未跑过时 null。 */
const lastOutcome = ref<FlowOutcome | null>(null)

/** 拉取选中 endpoint 的运行产物（切换 endpoint 时刷新）。 */
function reloadOutcome() {
  const id = props.profile?.id
  if (!id) {
    lastOutcome.value = null
    return
  }
  void fetchFlowResults(id).then(
    (outcome) => {
      // 异步回来时可能已切到别的 endpoint：按 id 核对后再落。
      if (props.profile?.id === id) lastOutcome.value = outcome
    },
    () => {
      lastOutcome.value = null
    },
  )
}

/** 行内提示；成功/失败都写这里。 */
const feedback = ref('')
/** 最近一次「运行测试」产物（字段预览 + 日志 + 错误）。 */
const testResult = ref<FlowTestResult | null>(null)
/** 是否正在跑测试（禁用按钮，避免重复触发）。 */
const testing = ref(false)
/** 是否正在保存。 */
const saving = ref(false)

/** 新增密钥的两个输入框。 */
const secretDraft = reactive({ name: '', value: '' })
/** 待两步确认移除的密钥名。 */
const pendingSecretRemoval = ref<string | null>(null)

// 切换 endpoint（id 变化）：用该档案已存配置回填草稿，没配过就用默认值起步。
// ⚠️ 按 id 而非整个 profile 对象 watch：保存 / 增删密钥会经事件回流替换 props.profile 引用，
// 按对象 watch 会在那一刻把刚显示的「已保存」提示与测试预览冲掉。密钥列表本身走 secretNames
// （computed，读 reactive 的 extensions）自行更新，不依赖这里。
watch(
  () => props.profile?.id,
  () => {
    const stored = flowFromExtensions(props.profile?.extensions)
    draft.enabled = stored?.enabled ?? false
    draft.intervalSec = stored?.interval_sec ?? 60
    draft.timeoutMs = stored?.timeout_ms ?? 8000
    draft.script = stored?.script ?? ''
    feedback.value = ''
    testResult.value = null
    pendingSecretRemoval.value = null
    secretDraft.name = ''
    secretDraft.value = ''
    reloadOutcome()
  },
  { immediate: true },
)

/** 保存当前草稿为 endpoint 的流程配置。 */
async function onSave() {
  if (!props.profile) return
  saving.value = true
  try {
    const list = await saveEndpointFlow(props.profile.id, {
      enabled: draft.enabled,
      interval_sec: clampInt(draft.intervalSec, MIN_INTERVAL_SEC, MAX_INTERVAL_SEC),
      timeout_ms: clampInt(draft.timeoutMs, 500, 15000),
      script: draft.script,
    })
    // 后端返回新的档案列表：直接并入 store，让左侧标签即时反映「已配流程」。
    profiles.applyList(list)
    feedback.value = draft.enabled ? '已保存，会话存活时按间隔轮询' : '已保存（当前未启用）'
    // 保存会 invalidate 旧结果，清掉面板上的「上次运行」展示。
    lastOutcome.value = null
  } catch (error) {
    feedback.value = `保存失败：${error}`
  } finally {
    saving.value = false
  }
}

/** 用当前草稿同步跑一次预览（不进调度、不改持久结果）。 */
async function onTest() {
  if (!props.profile) return
  testing.value = true
  try {
    testResult.value = await runFlowTest(props.profile.id, draft.script, clampInt(draft.timeoutMs, 500, 15000))
    feedback.value = ''
  } catch (error) {
    testResult.value = null
    feedback.value = `测试失败：${error}`
  } finally {
    testing.value = false
  }
}

/** 写入一个密钥（值即时清空、永不回显）。 */
async function onAddSecret() {
  if (!props.profile) return
  const name = secretDraft.name.trim()
  const value = secretDraft.value
  if (!name || !value) {
    feedback.value = '请填写密钥名与值'
    return
  }
  try {
    await setFlowSecret(props.profile.id, name, value)
    secretDraft.name = ''
    secretDraft.value = ''
    feedback.value = `已保存密钥 ${name}`
  } catch (error) {
    feedback.value = `保存密钥失败：${error}`
  }
}

/** 两步确认删除一个密钥。 */
async function onRemoveSecret(name: string) {
  if (!props.profile) return
  if (pendingSecretRemoval.value !== name) {
    pendingSecretRemoval.value = name
    feedback.value = `再点一次「确认」以删除密钥 ${name}`
    return
  }
  pendingSecretRemoval.value = null
  try {
    await deleteFlowSecret(props.profile.id, name)
    feedback.value = `已删除密钥 ${name}`
  } catch (error) {
    feedback.value = `删除密钥失败：${error}`
  }
}

function clampInt(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min
  return Math.min(Math.max(Math.trunc(value), min), max)
}
</script>

<template>
  <section class="ufo-card ep-detail" aria-labelledby="ep-detail-title">
    <div class="ufo-card__header">
      <h2 id="ep-detail-title" class="ufo-card__title">拓展字段</h2>
      <span v-if="profile" class="ufo-card__hint ep-detail__endpoint">{{ profile.url }}</span>
    </div>

    <p v-if="!profile" class="ep-detail__placeholder">先在左侧选择一个 endpoint。</p>

    <div v-else class="flow-editor">
      <p class="flow-editor__lead">
        写一段脚本（跑在沙盒里），返回值作为该 endpoint 信息卡上的拓展字段。仅在该 endpoint
        有存活会话时按间隔轮询。
      </p>

      <div class="flow-editor__row">
        <label class="flow-editor__toggle">
          <span>启用</span>
          <label class="ufo-switch" title="启用该 endpoint 的流程脚本">
            <input v-model="draft.enabled" type="checkbox" aria-label="启用流程脚本" />
            <span class="ufo-switch__track" aria-hidden="true" />
          </label>
        </label>
        <label class="flow-editor__num">
          <span>间隔（秒）</span>
          <input
            v-model.number="draft.intervalSec"
            class="ufo-input"
            type="number"
            :min="MIN_INTERVAL_SEC"
            :max="MAX_INTERVAL_SEC"
            step="1"
          />
        </label>
        <label class="flow-editor__num">
          <span>超时（毫秒）</span>
          <input
            v-model.number="draft.timeoutMs"
            class="ufo-input"
            type="number"
            min="500"
            max="15000"
            step="500"
          />
        </label>
      </div>

      <label class="flow-editor__field">
        <span>脚本</span>
        <textarea
          v-model="draft.script"
          class="ufo-input flow-editor__script"
          spellcheck="false"
          rows="10"
          aria-label="流程脚本源码"
          placeholder="return [{ label: '余额', value: '¥12.30', color: 'green' }]"
        ></textarea>
      </label>

      <div class="flow-editor__actions">
        <button
          class="ufo-btn ufo-btn--ghost"
          type="button"
          :disabled="testing"
          @click="onTest"
        >
          {{ testing ? '运行中…' : '运行测试' }}
        </button>
        <button
          class="ufo-btn ufo-btn--primary"
          type="button"
          :disabled="saving"
          @click="onSave"
        >
          {{ saving ? '保存中…' : '保存' }}
        </button>
      </div>

      <p v-if="feedback" class="flow-editor__feedback" role="status">{{ feedback }}</p>

      <!-- 该 endpoint 最近一次后台轮询的真实产物（非本次测试预览）。 -->
      <p v-if="lastOutcome && !lastOutcome.ok" class="flow-editor__lastfail" role="status">
        上次运行失败：{{ lastOutcome.error }}
      </p>

      <div v-if="testResult" class="flow-editor__test">
        <p v-if="testResult.error" class="flow-editor__test-error">错误：{{ testResult.error }}</p>
        <ul v-if="testResult.fields.length" class="flow-editor__preview">
          <li v-for="(field, index) in testResult.fields" :key="index" class="flow-editor__preview-row">
            <span class="flow-editor__preview-label">{{ field.label }}</span>
            <span class="flow-editor__preview-value" :class="`is-${flowColorClass(field.color)}`">
              {{ field.value }}
            </span>
          </li>
        </ul>
        <p v-else-if="!testResult.error" class="flow-editor__test-empty">脚本没有返回可显示的字段。</p>
        <pre v-if="testResult.logs.length" class="flow-editor__logs">{{ testResult.logs.join('\n') }}</pre>
      </div>

      <section class="flow-editor__secrets" aria-label="密钥">
        <h3 class="flow-editor__subhead">密钥</h3>
        <p class="flow-editor__hint">
          存进系统凭据管理器，脚本用 <code>secrets.get('名字')</code> 取；值不落盘、不回显。
        </p>
        <ul v-if="secretNames.length" class="flow-editor__secret-list">
          <li v-for="name in secretNames" :key="name" class="flow-editor__secret-row">
            <span class="flow-editor__secret-name">{{ name }}</span>
            <button
              type="button"
              class="ufo-btn ufo-btn--ghost ufo-btn--sm"
              :class="{ 'ufo-btn--armed': pendingSecretRemoval === name }"
              @click="onRemoveSecret(name)"
            >
              {{ pendingSecretRemoval === name ? '确认' : '删除' }}
            </button>
          </li>
        </ul>
        <p v-else class="flow-editor__secret-empty">尚未配置密钥。</p>
        <form class="flow-editor__secret-add" @submit.prevent="onAddSecret">
          <input
            v-model="secretDraft.name"
            class="ufo-input"
            type="text"
            spellcheck="false"
            placeholder="密钥名，如 ANTHROPIC_KEY"
            aria-label="密钥名"
          />
          <input
            v-model="secretDraft.value"
            class="ufo-input"
            type="password"
            placeholder="密钥值"
            aria-label="密钥值"
            autocomplete="off"
          />
          <button class="ufo-btn ufo-btn--ghost" type="submit">保存</button>
        </form>
      </section>

      <details class="flow-editor__ref">
        <summary>API 参考</summary>
        <pre class="flow-editor__ref-body">http.get(url, opts?)        → { status, ok, headers, text(), json() }
http.post(url, opts?)       opts: { headers, body, timeoutMs }
http.request(method, url, opts?)
secrets.get(name)           取系统凭据；未配置抛异常
cache.get(name) / cache.set(name, value, ttlMs?)
format.num(n) format.pct(0.42) format.bytes(n)
ctx.endpoint { id, url }    ctx.session { provider, phase, prompt, tokens }
ctx.lastResult              上次成功返回值（脚本未改时继承）

示例：
const res = await http.get('https://api.example.com/v1/usage', {
  headers: { Authorization: `Bearer ${secrets.get('TOKEN')}` },
})
const data = res.json()
return [
  { label: '余额', value: `$${data.balance}`, color: data.balance < 1 ? 'red' : 'green' },
]</pre>
        <p class="flow-editor__hint">
          ⚠️ 脚本无法访问文件系统 / 本机与私有网段地址，`import` / `require` 不可用；出网只走 `http`。
        </p>
      </details>
    </div>
  </section>
</template>
