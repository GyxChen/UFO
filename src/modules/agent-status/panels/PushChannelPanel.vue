<script setup lang="ts">
/**
 * 「消息推送」面板：上半区总开关与事件勾选，下半区左列选服务、右列管该服务的主题列表。
 *
 * 版面照 `reference/mock/push/push-a-dual-pane.html`（方案 A），与「信息卡拓展」同构：
 * 两栏各自滚动、占满内容区高度（靠 `.push-panel` 那条高度链，见 agent-status.css）。
 *
 * 一个服务只有两样东西：服务器地址 + 若干行「名字 / 主题」。表单因此是**写死**的两栏，
 * 不再按后端送来的字段表动态渲染 —— 控制流里也没有 `provider === 'bark'` 这类分支，
 * 服务差异只体现在文案与 `send` 里。
 *
 * ⚠️ 不用 `window.alert` / `confirm`（`tauri-plugin-prevent-default` 会拦）：
 * 反馈一律走行内 `role="status"`，删除走两步确认。
 */
import { computed, onMounted, ref, watch } from 'vue'
import { usePushStore } from '../stores/pushStatus'
import { pushProviderMeta } from '../lib/push-providers'
import {
  PUSH_CONTENT_OPTIONS,
  PUSH_EVENT_OPTIONS,
  validateServer,
  validateTopic,
  type ChannelView,
  type ProviderView,
} from '../lib/push'
import BaseButton from '@/components/ui/BaseButton.vue'
import BaseInput from '@/components/ui/BaseInput.vue'
import BaseSwitch from '@/components/ui/BaseSwitch.vue'
import SettingsField from '@/components/ui/SettingsField.vue'
import closeIcon from '@/assets/close.svg'

const push = usePushStore()

/** 当前选中的服务 id；null 表示后端还没回话。 */
const selectedProvider = ref<string | null>(null)
/** 行内提示（成功与失败都走这一条）。 */
const feedback = ref('')
/** 提示语气：决定行内文字用强调色还是危险色。 */
const feedbackTone = ref<'ok' | 'err'>('ok')
/** 待二次确认的主题行 id（点一次「移除」变「确认」，再点才真删）。 */
const pendingRemoval = ref<string | null>(null)

onMounted(() => {
  // 本面板自带一份订阅，不依赖 App.vue 的启动顺序；store 内部幂等。
  void push.start()
})

/** 后端注册的服务（顺序即 UI 顺序）。 */
const providers = computed(() => push.providers)

/** 选中的服务；注册表变化时回落到第一个，右列不该停在已经不存在的 id 上。 */
const activeProvider = computed<ProviderView | null>(
  () => providers.value.find(item => item.id === selectedProvider.value) ?? providers.value[0] ?? null
)

// 换服务时清掉待确认态：否则「确认」按钮可能指着一个别的服务下的行。
watch(activeProvider, provider => {
  selectedProvider.value = provider?.id ?? null
  pendingRemoval.value = null
})

/** 当前服务下的主题行。 */
const activeTopics = computed<ChannelView[]>(() =>
  activeProvider.value
    ? push.channels.filter(topic => topic.provider === activeProvider.value?.id)
    : []
)

/** 配置里某服务的开关状态。 */
function providerEnabled(id: string): boolean {
  return push.config.providers[id]?.enabled === true
}

/** 已启用的服务数 · 主题总数，显示在总开关那一行右侧。 */
const summary = computed(() => {
  const services = providers.value.filter(item => providerEnabled(item.id)).length
  return `${providers.value.length} 个服务 · ${push.channels.length} 行主题 · ${services} 个在服务`
})

/** 一句服务副标题（未登记服务也能显示，走注册表降级）。 */
function providerBlurb(id: string): string {
  return pushProviderMeta(id).blurb
}

/** 该服务下「一行收件地址」的量词（ntfy 叫主题、Bark 叫设备密钥）。只影响文案，不影响控制流。 */
function topicNoun(id: string): string {
  return pushProviderMeta(id).topicNoun
}

/** 当前服务下的量词：空态文案、输入框占位与添加按钮共用一份。 */
const activeNoun = computed(() => topicNoun(activeProvider.value?.id ?? ''))

/** 事件是否被勾选。 */
function eventOn(value: string): boolean {
  return push.config.events.includes(value as never)
}

/** 某项提醒内容是否被勾选（agent 状态固定发，不在这一列）。 */
function contentOn(value: string): boolean {
  return push.config.content.includes(value as never)
}

/** 统一包一层：写配置失败要把原因显示出来，不能静默吞掉。 */
async function run(action: () => Promise<unknown>, okText = '') {
  try {
    await action()
    feedback.value = okText
    feedbackTone.value = 'ok'
  } catch (error) {
    feedback.value = `没有保存：${String(error)}`
    feedbackTone.value = 'err'
  }
}

/** 读一个输入框的值（`BaseInput` 的 change 事件发的是原生 Event）。 */
function inputValue(event: Event): string {
  return (event.target as HTMLInputElement).value
}

/** 总开关。 */
function onMaster(enabled: boolean) {
  return run(() => push.setEnabled(enabled), enabled ? '推送已开启' : '推送已关闭，所有主题都不发')
}

/** 勾选 / 取消一个事件。 */
function onEvent(value: string, on: boolean) {
  return run(() => push.toggleEvent(value, on))
}

/** 勾选 / 去勾一项提醒内容。 */
function onContent(value: string, on: boolean) {
  return run(() => push.toggleContent(value, on))
}

/** 服务级开关。 */
function onProviderToggle(id: string, on: boolean) {
  return run(() => push.setProviderEnabled(id, on))
}

/** 服务器地址：坏值（缺 scheme / 超长）在前端就拦下，不写进配置。 */
function onServer(event: Event) {
  const value = inputValue(event).trim()
  const error = validateServer(value)
  if (error) {
    feedback.value = error
    feedbackTone.value = 'err'
    return
  }
  return run(() => push.setProviderServer(activeProvider.value?.id ?? '', value), '服务器地址已更新')
}

/** 主题行的名字。 */
function onTopicName(topic: ChannelView, event: Event) {
  return run(() => push.updateTopic(topic.id, { name: inputValue(event) }))
}

/** 主题串：空值照样保存（用户可以留着半成品），但要提醒一句「空的发不出去」。 */
function onTopicValue(topic: ChannelView, event: Event) {
  const value = inputValue(event)
  const warning = validateTopic(value)
  return run(
    () => push.updateTopic(topic.id, { topic: value }),
    warning ? `已保存，但${warning.replace('主题', '这一行主题')}` : ''
  )
}

/** 新建一行主题。 */
async function onAddTopic() {
  const provider = activeProvider.value
  if (!provider) return
  await run(() => push.addTopic(provider.id))
  feedback.value = `已加一行${topicNoun(provider.id)}，填上值才生效`
  feedbackTone.value = 'ok'
}

/**
 * 给当前服务下的每个主题各发一条测试。
 *
 * 逐主题报成败：一条坏地址不该把其它行的结果一起吞掉。
 */
async function onTestAll() {
  const provider = activeProvider.value
  if (!provider) return
  feedback.value = '正在发送测试…'
  feedbackTone.value = 'ok'
  try {
    const outcomes = await push.testProvider(provider.id)
    feedback.value = outcomes
      .map(item => `${item.name} ${item.ok ? '✓' : `✗ ${item.error ?? '失败'}`}`)
      .join('　·　')
    feedbackTone.value = outcomes.every(item => item.ok) ? 'ok' : 'err'
  } catch (error) {
    feedback.value = `测试失败：${String(error)}`
    feedbackTone.value = 'err'
  }
}

/** 两步确认删除：第一下只进入待确认态。 */
async function onRemove(topic: ChannelView) {
  if (pendingRemoval.value !== topic.id) {
    pendingRemoval.value = topic.id
    feedback.value = `再点一次「确认」以移除 ${topic.name || topic.id}`
    feedbackTone.value = 'err'
    return
  }
  pendingRemoval.value = null
  await run(() => push.removeTopic(topic.id), '主题已删除')
}
</script>

<template>
  <div class="push-panel">
    <!-- ① 上半区：总开关与「发什么 / 发哪些事件」（所有服务共用，一次配好） -->
    <section class="ufo-card push-master" aria-labelledby="push-master-title">
      <div class="push-master__head">
        <BaseSwitch
          :model-value="push.config.enabled"
          aria-label="消息推送总开关"
          @update:model-value="onMaster"
        />
        <div class="push-master__text">
          <h3 id="push-master-title" class="push-master__title">消息推送</h3>
          <p class="push-master__sub">总开关。关掉后所有主题一律不发，也不会碰网络。</p>
        </div>
        <span class="push-master__meta">{{ summary }}</span>
      </div>

      <div class="push-master__body" :class="{ 'push-master__body--dim': !push.config.enabled }">
        <SettingsField label="哪些事发通知（可多选）">
          <div class="push-chips" role="group" aria-label="推送事件">
            <button
              v-for="option in PUSH_EVENT_OPTIONS"
              :key="option.value"
              type="button"
              class="push-chip"
              :aria-pressed="eventOn(option.value)"
              :title="option.hint"
              @click="onEvent(option.value, !eventOn(option.value))"
            >
              <span class="push-dot" :class="`push-dot--${option.dot}`" aria-hidden="true"></span>
              {{ option.label }}
            </button>
          </div>
        </SettingsField>

        <SettingsField
          label="提醒内容（可多选）"
          hint="「哪个平台的哪类状态」固定发，这里只挑还要额外带什么；不勾的那项整行不出现"
        >
          <div class="push-chips" role="group" aria-label="提醒内容">
            <button
              v-for="option in PUSH_CONTENT_OPTIONS"
              :key="option.value"
              type="button"
              class="push-chip"
              :aria-pressed="contentOn(option.value)"
              :title="option.hint"
              @click="onContent(option.value, !contentOn(option.value))"
            >
              {{ option.label }}
            </button>
          </div>
        </SettingsField>
      </div>
    </section>

    <!-- ② 下半区：左列选服务，右列管该服务的主题（与「信息卡拓展」同构） -->
    <div class="push-split">
      <section class="ufo-card push-list-card" aria-labelledby="push-provider-title">
        <div class="ufo-card__header">
          <h3 id="push-provider-title" class="ufo-card__title">推送服务</h3>
          <span class="ufo-card__hint">{{ providers.length }} 个</span>
        </div>

        <p v-if="!push.loaded" class="push-list__empty">正在读取推送配置…</p>
        <p v-else-if="push.connection === 'degraded'" class="push-list__empty">
          暂时读不到推送配置（IPC 不可用），请重启应用。
        </p>
        <p v-else-if="!providers.length" class="push-list__empty">
          后端还没有注册任何推送服务。
        </p>
        <ul v-else class="push-list">
          <li v-for="provider in providers" :key="provider.id" class="push-list__item">
            <!-- 行不是按钮：里面还嵌着服务开关，按钮套交互控件是非法 HTML，键盘也会串线 -->
            <div
              class="push-list__row"
              :class="{ 'push-list__row--active': provider.id === activeProvider?.id }"
            >
              <button
                type="button"
                class="push-list__pick"
                :aria-current="provider.id === activeProvider?.id ? 'true' : undefined"
                @click="selectedProvider = provider.id"
              >
                <span class="push-list__name">{{ pushProviderMeta(provider.id).label }}</span>
                <span class="push-list__sub">{{ providerBlurb(provider.id) }}</span>
              </button>
              <span class="push-list__side">
                <span class="push-tag">{{
                  push.channels.filter(item => item.provider === provider.id).length
                }} 个{{ topicNoun(provider.id) }}</span>
                <BaseSwitch
                  :model-value="providerEnabled(provider.id)"
                  :aria-label="`启用服务 ${pushProviderMeta(provider.id).label}`"
                  @update:model-value="onProviderToggle(provider.id, $event)"
                />
              </span>
            </div>
          </li>
        </ul>
        <p class="push-list__foot">每个服务自带开关：服务关掉时，它下面的主题一条都不发。</p>
      </section>

      <section class="ufo-card push-detail" aria-labelledby="push-detail-title">
        <!-- 标题在左、统一测试在右上角：测试是「整块一次验」，不该藏在每一行里 -->
        <div class="ufo-card__header push-detail__head">
          <div class="push-detail__heading">
            <h3 id="push-detail-title" class="ufo-card__title">
              {{ activeProvider ? pushProviderMeta(activeProvider.id).label : '主题' }}
            </h3>
            <span class="ufo-card__hint">改完即存，不需要点保存</span>
          </div>
          <BaseButton
            size="sm"
            :disabled="!activeTopics.length || push.isBusy(activeProvider?.id ?? '')"
            @click="onTestAll"
          >
            {{
              activeProvider && push.isBusy(activeProvider.id)
                ? '发送中…'
                : `发一条测试（${activeTopics.length}）`
            }}
          </BaseButton>
        </div>

        <div v-if="activeProvider" class="push-detail__body">
          <SettingsField :label="activeProvider.spec.label" :hint="activeProvider.spec.help">
            <BaseInput
              :model-value="activeProvider.server"
              :placeholder="activeProvider.spec.default"
              :aria-label="`${pushProviderMeta(activeProvider.id).label} ${activeProvider.spec.label}`"
              @change="onServer"
            />
          </SettingsField>

          <!-- 一行为空时不写解释文案（下面的「＋ 添加…」就是指引）；行也不是 button：
               里面两个输入框加一个删除按钮，交互控件不能互相嵌套 -->
          <ul v-if="activeTopics.length" class="push-topics">
            <li v-for="topic in activeTopics" :key="topic.id" class="push-topic">
              <input
                class="push-topic__name"
                :value="topic.name"
                placeholder="名称"
                :aria-label="`名称（${topic.id}）`"
                @change="onTopicName(topic, $event)"
              />
              <input
                class="push-topic__value"
                :value="topic.topic"
                :placeholder="activeNoun"
                spellcheck="false"
                :aria-label="`${topic.name || activeNoun} 的${activeNoun}`"
                @change="onTopicValue(topic, $event)"
              />
              <button
                type="button"
                class="ep-list__remove"
                :class="{ 'ep-list__remove--armed': pendingRemoval === topic.id }"
                :aria-label="
                  pendingRemoval === topic.id
                    ? `确认移除 ${topic.name || topic.id}`
                    : `移除 ${topic.name || topic.id}`
                "
                @click="onRemove(topic)"
              >
                <span v-if="pendingRemoval === topic.id" class="ep-list__remove-text">确认</span>
                <img v-else :src="closeIcon" alt="" class="ufo-mono-icon" aria-hidden="true" />
              </button>
            </li>
          </ul>

          <div class="push-detail__foot">
            <BaseButton size="sm" variant="ghost" @click="onAddTopic">
              ＋ 添加{{ activeNoun }}
            </BaseButton>
          </div>
        </div>
      </section>
    </div>

    <!-- 行内反馈（禁用原生对话框） -->
    <p class="push-status" :class="`push-status--${feedbackTone}`" role="status" aria-live="polite">
      {{ feedback }}
    </p>
  </div>
</template>
