<script setup lang="ts">
/**
 * Agent 状态灯设置 · 常规：所有「填一下就完事」的配置，两块卡片横向并排。
 *
 * 左「行为」（时间参数与全局取值）、右「窗口格子」（各形态上限）。
 * 它们都是短表单，纵向堆叠会白白拉长页面，所以并排铺开 —— 一屏看完，不用滚。
 * 需要「占满高度 + 左右各自滚动」的两个面板（信息卡拓展 / 会话记录）才各占一个二级 tab。
 *
 * 左卡的字段是本组件自己的配置（`../settings` 读写外层的 `module_settings['agent-status']`，字段名不经过外壳）；
 * 右卡的格子上限是**整个悬浮窗**的参数，同级组件共享，因此仍由外壳 store 持有。
 */
import { computed, onMounted, ref, watch } from 'vue'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import BaseButton from '@/components/ui/BaseButton.vue'
import BaseInput from '@/components/ui/BaseInput.vue'
import BaseSegmentedControl from '@/components/ui/BaseSegmentedControl.vue'
import BaseSelect from '@/components/ui/BaseSelect.vue'
import BaseSlider from '@/components/ui/BaseSlider.vue'
import SettingsField from '@/components/ui/SettingsField.vue'
import {
  AGENT_MAX_CELL_STEPS,
  AGENT_STATUS_MODULE_ID,
  MAX_AGENT_CELLS,
  MAX_RETENTION_SECONDS,
  MAX_RUNNING_TIMEOUT_SECONDS,
  MIN_RETENTION_SECONDS,
  MIN_RUNNING_TIMEOUT_SECONDS,
  agentStatusSettings,
  fetchProxyResolvers,
  retentionMsFromSeconds,
  runningTimeoutMsFromSeconds,
  toSeconds,
  type ProxyResolverInfo,
  type TokenUnit,
} from '../settings'

const modules = useFloatingModulesStore()

/** 本组件自己的配置块（缺字段 / 非法值由 `agentStatusSettings` 回落默认并夹取）。 */
const settings = computed(() => agentStatusSettings(modules.config))

const retentionInput = ref(toSeconds(settings.value.completed_retention_ms))
const runningTimeoutInput = ref(toSeconds(settings.value.running_timeout_ms))
const tokenUnitInput = ref<TokenUnit>(settings.value.token_unit)
const proxyResolverInput = ref(settings.value.proxy_resolver)

/** 代理解析器选项：这是本组件自己的概念，所以由本组件自己向后端拉。 */
const resolvers = ref<ProxyResolverInfo[]>([])

onMounted(async () => {
  try {
    resolvers.value = await fetchProxyResolvers()
  } catch (error) {
    console.warn('proxy resolver list unavailable', error)
  }
})

/** token 单位选项；`tokens` 显示原始计数，`m_tokens` 显示百万。 */
const tokenUnitOptions: { value: TokenUnit; label: string }[] = [
  { value: 'tokens', label: 'Tokens' },
  { value: 'm_tokens', label: 'M tokens' },
]

/** 代理解析器下拉选项；只把后端 id/label 映射成通用下拉需要的结构。 */
const proxyResolverOptions = computed(() =>
  resolvers.value.map(option => ({ value: option.id, label: option.label }))
)

/**
 * 窗口格子上限：滑块按「档位下标」0..5 驱动，映射到本模块的阶梯 `AGENT_MAX_CELL_STEPS` = 1,2,3,4,6,9。
 * 配置里存的是格子数，但可能是迁移来的非档位值（如 8）—— 下标取「不大于它的最大档位」，
 * 与生效上限（`effectiveLayoutMax` 向下归档）对齐，滑块因此永远落在合法档位上。
 */
const maxCellsIndex = ref(indexForCells(settings.value.max_cells))
const currentMaxCells = computed(() => AGENT_MAX_CELL_STEPS[maxCellsIndex.value] ?? MAX_AGENT_CELLS)

/** 贴角网格标注：档位 → 行×列，放在取值旁帮用户理解这个数撑多大。 */
const cornerShapes: Record<number, string> = { 1: '1×1', 2: '1×2', 3: '1×3', 4: '2×2', 6: '2×3', 9: '3×3' }
const currentCornerShape = computed(() => cornerShapes[currentMaxCells.value] ?? '')

/** 取某个格子数对应的滑块下标（不大于它的最大档位；越界夹到两端）。 */
function indexForCells(value: number): number {
  let index = 0
  for (let i = 0; i < AGENT_MAX_CELL_STEPS.length; i += 1) {
    if ((AGENT_MAX_CELL_STEPS[i] ?? 0) <= value) index = i
  }
  return index
}

// 配置变化（别的窗口改了 / 重连后拿到配置）时同步回输入框，避免显示过期值。
watch(
  settings,
  value => {
    retentionInput.value = toSeconds(value.completed_retention_ms)
    runningTimeoutInput.value = toSeconds(value.running_timeout_ms)
    tokenUnitInput.value = value.token_unit
    proxyResolverInput.value = value.proxy_resolver
    maxCellsIndex.value = indexForCells(value.max_cells)
  },
  { immediate: true }
)

/** 更新完成灯保留时长（输入是秒，配置里存毫秒）。 */
async function updateRetention() {
  try {
    await modules.updateModuleSettings(AGENT_STATUS_MODULE_ID, {
      completed_retention_ms: retentionMsFromSeconds(retentionInput.value),
    })
  } catch {
    retentionInput.value = toSeconds(settings.value.completed_retention_ms)
  }
}

/** 更新 Running 无新 hook 超时（秒）；慢 agent 可拉长避免绿灯被误清。 */
async function updateRunningTimeout() {
  try {
    await modules.updateModuleSettings(AGENT_STATUS_MODULE_ID, {
      running_timeout_ms: runningTimeoutMsFromSeconds(runningTimeoutInput.value),
    })
  } catch {
    runningTimeoutInput.value = toSeconds(settings.value.running_timeout_ms)
  }
}

/** 滑块松手后落盘：把档位下标映射回格子数，写进本组件自己的配置块；失败 / 没变时退回已知值。 */
async function updateMaxCells() {
  const next = currentMaxCells.value
  if (next === settings.value.max_cells) {
    maxCellsIndex.value = indexForCells(settings.value.max_cells)
    return
  }
  try {
    await modules.updateModuleSettings(AGENT_STATUS_MODULE_ID, { max_cells: next })
  } catch {
    maxCellsIndex.value = indexForCells(settings.value.max_cells)
  }
}

/** 更新 token 展示单位（tokens / M tokens）。 */
async function updateTokenUnit() {
  try {
    await modules.updateModuleSettings(AGENT_STATUS_MODULE_ID, { token_unit: tokenUnitInput.value })
  } catch {
    tokenUnitInput.value = settings.value.token_unit
  }
}

/** 更新代理解析器（下拉的 value=id）；后端会即时同步给状态监测。 */
async function updateProxyResolver() {
  try {
    await modules.updateModuleSettings(AGENT_STATUS_MODULE_ID, { proxy_resolver: proxyResolverInput.value })
  } catch {
    proxyResolverInput.value = settings.value.proxy_resolver
  }
}

/** 把输入框恢复成当前配置值（放弃未提交的编辑）。 */
function resetInputs() {
  maxCellsIndex.value = indexForCells(settings.value.max_cells)
  retentionInput.value = toSeconds(settings.value.completed_retention_ms)
  runningTimeoutInput.value = toSeconds(settings.value.running_timeout_ms)
  tokenUnitInput.value = settings.value.token_unit
  proxyResolverInput.value = settings.value.proxy_resolver
}

/**
 * 提交时间类配置（保留时长 + 执行中超时）。
 * 各字段本来就在 `@change` 时落盘，这个按钮是给「改完没失焦就想保存」兜底。
 */
async function saveTimeouts() {
  await updateRetention()
  await updateRunningTimeout()
}
</script>

<template>
  <!-- 两块卡片横向并排（窄窗口由 .general-cards 的媒体查询塌成单列）。 -->
  <div class="general-cards">
    <section class="ufo-card module-settings" aria-labelledby="agent-status-behaviour-title">
      <div class="ufo-card__header">
        <h2 id="agent-status-behaviour-title" class="ufo-card__title">行为</h2>
        <span class="ufo-card__hint">即时生效</span>
      </div>

      <SettingsField label="完成灯保留（秒）" label-for="completed-retention-seconds">
        <BaseInput
          v-model.number="retentionInput"
          id="completed-retention-seconds"
          type="number"
          :min="MIN_RETENTION_SECONDS"
          :max="MAX_RETENTION_SECONDS"
          step="1"
          @change="updateRetention"
        />
      </SettingsField>

      <SettingsField
        label="执行中超时（秒）"
        label-for="running-timeout-seconds"
        hint="绿灯（Running）超过该时长无新事件会被移除；默认 60 秒，范围 60–3600。"
      >
        <BaseInput
          v-model.number="runningTimeoutInput"
          id="running-timeout-seconds"
          type="number"
          :min="MIN_RUNNING_TIMEOUT_SECONDS"
          :max="MAX_RUNNING_TIMEOUT_SECONDS"
          step="30"
          title="Running 无新 hook 时移除绿灯；慢 agent 可拉长"
          @change="updateRunningTimeout"
        />
      </SettingsField>

      <SettingsField
        label="Token 单位"
        label-id="token-unit-label"
        hint="悬浮窗提示行里的 token 总量写法：Tokens 显示原始计数，M tokens 以百万为单位。"
      >
        <BaseSegmentedControl
          v-model="tokenUnitInput"
          name="token-unit"
          :options="tokenUnitOptions"
          aria-labelledby="token-unit-label"
          @change="updateTokenUnit"
        />
      </SettingsField>

      <SettingsField
        label="代理解析器"
        label-id="proxy-resolver-label"
        hint="选了代理工具后，会把本地监听地址反查成真正的上游 endpoint 显示在提示窗；不解析时按原样显示。"
      >
        <BaseSelect
          v-model="proxyResolverInput"
          :options="proxyResolverOptions"
          aria-labelledby="proxy-resolver-label"
          @change="updateProxyResolver"
        />
      </SettingsField>
    </section>

    <section class="ufo-card module-settings" aria-labelledby="agent-status-cells-title">
      <div class="ufo-card__header">
        <h2 id="agent-status-cells-title" class="ufo-card__title">窗口格子</h2>
        <span class="ufo-card__hint">最大 {{ currentMaxCells }} 格</span>
      </div>

      <p class="ufo-card__hint">
        窗口按会话数在阶梯 1 / 2 / 3 / 4 / 6 / 9 里自动伸缩：横条、竖条按个数精确显示，贴角对应 1×1…3×3（最少 1×2）。
        这里设的是本模块的上限，超出的会话会收进最后一格的 +N；只影响状态灯，不改动其它组件。
      </p>

      <SettingsField label="最大格子数" label-id="max-cells-label">
        <div class="module-settings__cap-row">
          <BaseSlider
            v-model="maxCellsIndex"
            class="flex-1"
            :min="0"
            :max="AGENT_MAX_CELL_STEPS.length - 1"
            :step="1"
            aria-labelledby="max-cells-label"
            :aria-value-text="`${currentMaxCells} 格`"
            @change="updateMaxCells"
          />
          <span class="module-settings__cap-value">
            {{ currentMaxCells }} 格<span v-if="currentCornerShape"> · 贴角 {{ currentCornerShape }}</span>
          </span>
        </div>
      </SettingsField>
    </section>
  </div>

  <!-- 重置 / 保存是整页动作（保存的是左卡的两个时间参数，格子上限本就在 change 时落盘），
       所以提到两卡之外，避免看起来只作用于右卡。 -->
  <div class="module-settings__actions">
    <BaseButton variant="ghost" @click="resetInputs">重置</BaseButton>
    <BaseButton variant="primary" @click="saveTimeouts">保存</BaseButton>
  </div>
</template>