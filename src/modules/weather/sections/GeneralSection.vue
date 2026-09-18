<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import BaseButton from '@/components/ui/BaseButton.vue'
import BaseInput from '@/components/ui/BaseInput.vue'
import BaseSegmentedControl from '@/components/ui/BaseSegmentedControl.vue'
import BaseSelect from '@/components/ui/BaseSelect.vue'
import SettingsField from '@/components/ui/SettingsField.vue'
import { weatherSettings, type WeatherSettings } from '../settings'
import { useWeatherStore, type CityLookupItem } from '../stores/weather'

const modules = useFloatingModulesStore()
const weatherStore = useWeatherStore()

const currentSettings = computed(() => weatherSettings(modules.config))

// 本地 API Key 输入状态（遵照规范第 5.3 节：不回显已存密钥，仅用于输入新值替换）
const apiKeyInput = ref('')
const isSavingKey = ref(false)
const keyActionMsg = ref<string | null>(null)

// 本地普通配置输入状态
const hostTypeInput = ref(currentSettings.value.host_type)
const customHostInput = ref(currentSettings.value.custom_host)
const locationModeInput = ref(currentSettings.value.location_mode)
const manualLocationInput = ref(currentSettings.value.manual_location)
const manualLocationNameInput = ref(currentSettings.value.manual_location_name)
const refreshIntervalInput = ref(currentSettings.value.refresh_interval_min)

// 城市搜索状态
const citySearchQuery = ref('')
const isSearchingCity = ref(false)
const citySearchResults = ref<CityLookupItem[]>([])
const searchError = ref<string | null>(null)

// 连通性测试状态
const isTesting = ref(false)
const testResult = ref<string | null>(null)
const testError = ref<string | null>(null)

onMounted(() => {
  void weatherStore.checkKeyStatus()
})

// 监听配置外部更新
watch(
  currentSettings,
  (val) => {
    hostTypeInput.value = val.host_type
    customHostInput.value = val.custom_host
    locationModeInput.value = val.location_mode
    manualLocationInput.value = val.manual_location
    manualLocationNameInput.value = val.manual_location_name
    refreshIntervalInput.value = val.refresh_interval_min
  },
  { immediate: true },
)

/**
 * 提交保存普通设置项
 */
async function saveCommonSettings(): Promise<void> {
  const patch: Partial<WeatherSettings> = {
    host_type: hostTypeInput.value,
    custom_host: customHostInput.value.trim(),
    location_mode: locationModeInput.value,
    manual_location: manualLocationInput.value.trim(),
    manual_location_name: manualLocationNameInput.value.trim(),
    refresh_interval_min: Math.min(Math.max(Number(refreshIntervalInput.value) || 15, 10), 60),
  }
  await modules.updateModuleSettings('weather', patch)
}

/**
 * 保存密钥至系统凭据管理器
 */
async function handleSaveKey(): Promise<void> {
  if (!apiKeyInput.value.trim()) return
  isSavingKey.value = true
  keyActionMsg.value = null
  try {
    await weatherStore.saveApiKey(apiKeyInput.value.trim())
    apiKeyInput.value = ''
    keyActionMsg.value = 'API Key 已安全保存到系统凭据管理器'
  } catch (err) {
    keyActionMsg.value = typeof err === 'string' ? err : '保存密钥失败'
  } finally {
    isSavingKey.value = false
  }
}

/**
 * 从系统凭据管理器清除密钥
 */
async function handleDeleteKey(): Promise<void> {
  isSavingKey.value = true
  keyActionMsg.value = null
  try {
    await weatherStore.deleteApiKey()
    apiKeyInput.value = ''
    keyActionMsg.value = 'API Key 已从系统凭据管理器删除'
  } catch (err) {
    keyActionMsg.value = typeof err === 'string' ? err : '删除密钥失败'
  } finally {
    isSavingKey.value = false
  }
}

/**
 * 触发城市搜索
 */
async function handleSearchCity(): Promise<void> {
  if (!citySearchQuery.value.trim()) return
  isSearchingCity.value = true
  searchError.value = null
  try {
    const results = await weatherStore.searchCity(
      citySearchQuery.value,
      {
        has_key: currentSettings.value.has_key,
        host_type: hostTypeInput.value,
        custom_host: customHostInput.value.trim(),
        location_mode: 'manual',
        manual_location: '',
        manual_location_name: '',
        refresh_interval_min: 15,
      },
      apiKeyInput.value.trim() || undefined,
    )
    citySearchResults.value = results
    if (results.length === 0) {
      searchError.value = '未找到匹配的城市'
    }
  } catch (err) {
    searchError.value = typeof err === 'string' ? err : '搜索城市失败'
  } finally {
    isSearchingCity.value = false
  }
}

/**
 * 用户选中检索出的城市
 */
function handleSelectCity(city: CityLookupItem): void {
  manualLocationInput.value = city.id
  manualLocationNameInput.value = `${city.adm1}-${city.name}`
  citySearchResults.value = []
  citySearchQuery.value = ''
  void saveCommonSettings()
}

/**
 * 执行测试连接
 */
async function handleTestConnection(): Promise<void> {
  isTesting.value = true
  testResult.value = null
  testError.value = null
  try {
    const snapshot = await weatherStore.testConnection(
      {
        has_key: currentSettings.value.has_key,
        host_type: hostTypeInput.value,
        custom_host: customHostInput.value.trim(),
        location_mode: locationModeInput.value,
        manual_location: manualLocationInput.value.trim(),
        manual_location_name: manualLocationNameInput.value.trim(),
        refresh_interval_min: 15,
      },
      apiKeyInput.value.trim() || undefined,
    )
    testResult.value = `连接成功！当前城市：${snapshot.location_name}，天气：${snapshot.text}，气温：${snapshot.temp}°C`
  } catch (err) {
    testError.value = typeof err === 'string' ? err : '连接测试失败，请检查 API Key 或网络'
  } finally {
    isTesting.value = false
  }
}

const hostOptions = [
  { value: 'dev', label: '开发版 (devapi.qweather.com)' },
  { value: 'commercial', label: '商业版 (api.qweather.com)' },
  { value: 'custom', label: '自定义 Host' },
]

const locationModeOptions = [
  { value: 'auto', label: '自动 IP 定位' },
  { value: 'manual', label: '手动指定城市' },
]
</script>

<template>
  <div class="general-cards">
    <!-- 左卡：API 授权与基础服务 -->
    <section class="ufo-card p-4 space-y-4">
      <header class="border-b border-[var(--ufo-border)] pb-2">
        <h3 class="text-sm font-semibold text-[var(--ufo-text)]">API 授权与服务</h3>
        <p class="text-xs text-[var(--ufo-muted)]">API Key 将安全存储在系统凭据管理器中，不进入配置文件</p>
      </header>

      <!-- 遵循规范第 5.3 & 9.4 节：密钥管理专属交互 -->
      <SettingsField
        label="API Key"
        :hint="weatherStore.hasSavedKey ? '当前状态：已保存至凭据管理器。输入新值点击保存可替换。' : '当前状态：未保存。请输入和风天气控制台生成的 Key。'"
        layout="stacked"
      >
        <div class="space-y-2">
          <BaseInput
            v-model="apiKeyInput"
            placeholder="输入新 Key 进行保存或测试..."
            @keydown.enter="handleSaveKey"
          />
          <div class="flex items-center gap-2">
            <BaseButton
              variant="primary"
              :disabled="isSavingKey || !apiKeyInput.trim()"
              @click="handleSaveKey"
            >
              保存密钥
            </BaseButton>
            <BaseButton
              v-if="weatherStore.hasSavedKey"
              variant="secondary"
              :disabled="isSavingKey"
              @click="handleDeleteKey"
            >
              清除已存密钥
            </BaseButton>
          </div>
          <p v-if="keyActionMsg" class="text-xs text-[var(--ufo-accent)]" role="status">
            {{ keyActionMsg }}
          </p>
        </div>
      </SettingsField>

      <SettingsField
        label="服务类型"
        hint="免费个人开发者请选开发版"
        layout="stacked"
      >
        <BaseSelect
          v-model="hostTypeInput"
          :options="hostOptions"
          @update:model-value="saveCommonSettings"
        />
      </SettingsField>

      <SettingsField
        v-if="hostTypeInput === 'custom'"
        label="自定义 Host 前缀"
        hint="如: https://api.your-proxy.com"
        layout="stacked"
      >
        <BaseInput
          v-model="customHostInput"
          placeholder="https://..."
          @blur="saveCommonSettings"
          @keydown.enter="saveCommonSettings"
        />
      </SettingsField>

      <SettingsField
        label="自动刷新周期（分钟）"
        hint="数据拉取间隔，范围 10~60 分钟（防额度耗尽）"
      >
        <div class="flex items-center gap-2">
          <div class="w-24">
            <BaseInput
              v-model="refreshIntervalInput"
              type="number"
              :min="10"
              :max="60"
              @blur="saveCommonSettings"
            />
          </div>
          <span class="text-xs text-[var(--ufo-muted)]">分钟</span>
        </div>
      </SettingsField>

      <div class="pt-2 border-t border-[var(--ufo-border)] flex flex-col gap-2">
        <BaseButton
          variant="secondary"
          :disabled="isTesting || (!weatherStore.hasSavedKey && !apiKeyInput.trim())"
          @click="handleTestConnection"
        >
          {{ isTesting ? '正在测试连接...' : '测试连接与定位' }}
        </BaseButton>

        <p v-if="testResult" class="text-xs text-[var(--ufo-accent)]" role="status">
          {{ testResult }}
        </p>
        <p v-if="testError" class="text-xs text-[var(--ufo-danger)]" role="status">
          {{ testError }}
        </p>
      </div>
    </section>

    <!-- 右卡：地理定位与城市选择 -->
    <section class="ufo-card p-4 space-y-4">
      <header class="border-b border-[var(--ufo-border)] pb-2">
        <h3 class="text-sm font-semibold text-[var(--ufo-text)]">地理位置</h3>
        <p class="text-xs text-[var(--ufo-muted)]">选择自动基于网络 IP 定位或精确指定城市</p>
      </header>

      <SettingsField
        label="定位模式"
        hint="推荐网络环境固定时开启自动定位"
        layout="stacked"
      >
        <BaseSegmentedControl
          v-model="locationModeInput"
          :options="locationModeOptions"
          @update:model-value="saveCommonSettings"
        />
      </SettingsField>

      <!-- 手动模式时的城市搜索 -->
      <template v-if="locationModeInput === 'manual'">
        <SettingsField
          label="当前指定城市"
          :hint="manualLocationNameInput ? `已选中: ${manualLocationNameInput} (ID: ${manualLocationInput})` : '尚未选择城市，请在下方搜索'"
          layout="stacked"
        >
          <div class="flex gap-2">
            <BaseInput
              v-model="citySearchQuery"
              placeholder="输入城市或区县名，如: 海淀 或 杭州"
              @keydown.enter="handleSearchCity"
            />
            <BaseButton
              variant="secondary"
              :disabled="isSearchingCity || !citySearchQuery.trim() || (!weatherStore.hasSavedKey && !apiKeyInput.trim())"
              @click="handleSearchCity"
            >
              {{ isSearchingCity ? '搜索中...' : '搜索' }}
            </BaseButton>
          </div>
        </SettingsField>

        <p v-if="searchError" class="text-xs text-[var(--ufo-danger)]" role="status">
          {{ searchError }}
        </p>

        <!-- 遵循规范第 9.3 节：选择列表项改用可聚焦按钮，支持键盘导航 -->
        <div
          v-if="citySearchResults.length > 0"
          class="max-h-40 overflow-y-auto space-y-1 p-2 bg-[var(--ufo-surface-2)] rounded border border-[var(--ufo-border)]"
        >
          <button
            v-for="city in citySearchResults"
            :key="city.id"
            type="button"
            class="w-full flex items-center justify-between p-2 rounded text-left text-xs transition-colors hover:bg-[var(--ufo-surface)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ufo-ring)]"
            @click="handleSelectCity(city)"
          >
            <span class="text-[var(--ufo-text)] font-medium">
              {{ city.adm1 }} - {{ city.name }}
            </span>
            <span class="text-[var(--ufo-muted)] text-[10px]">
              {{ city.adm2 }} · ID: {{ city.id }}
            </span>
          </button>
        </div>
      </template>
    </section>
  </div>
</template>
