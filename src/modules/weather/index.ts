import type { FloatingModuleDefinition } from '@/lib/floating-modules'
import WeatherModule from './WeatherModule.vue'
import WeatherTooltip from './WeatherTooltip.vue'
import GeneralSection from './sections/GeneralSection.vue'
import { weatherDemand } from './demand'
import { useWeatherStore } from './stores/weather'

// 导入天气模块专用样式
import './weather.css'

/**
 * 和风天气悬浮窗模块定义
 *
 * - 恒定占用 2 个单元格 (weatherDemand)
 * - 悬停时通过 reportHover('weather-now') 唤起天气详情 Tooltip
 * - 主窗口提供 GeneralSection 单一常规设置分区，用于配置 API Key、定位和刷新周期
 */
export const weatherModule: FloatingModuleDefinition = {
  id: 'weather',
  label: '实时天气',
  description: '和风天气：实时温度与天气状况，悬停查看气压、风力及 24h 趋势。',
  version: 1,
  defaultEnabled: false,
  supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
  component: WeatherModule,
  demand: weatherDemand,
  settings: {
    // 单一常规设置分区，主窗口将自动隐藏二级 Tab
    sections: [
      { id: 'general', label: '常规设置', component: GeneralSection },
    ],
  },
  activate: () => {
    const store = useWeatherStore()
    void store.start()
    return () => {
      store.stop()
    }
  },
  tooltip: {
    component: WeatherTooltip,
    size: { width: 300, height: 320 },
    // 即使快照还未获取，只要处于 hover 状态也允许弹出卡片展示加载或已缓存天气
    hasContent: () => true,
  },
}
