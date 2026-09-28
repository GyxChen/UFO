<script setup lang="ts">
/**
 * 「信息卡拓展」面板：左侧 endpoint 列表，右侧该 endpoint 的拓展字段配置（流程脚本编辑器）。
 *
 * 列表数据来自后端持久化的 endpoint 档案（会话里发现的、非本地代理的地址，
 * 或用户手动添加的）。右侧委派给 `EndpointFlowEditor`，写一段跑在 QuickJS 沙盒里的脚本。
 *
 * 不使用 window.alert / window.confirm：本应用装了 tauri-plugin-prevent-default，
 * 原生对话框可能被拦掉，所以反馈一律走行内提示。
 */
import { computed, onMounted, ref, watch } from 'vue'
import { formatProfileTime, type EndpointProfile } from '../lib/endpoint-profiles'
import { flowFromExtensions } from '../lib/flow'
import { useEndpointProfilesStore } from '../stores/endpointProfiles'
import EndpointFlowEditor from './EndpointFlowEditor.vue'
import BaseButton from '@/components/ui/BaseButton.vue'
import BaseInput from '@/components/ui/BaseInput.vue'
import closeIcon from '@/assets/icon/close.svg'

const profiles = useEndpointProfilesStore()

/** 当前选中的档案 id；null 表示还没有 endpoint 可选。 */
const selectedId = ref<string | null>(null)

/** 手动添加框的草稿值。 */
const draftUrl = ref('')

/** 待二次确认的档案 id（点一次移除 → 按钮变「确认」，再点才真删）。 */
const pendingRemoval = ref<string | null>(null)

/** 面板内的行内错误/提示文案；空串表示无。 */
const feedback = ref('')

/** 选中项；档案列表可能因后端采集而变化，故按 id 现算而非缓存对象。 */
const selected = computed<EndpointProfile | null>(
  () => profiles.profiles.find(profile => profile.id === selectedId.value) ?? null
)

// 选中项被删除、或首次采集到数据时，把选中态落到第一条 —— 右侧不应停在已消失的 id 上。
// 同时清掉待确认态：否则「确认」按钮可能指着一个已经不存在的行，等同一 id 将来被重新
// 采集到时会变成一次点击就删。
watch(
  () => profiles.profiles,
  list => {
    pendingRemoval.value = null
    if (!list.some(profile => profile.id === selectedId.value)) {
      selectedId.value = list[0]?.id ?? null
    }
  },
  { immediate: true }
)

onMounted(() => {
  // 本面板自带一份订阅，避免依赖 App.vue 的启动顺序；store 内部幂等。
  void profiles.start()
})

/**
 * 移除档案：第一次点击只进入待确认态（按钮变「确认」），第二次才真删 —— 破坏性操作不一步到位。
 * 点别的行会自动把 armed 态转移到那一行；删除失败时错误进 `feedback`。
 */
function onRemove(profile: EndpointProfile) {
  if (pendingRemoval.value !== profile.id) {
    pendingRemoval.value = profile.id
    feedback.value = `再点一次「确认」以移除 ${profile.url}`
    return
  }
  pendingRemoval.value = null
  void profiles
    .remove(profile.id)
    .then(() => {
      feedback.value = ''
    })
    .catch(error => {
      feedback.value = `移除失败：${error}`
    })
}

/** 手动登记一条 endpoint；后端会拒绝本地代理地址与非法值，原因走行内提示。 */
function onAdd() {
  const url = draftUrl.value.trim()
  if (!url) {
    feedback.value = '请先填写 endpoint 地址'
    return
  }
  void profiles.add(url).then(
    () => {
      draftUrl.value = ''
      feedback.value = ''
    },
    error => {
      feedback.value = `添加失败：${error}`
    }
  )
}
</script>

<template>
  <div class="ep-panel">
    <section class="ufo-card ep-list-card" aria-labelledby="ep-list-title">
      <div class="ufo-card__header">
        <h2 id="ep-list-title" class="ufo-card__title">Endpoint</h2>
        <span class="ufo-card__hint">{{ profiles.profiles.length }} 个</span>
      </div>

      <p v-if="!profiles.loaded" class="ep-list__empty">正在读取已登记的 endpoint…</p>
      <p v-else-if="profiles.connection === 'degraded'" class="ep-list__empty">
        暂时读不到本地 endpoint 记录（IPC 不可用），请重启应用。
      </p>
      <p v-else-if="!profiles.profiles.length" class="ep-list__empty">
        还没有可用的 endpoint。会话连上非本地代理的地址后会自动登记在这里。
      </p>
      <ul v-else class="ep-list">
        <li v-for="profile in profiles.profiles" :key="profile.id" class="ep-list__item">
          <button
            type="button"
            class="ep-list__row"
            :class="{ 'ep-list__row--active': profile.id === selectedId }"
            :aria-current="profile.id === selectedId ? 'true' : undefined"
            @click="selectedId = profile.id"
          >
            <span class="ep-list__url">{{ profile.url }}</span>
            <span class="ep-list__meta">
              <span
                v-if="flowFromExtensions(profile.extensions)?.enabled"
                class="ep-list__tag"
                title="该 endpoint 已启用流程脚本"
                >流程</span
              >
              <span class="ep-list__time">{{ formatProfileTime(profile.first_seen_at_ms) }}</span>
            </span>
          </button>
          <button
            type="button"
            class="ep-list__remove"
            :class="{ 'ep-list__remove--armed': pendingRemoval === profile.id }"
            :aria-label="pendingRemoval === profile.id ? `确认移除 ${profile.url}` : `移除 ${profile.url}`"
            @click="onRemove(profile)"
          >
            <span v-if="pendingRemoval === profile.id" class="ep-list__remove-text">确认</span>
            <img v-else :src="closeIcon" alt="" class="ufo-mono-icon" aria-hidden="true" />
          </button>
        </li>
      </ul>

      <form class="ep-add" @submit.prevent="onAdd">
        <label class="ep-add__field">
          <span>手动添加</span>
          <BaseInput
            v-model="draftUrl"
            type="text"
            spellcheck="false"
            placeholder="https://api.example.com"
            aria-label="手动添加 endpoint 地址"
          />
        </label>
        <BaseButton class="ep-add__btn" type="submit">添加</BaseButton>
      </form>
      <p v-if="feedback" class="ep-list__feedback" role="status">{{ feedback }}</p>
    </section>

    <EndpointFlowEditor :profile="selected" />
  </div>
</template>
