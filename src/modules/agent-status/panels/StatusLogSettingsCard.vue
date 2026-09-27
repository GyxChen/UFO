<script setup lang="ts">
/**
 * 「转灯日志」设置卡：会话记录页顶部那张占满宽度的卡，只有一个 switch。
 *
 * 一个行为只留一个入口 —— 关 = 完全不记（`level: off`），开 = 明细级 + hook 侧同记
 * （`level: verbose` + `hook_enabled: true`）。Rust 侧仍保留四档与独立 hook 开关的字段结构，
 * 但界面上不再暴露：写回时整块提交，未暴露字段（保留期 / 体积上限 / 环形容量）原样保留。
 *
 * ⚠️ 关掉日志**不影响**「会话记录 / 记录详情」里的提问、Endpoint 等存档 ——
 * 那是加日志之前就有的独立功能（`session_store.rs`）。文案里要写明这一点，
 * 否则用户会拿「提问还在不在记」来检验这次改动有没有破坏既有行为。
 */
import { computed, ref, watch } from 'vue'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import {
  AGENT_STATUS_MODULE_ID,
  agentStatusSettings,
  logBlockWithSwitch,
  logSwitchOn,
} from '../settings'
import BaseSwitch from '@/components/ui/BaseSwitch.vue'

const modules = useFloatingModulesStore()

/** 本组件当前配置块里的日志部分（外壳只透传整块 JSON，字段由本模块解释）。 */
const settings = computed(() => agentStatusSettings(modules.config))

/** switch 的本地态：落盘失败要能退回已知值，不能停在「界面说在记、盘上没记」。 */
const on = ref(logSwitchOn(settings.value.log))

// 配置从后端回来的那一刻（含其它窗口改过同一份配置）以它为准。
watch(
  () => settings.value.log,
  log => {
    on.value = logSwitchOn(log)
  },
  { immediate: true }
)

/** 右侧常驻读数：「当前是什么状态」按项目规矩直接写出来，不藏进问号角标。 */
const stateLabel = computed(() =>
  on.value ? '正在记 · 明细级 + hook' : '已关闭 · 零写盘'
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
</script>

<template>
  <section class="ufo-card lg-settings" :class="{ 'lg-settings--off': !on }" aria-labelledby="lg-settings-title">
    <div class="lg-settings__head">
      <BaseSwitch
        v-model="on"
        aria-labelledby="lg-settings-title"
        :title="on ? '关闭后不再记录任何转灯裁决' : '打开后记录每一次转灯裁决'"
        @change="save"
      />
      <div class="lg-settings__text">
        <h2 id="lg-settings-title" class="lg-settings__title ufo-heading-row__title">转灯日志</h2>
        <p class="lg-settings__sub">
          打开后把每一次转灯裁决落成一行可回查的账；关闭则完全不写盘。关掉它不影响下面的会话存档。
        </p>
      </div>
      <span class="lg-settings__state">{{ stateLabel }}</span>
    </div>

    <!-- 风险告知紧贴开关下方常驻：这是「勾选项自身的风险」，按约定不走问号角标。 -->
    <p v-if="on" class="lg-settings__risk">
      明细级会把<b>提问摘录</b>与审批理由写进本地日志文件，导出后即不受控；凭据与 API key 任何档位都不写入。
    </p>
  </section>
</template>
