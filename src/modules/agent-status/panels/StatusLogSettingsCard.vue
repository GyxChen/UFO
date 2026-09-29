<script setup lang="ts">
/**
 * 「转灯日志」设置卡：会话记录页顶部那张占满宽度的卡。
 *
 * 一个行为只留一个入口 ——
 * - 记不记：只有一个 switch（关 = `level: off` 完全不记，开 = 明细级 + hook 侧同记）；
 * - 留多久：只有一个「保留天数」下拉（界面上唯一的回收旋钮，只有 7 / 14 两档）。体积上限
 *   刻意不给入口：两个旋钮会让人分不清「留多久」与「占多少」谁说了算；但那条线仍在背后
 *   削最旧的整天，所以占用与「削过没有」必须常驻显示（写在标题下面一行），不许藏进问号角标。
 *
 * ⚠️ 写回一律整块提交（`updateModuleSettings` 是模块层浅合并，只传半个 `log` 子块会把
 * 未暴露字段抹掉）；Rust 侧仍保留四档与独立 hook 开关的字段结构，界面上不再暴露。
 *
 * ⚠️ 关掉日志**不影响**「会话记录 / 记录详情」里的提问、Endpoint 等存档 ——
 * 那是加日志之前就有的独立功能（`session_store.rs`）。文案里要写明这一点，
 * 否则用户会拿「提问还在不在记」来检验这次改动有没有破坏既有行为。
 */
import { computed, ref, watch } from 'vue'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import {
  AGENT_STATUS_MODULE_ID,
  LOG_RETENTION_DAYS_CHOICES,
  agentStatusSettings,
  logBlockWithSwitch,
  logSwitchOn,
  nearestRetentionChoice,
} from '../settings'
import { logUsageLabel } from '../lib/status-log'
import { useStatusLogStore } from '../stores/statusLog'
import BaseSwitch from '@/components/ui/BaseSwitch.vue'
import BaseSelect from '@/components/ui/BaseSelect.vue'
import SettingsField from '@/components/ui/SettingsField.vue'

const modules = useFloatingModulesStore()
const log = useStatusLogStore()

/** 本组件当前配置块里的日志部分（外壳只透传整块 JSON，字段由本模块解释）。 */
const settings = computed(() => agentStatusSettings(modules.config))

/** switch 的本地态：落盘失败要能退回已知值，不能停在「界面说在记、盘上没记」。 */
const on = ref(logSwitchOn(settings.value.log))

/** 下拉的两档（`BaseSelect` 的取值是字符串，落盘前转回数字）。 */
const retentionOptions = LOG_RETENTION_DAYS_CHOICES.map(choice => ({
  value: String(choice),
  label: `${choice} 天`,
}))

/** 盘上那个天数 → 下拉里真能选中的那一档（旧版数字输入框能写 1–90）。 */
function choiceOf(block: { retention_days: number }): number {
  return nearestRetentionChoice(block.retention_days, block.retention_days)
}

/** 保留天数的本地态：与 `on` 同一套路，配置回来以它为准，写失败退回。 */
const days = ref(choiceOf(settings.value.log))

/** 已经为「盘上那个选不出来的天数」回写过哪个数；写失败就清空，允许下次配置变化再试。 */
let snappedTo: number | null = null

// 占用读数要有「还没读到」这一档：`log.start()` 的首拉落地前，写「还没写过日志」是错报。
const usageLabel = computed(() => (log.loaded ? logUsageLabel(log.usage) : '占用读取中…'))

// 配置从后端回来的那一刻（含其它窗口改过同一份配置）以它为准。
watch(
  () => settings.value.log,
  block => {
    on.value = logSwitchOn(block)
    const choice = choiceOf(block)
    days.value = choice
    // 界面只有两档，就得让盘上那个数也落在这两档上：否则「面板说 14 天、后端按 3 天删」，
    // 显示与事实分离正是这个旋钮最不该出现的样子。同一个目标值只试一次，不反复自旋。
    if (choice !== block.retention_days && snappedTo !== choice) {
      snappedTo = choice
      void saveRetentionDays(choice)
    }
  },
  { immediate: true }
)

/**
 * 切换开关并整块落盘。
 *
 * 失败时把本地值退回配置里的已知值 —— `updateModuleSettings` 会抛，界面若不回退就成了
 * 「看起来开了其实没开」，那正是这个功能最不该出现的错。
 */
async function save() {
  const next = logBlockWithSwitch(settings.value.log, on.value)
  try {
    await modules.updateModuleSettings(AGENT_STATUS_MODULE_ID, { log: next })
  } catch {
    on.value = logSwitchOn(settings.value.log)
  }
}

/**
 * 把保留天数写回配置。
 *
 * ⚠️ `log` 必须整块提交：`updateModuleSettings` 是模块层浅合并，只传半个子块会把
 * `retention_days` 之外的字段（档位、体积上限…）一起抹掉。
 * ⚠️ 合并用的是**这次提交开始时**那份块：写入在途时配置可能被别的窗口改过，
 * 拿回读值会把人家的改动一起覆盖掉。
 */
async function saveRetentionDays(next: number) {
  const block = settings.value.log
  try {
    await modules.updateModuleSettings(AGENT_STATUS_MODULE_ID, {
      log: { ...block, retention_days: next },
    })
  } catch {
    // 写不进去就退回盘上那个数对应的那一档：界面不许停在一个没落盘的数上。
    // 同时清掉回写标记，让下一次配置变化还能再试一次（不静默放弃归一）。
    snappedTo = null
    days.value = choiceOf(settings.value.log)
  }
}

/** 下拉选中：值只可能是那两档之一，转回数字直接落盘。 */
function onRetentionChange(value: string) {
  days.value = choiceOf({ retention_days: Number(value) })
  void saveRetentionDays(days.value)
}
</script>

<template>
  <section class="ufo-card lg-settings" aria-label="转灯日志设置">
    <!--
      标准设置行：标题 + 问号在左、开关贴右。
      说明收进问号，其中「关掉不影响会话存档」是红线句，必须留在文案里。
      不给 state 读数：在不在记，开关自己说得出。
    -->
    <SettingsField
      label="转灯日志"
      hint="打开后把每一次转灯裁决落成一行可回查的账；关闭则完全不写盘。关掉它不影响下面的会话存档。明细级会落提问与等待原因的截断摘录，凭据任何档位都不写。"
    >
      <BaseSwitch
        v-model="on"
        aria-label="转灯日志"
        :title="on ? '关闭后不再记录任何转灯裁决' : '打开后记录每一次转灯裁决'"
        @change="save"
      />
    </SettingsField>

    <!--
      保留天数是唯一的回收旋钮：标准设置行 = 标题 + 问号角标在左、下拉钉右；
      「当前占多少盘」是下拉读不出的事实，走 `state` 常驻在标题下面一行。
      ⚠️ 这行在日志关闭时也保持可用：不再写新的不等于已有账不必回收。
    -->
    <SettingsField
      label="保留天数"
      label-id="lg-retention-label"
      :state="usageLabel"
      :hint="`日志按 UTC 日分文件保存；超过这个天数的整天文件会被自动回收（当天的永不删）。只能选 ${LOG_RETENTION_DAYS_CHOICES.join(' 或 ')} 天，默认 7 天；盘上若还留着更早版本手填的其它天数，会自动就近改成这两档之一并写回。另有一条体积硬上限不给界面入口：量特别大的日子它会提前削掉最旧的一天，读数里会写明。`"
    >
      <BaseSelect
        :model-value="String(days)"
        :options="retentionOptions"
        aria-labelledby="lg-retention-label"
        @change="onRetentionChange"
      />
    </SettingsField>
  </section>
</template>
