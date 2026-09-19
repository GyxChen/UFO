<script setup lang="ts">
import { computed } from 'vue'
import type { FloatingHoverTarget } from '@/lib/floating-module-context'
import TooltipRow from '@/components/floating/tooltip/TooltipRow.vue'
import TooltipSection from '@/components/floating/tooltip/TooltipSection.vue'
import { resolveWeatherIcon } from './lib/weather-icons'
import { formatWeatherUpdatedTime, formatWind } from './lib/weather-format'
import { useWeatherStore } from './stores/weather'

/**
 * 和风天气的悬停信息卡。
 *
 * 提示窗按注册表挂载本组件并透传悬停目标（`target`）：声明成 prop 才是「组件自己的契约」，
 * 否则会被当成多余的透传属性 —— fragment 根节点接不住它，控制台每次悬停都报一次警告。
 * 内容组件用多根节点渲染（`.tooltip-shell` 直接是行级 flex 容器），所以关掉属性透传。
 */
defineOptions({ inheritAttrs: false })

defineProps<{
  /** 悬停目标（哪个模块的哪个对象）；本卡是整块聚合信息，只用它满足提示窗契约。 */
  target: FloatingHoverTarget
}>()

const weatherStore = useWeatherStore()
const snapshot = computed(() => weatherStore.snapshot)
const weatherIcon = computed(() => resolveWeatherIcon(snapshot.value?.icon_code))
</script>

<template>
  <template v-if="snapshot">
    <!-- 基础状况：位置与实时气象 -->
    <TooltipRow label="位置" :value="snapshot.location_name" />
    <TooltipRow label="天气">
      <div class="weather-tooltip__now">
        <component :is="weatherIcon" class="weather-tooltip__icon" />
        <span>{{ snapshot.text }} {{ Math.round(snapshot.temp) }}°C (体感 {{ Math.round(snapshot.feels_like) }}°C)</span>
      </div>
    </TooltipRow>
    <TooltipRow label="风向风速" :value="formatWind(snapshot.wind_dir, snapshot.wind_scale)" />
    <TooltipRow label="空气湿度" :value="`${snapshot.humidity}%`" mono />

    <!-- 拓展指标 -->
    <TooltipSection>
      <TooltipRow label="大气压强" :value="`${snapshot.pressure} hPa`" mono />
      <TooltipRow label="能见度" :value="`${snapshot.vis} km`" mono />
      <TooltipRow
        label="更新时间"
        :value="formatWeatherUpdatedTime(snapshot.updated_at_ms)"
        tone="muted"
      />
    </TooltipSection>

    <!-- 未来 24 小时简要预报趋势 -->
    <TooltipSection v-if="snapshot.hourly_forecast && snapshot.hourly_forecast.length > 0">
      <div class="weather-tooltip__hourly">
        <div
          v-for="item in snapshot.hourly_forecast"
          :key="item.time"
          class="weather-tooltip__hourly-item"
        >
          <span class="weather-tooltip__hourly-time">{{ item.time }}</span>
          <component
            :is="resolveWeatherIcon(item.icon_code)"
            class="weather-tooltip__hourly-icon"
          />
          <span class="weather-tooltip__hourly-temp">{{ Math.round(item.temp) }}°</span>
        </div>
      </div>
    </TooltipSection>
  </template>

  <!-- 暂无天气快照或未配置 API Key 时的提示 -->
  <template v-else>
    <TooltipRow label="状态" :value="weatherStore.lastError || '正在获取天气数据...'" tone="muted" />
    <TooltipSection>
      <TooltipRow label="提示" value="请在主窗口「实时天气」设置中填写和风天气 API Key" />
    </TooltipSection>
  </template>
</template>
