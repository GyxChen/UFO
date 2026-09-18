import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { ref, watch, type WatchStopHandle } from 'vue'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import { isModuleEnabled } from '@/lib/floating-modules'
import { weatherSettings, type WeatherSettings } from '../settings'

/**
 * 逐小时天气数据
 */
export interface HourlyWeatherItem {
  time: string
  temp: number
  icon_code: string
  text: string
}

/**
 * 天气状况快照
 */
export interface WeatherSnapshot {
  location_name: string
  location_id: string
  temp: number
  feels_like: number
  text: string
  icon_code: string
  wind_dir: string
  wind_scale: string
  humidity: number
  pressure: number
  vis: number
  updated_at_ms: number
  hourly_forecast: HourlyWeatherItem[]
}

/**
 * 城市检索命中项
 */
export interface CityLookupItem {
  id: string
  name: string
  adm1: string
  adm2: string
  country: string
}

/**
 * 天气模块 Pinia 状态管理
 */
export const useWeatherStore = defineStore('weather', () => {
  const snapshot = ref<WeatherSnapshot | null>(null)
  const isLoading = ref(false)
  const lastError = ref<string | null>(null)
  const hasSavedKey = ref(false)

  let timerId: number | null = null
  let stopListener: UnlistenFn | undefined
  let stopConfigWatch: WatchStopHandle | undefined

  const windowLabel = getCurrentWindow().label
  // 遵循规范第 4.3 节：指定常驻的 floating 窗口为唯一的轮询 owner
  const isOwnerWindow = windowLabel === 'floating'

  /**
   * 接收来自其他窗口或自身命令广播的最新天气快照
   */
  function applySnapshot(data: WeatherSnapshot): void {
    snapshot.value = data
    lastError.value = null
  }

  /**
   * 拉取最新天气数据
   *
   * @param forceRefresh 是否强制绕过后端缓存发起真实请求
   */
  async function fetchSnapshot(forceRefresh = false): Promise<WeatherSnapshot | null> {
    const modulesStore = useFloatingModulesStore()
    const settings = weatherSettings(modulesStore.config)

    if (!settings.has_key) {
      lastError.value = '未配置和风天气 API Key'
      return null
    }

    isLoading.value = true
    try {
      const data = await invoke<WeatherSnapshot>('get_weather_snapshot', {
        forceRefresh,
      })
      applySnapshot(data)
      return data
    } catch (err) {
      const msg = typeof err === 'string' ? err : String(err)
      lastError.value = msg
      return null
    } finally {
      isLoading.value = false
    }
  }

  /**
   * 重启定时轮询任务（仅 owner 窗口）
   */
  function restartPolling(intervalMin: number): void {
    stopPolling()
    const intervalMs = Math.max(intervalMin, 10) * 60 * 1000
    timerId = window.setInterval(() => {
      void fetchSnapshot(false)
    }, intervalMs)
  }

  /**
   * 停止定时轮询任务
   */
  function stopPolling(): void {
    if (timerId !== null) {
      window.clearInterval(timerId)
      timerId = null
    }
  }

  /**
   * 检查系统凭据管理器中是否已存储 Key
   */
  async function checkKeyStatus(): Promise<boolean> {
    try {
      const status = await invoke<boolean>('get_weather_secret_status')
      hasSavedKey.value = status
      return status
    } catch {
      hasSavedKey.value = false
      return false
    }
  }

  /**
   * 保存 API Key 到系统凭据管理器（仅主窗口调用）
   */
  async function saveApiKey(key: string): Promise<void> {
    const trimmed = key.trim()
    if (!trimmed) {
      await deleteApiKey()
      return
    }
    await invoke('set_weather_secret', { apiKey: trimmed })
    hasSavedKey.value = true
    const modulesStore = useFloatingModulesStore()
    await modulesStore.updateModuleSettings('weather', { has_key: true })
    void fetchSnapshot(true)
  }

  /**
   * 从系统凭据管理器删除 API Key（仅主窗口调用）
   */
  async function deleteApiKey(): Promise<void> {
    await invoke('delete_weather_secret')
    hasSavedKey.value = false
    snapshot.value = null
    const modulesStore = useFloatingModulesStore()
    await modulesStore.updateModuleSettings('weather', { has_key: false })
  }

  /**
   * 根据当前配置同步组件工作状态（启用/禁用响应、动态刷新间隔）
   */
  function syncFromConfig(): void {
    const modulesStore = useFloatingModulesStore()
    const enabled = isModuleEnabled(modulesStore.config, 'weather', false)
    const settings = weatherSettings(modulesStore.config)

    // 遵循规范第 1.8 节：关掉组件就是停止后台工作
    if (!enabled || !settings.has_key) {
      stopPolling()
      return
    }

    if (!isOwnerWindow) {
      // 非 owner 窗口只做一次只读补齐，不跑轮询定时器
      if (!snapshot.value) {
        void fetchSnapshot(false)
      }
      return
    }

    // Owner 窗口：启动/重设轮询定时器并立即抓取
    restartPolling(settings.refresh_interval_min)
    void fetchSnapshot(false)
  }

  /**
   * 启动生命周期（由模块定义的 activate 调用）
   */
  async function start(): Promise<void> {
    // 1. 等待外壳配置就绪
    const modulesStore = useFloatingModulesStore()
    await modulesStore.start()

    // 2. 注册跨窗口事件监听（广播数据）
    if (!stopListener) {
      try {
        stopListener = await listen<WeatherSnapshot>('weather-snapshot', event => {
          applySnapshot(event.payload)
        })
      } catch (error) {
        console.warn('weather-snapshot listen failed', error)
      }
    }

    // 3. 订阅配置与启用状态变化
    stopConfigWatch ??= watch(
      () => [
        isModuleEnabled(modulesStore.config, 'weather', false),
        weatherSettings(modulesStore.config),
      ],
      () => {
        syncFromConfig()
      },
      { deep: true, immediate: true }
    )
  }

  /**
   * 停止生命周期
   */
  function stop(): void {
    stopPolling()
    stopListener?.()
    stopListener = undefined
    stopConfigWatch?.()
    stopConfigWatch = undefined
  }

  /**
   * 关键字检索城市列表
   */
  async function searchCity(
    query: string,
    settings: WeatherSettings,
    overrideApiKey?: string,
  ): Promise<CityLookupItem[]> {
    if (!query.trim()) return []
    return await invoke<CityLookupItem[]>('search_weather_city', {
      query: query.trim(),
      hostType: settings.host_type,
      overrideApiKey: overrideApiKey ? overrideApiKey.trim() : null,
    })
  }

  /**
   * 测试连接与定位
   */
  async function testConnection(
    settings: WeatherSettings,
    overrideApiKey?: string,
  ): Promise<WeatherSnapshot> {
    return await invoke<WeatherSnapshot>('test_weather_connection', {
      hostType: settings.host_type,
      locationMode: settings.location_mode,
      manualLocation: settings.manual_location,
      manualLocationName: settings.manual_location_name,
      overrideApiKey: overrideApiKey ? overrideApiKey.trim() : null,
    })
  }

  return {
    snapshot,
    isLoading,
    lastError,
    hasSavedKey,
    fetchSnapshot,
    start,
    stop,
    checkKeyStatus,
    saveApiKey,
    deleteApiKey,
    searchCity,
    testConnection,
  }
})
