<script setup lang="ts">
/**
 * Agent 状态灯设置 · 常规：所有「填一下就完事」的配置，两块无卡头的短表单卡横向并排。
 *
 * 左卡 = 时间参数与全局取值（灯闪烁持续时间 / 无响应超时 / Token 单位 / 代理解析器），
 * 右卡 = 窗口与声音（窗口格子滑块 + 状态音效开关）。
 * 都不带卡头：每一行的标题已经说清是什么，「行为 / 即时生效」这类汇总性标题是噪音。
 * 纵向堆叠会白白拉长页面，所以并排铺开 —— 一屏看完，不用滚；
 * 需要「占满高度 + 左右各自滚动」的面板（信息卡拓展 / 会话记录）才各占一个二级 tab。
 *
 * 卡里的字段是本组件自己的配置（`../settings` 读写外层的 `module_settings['agent-status']`，
 * 字段名不经过外壳）；右卡的「最多显示几盏灯」也是本组件自己的配置（同块的 `max_cells`），
 * 只限制状态灯，不影响其它组件 —— 同级组件各管各的格子数，整窗按它们的需求之和自动伸缩。
 */
import { computed, onMounted, ref, watch } from 'vue'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import BaseButton from '@/components/ui/BaseButton.vue'
import BaseInput from '@/components/ui/BaseInput.vue'
import BaseSegmentedControl from '@/components/ui/BaseSegmentedControl.vue'
import BaseSelect from '@/components/ui/BaseSelect.vue'
import BaseSlider from '@/components/ui/BaseSlider.vue'
import BaseSwitch from '@/components/ui/BaseSwitch.vue'
import SettingsField from '@/components/ui/SettingsField.vue'
import {
  AGENT_MAX_CELL_STEPS,
  AGENT_STATUS_MODULE_ID,
  MAX_AGENT_CELLS,
  MAX_RETENTION_SECONDS,
  MAX_RUNNING_TIMEOUT_MINUTES,
  MIN_RETENTION_SECONDS,
  MIN_RUNNING_TIMEOUT_MINUTES,
  agentStatusSettings,
  fetchProxyResolvers,
  retentionMsFromSeconds,
  runningTimeoutMsFromMinutes,
  toMinutes,
  toSeconds,
  type ProxyResolverInfo,
  type TokenUnit,
} from '../settings'

const modules = useFloatingModulesStore()

/** 本组件自己的配置块（缺字段 / 非法值由 `agentStatusSettings` 回落默认并夹取）。 */
const settings = computed(() => agentStatusSettings(modules.config))

const retentionInput = ref(toSeconds(settings.value.completed_retention_ms))
/** 无响应超时的输入单位是分钟（配置里存毫秒）。 */
const runningTimeoutInput = ref(toMinutes(settings.value.running_timeout_ms))
const tokenUnitInput = ref<TokenUnit>(settings.value.token_unit)
const proxyResolverInput = ref(settings.value.proxy_resolver)
/** 状态音效开关：悬浮窗前端播放，关掉后三种跳色都不出声。 */
const soundEnabledInput = ref(settings.value.sound_enabled)

/** 代理解析器选项：这是本组件自己的概念，所以由本组件自己向后端拉。 */
const resolvers = ref<ProxyResolverInfo[]>([])

onMounted(async () => {
  try {
    resolvers.value = await fetchProxyResolvers()
  } catch (error) {
    console.warn('proxy resolver list unavailable', error)
  }
})

/** 状态音效的说明（收进问号角标）：三种提示音的语义 + 换音频的操作指引，都是解释不是读数。 */
const soundHint =
  '红灯 / 黄灯 / 完成绿闪各播一段提示音；绿常亮不发声，绿闪落成黄常亮也不重复响。换音频：把安装路径下 data/bgm 里的 goError.mp3 / goWarn.mp3 / goSucc.mp3 覆盖掉（重启后生效）。'

/** token 单位选项；`tokens` 显示原始计数，`m_tokens` 显示百万。 */
const tokenUnitOptions: { value: TokenUnit; label: string }[] = [
  { value: 'tokens', label: 'Tokens' },
  { value: 'm_tokens', label: 'M tokens' },
]

/** 代理解析器下拉选项；只把后端 id/label/说明映射成通用下拉需要的结构。 */
const proxyResolverOptions = computed(() =>
  resolvers.value.map(option => ({
    value: option.id,
    label: option.label,
    description: option.description,
  }))
)

/**
 * 窗口格子上限：滑块按「档位下标」0..5 驱动，映射到本模块的阶梯 `AGENT_MAX_CELL_STEPS` = 1,2,3,4,6,9。
 * 配置里存的是格子数，但可能是迁移来的非档位值（如 8）—— 下标取「不大于它的最大档位」，
 * 与生效上限（`effectiveLayoutMax` 向下归档）对齐，滑块因此永远落在合法档位上。
 */
const maxCellsIndex = ref(indexForCells(settings.value.max_cells))
const currentMaxCells = computed(() => AGENT_MAX_CELL_STEPS[maxCellsIndex.value] ?? MAX_AGENT_CELLS)

/** 贴角网格标注：档位 → 行×列（如 9 盏灯 = 3×3 方块），标在滑块刻度下方帮用户理解这个数撑多大。 */
const cornerShapes: Record<number, string> = { 1: '1×1', 2: '1×2', 3: '1×3', 4: '2×2', 6: '2×3', 9: '3×3' }

/**
 * 滑块刻度：每个档位一枚，下标即档位、标注贴到四角时的方块形状。
 * 刻度是纯展示（aria-hidden），读屏走 aria-valuetext。
 */
const maxCellTicks = AGENT_MAX_CELL_STEPS.map((cells, index) => ({
  value: index,
  label: cornerShapes[cells] ?? String(cells),
}))

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
    runningTimeoutInput.value = toMinutes(value.running_timeout_ms)
    tokenUnitInput.value = value.token_unit
    proxyResolverInput.value = value.proxy_resolver
    maxCellsIndex.value = indexForCells(value.max_cells)
    soundEnabledInput.value = value.sound_enabled
  },
  { immediate: true }
)

/** 更新灯闪烁持续时间（输入是秒，配置里存毫秒）。 */
async function updateRetention() {
  try {
    await modules.updateModuleSettings(AGENT_STATUS_MODULE_ID, {
      completed_retention_ms: retentionMsFromSeconds(retentionInput.value),
    })
  } catch {
    retentionInput.value = toSeconds(settings.value.completed_retention_ms)
  }
}

/** 更新无响应超时（输入是分钟，配置里存毫秒）；慢 agent 可拉长避免绿灯被误清。 */
async function updateRunningTimeout() {
  try {
    await modules.updateModuleSettings(AGENT_STATUS_MODULE_ID, {
      running_timeout_ms: runningTimeoutMsFromMinutes(runningTimeoutInput.value),
    })
  } catch {
    runningTimeoutInput.value = toMinutes(settings.value.running_timeout_ms)
  }
}

/** 滑块松手后落盘：把档位下标映射回格子数（= 最多显示几盏灯），写进本组件自己的配置块；失败 / 没变时退回已知值。 */
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

/** 更新状态音效开关（悬浮窗前端据此决定跳色时播不播提示音）；失败时退回已知值。 */
async function updateSoundEnabled() {
  try {
    await modules.updateModuleSettings(AGENT_STATUS_MODULE_ID, { sound_enabled: soundEnabledInput.value })
  } catch {
    soundEnabledInput.value = settings.value.sound_enabled
  }
}

/** 把输入框恢复成当前配置值（放弃未提交的编辑）。 */
function resetInputs() {
  maxCellsIndex.value = indexForCells(settings.value.max_cells)
  retentionInput.value = toSeconds(settings.value.completed_retention_ms)
  runningTimeoutInput.value = toMinutes(settings.value.running_timeout_ms)
  tokenUnitInput.value = settings.value.token_unit
  proxyResolverInput.value = settings.value.proxy_resolver
  soundEnabledInput.value = settings.value.sound_enabled
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
  <!-- 两块卡片横向并排（窄窗口由 .general-cards 的媒体查询塌成单列）。两张卡都不带卡头：
       每一行自己的标题已经说清是什么，「行为 / 即时生效」这类汇总性标题是噪音。 -->
  <div class="general-cards">
    <section class="ufo-card module-settings">
      <!-- 单位留在标题里（输入框只有数字，单位必须可见）；取值范围收进问号。 -->
      <SettingsField
        label="灯闪烁持续时间（秒）"
        label-for="completed-retention-seconds"
        hint="一轮完成后绿灯闪烁维持的时长，到期落成黄常亮；默认 30，范围 1–86400。"
      >
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

      <!-- 说明收进问号角标（`SettingsHint`）：取值范围与默认值是次要文案，常驻会把卡片撑高。 -->
      <SettingsField
        label="无响应超时（分钟）"
        label-for="running-timeout-minutes"
        hint="绿灯（Running）期间超过该时长没有任何新事件，就判定这一轮已结束并清掉灯；默认 1，范围 1–60。慢 agent 可拉长。"
      >
        <BaseInput
          v-model.number="runningTimeoutInput"
          id="running-timeout-minutes"
          type="number"
          :min="MIN_RUNNING_TIMEOUT_MINUTES"
          :max="MAX_RUNNING_TIMEOUT_MINUTES"
          step="1"
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

    <!-- 右卡 = 窗口与声音两行（原先的「窗口格子」「状态音效」两张卡合并；都不带卡头）。 -->
    <section class="ufo-card module-settings">
      <!-- 窗口格子：滑块贴右；「最多 N 盏灯」是刻度形状读不出的结论（3×3 要换算成盏数），走 state 常驻标题下面。 -->
      <SettingsField
        label="窗口格子"
        :state="`最多 ${currentMaxCells} 盏灯`"
        hint="有几个会话就显示几盏灯：上下贴边时横向排开、左右贴边时纵向排开、贴到四角时排成方块。这里限制的是「最多显示几盏」，超出的会话折叠成最后一格的 +N；只影响状态灯，不影响其它组件。"
      >
        <BaseSlider
          v-model="maxCellsIndex"
          class="module-settings__cap-slider"
          :min="0"
          :max="AGENT_MAX_CELL_STEPS.length - 1"
          :step="1"
          :ticks="maxCellTicks"
          aria-label="最多显示几盏灯"
          :aria-value-text="`${currentMaxCells} 盏灯`"
          @change="updateMaxCells"
        />
      </SettingsField>

      <!--
        状态音效：它配的是「有没有声音」，与左卡的时间参数、上面的格子尺寸都不是一类，
        但都是「灯怎么表现」的开关，合并在同一张卡里各占一行。
        换音频说明是静态解释、不是可变读数 → 收进问号，行内不常驻灰字。
      -->
      <SettingsField label="状态音效" :hint="soundHint">
        <BaseSwitch v-model="soundEnabledInput" aria-label="状态音效" @change="updateSoundEnabled" />
      </SettingsField>
    </section>
  </div>

  <!-- 重置 / 保存是整页动作（保存的是左卡的两个时间参数，格子上限本就在 change 时落盘），
       所以提到两卡之外，避免看起来只作用于某一张卡。 -->
  <div class="module-settings__actions">
    <BaseButton variant="ghost" @click="resetInputs">重置</BaseButton>
    <BaseButton variant="primary" @click="saveTimeouts">保存</BaseButton>
  </div>
</template>