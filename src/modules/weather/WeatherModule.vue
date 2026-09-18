<script setup lang="ts">
import { computed, inject } from 'vue'
import type { FloatingLayout } from '@/lib/floating-layout'
import { floatingModuleKey } from '@/lib/floating-module-context'
import { weatherDemand } from './demand'
import { resolveWeatherIcon } from './lib/weather-icons'
import { useWeatherStore } from './stores/weather'

/**
 * 接收来自宿主的当前贴边形态
 */
const props = defineProps<{
  layoutMode: FloatingLayout
}>()

const moduleApi = inject(floatingModuleKey, null)
const weatherStore = useWeatherStore()

const snapshot = computed(() => weatherStore.snapshot)
const weatherIcon = computed(() => resolveWeatherIcon(snapshot.value?.icon_code))

/**
 * 遵循规范第 7.2 节：计算可用预算 budget = min(demand, allocatedCells)。
 * 当宿主被其他模块挤占导致分配到 1 格时（budget < 2），自动触发降级紧凑布局。
 */
const demand = computed(() => weatherDemand(props.layoutMode))
const budget = computed(() => Math.min(demand.value, moduleApi?.allocatedCells.value ?? demand.value))
const isBudgetConstrained = computed(() => budget.value < 2)

/**
 * 悬停时上报 hover key（宿主触发 tooltip 显隐）
 */
function handlePointerEnter(): void {
  moduleApi?.reportHover('weather-now')
}

function handlePointerLeave(): void {
  moduleApi?.reportHover(null)
}
</script>

<template>
  <!-- 遵循规范第 8.4 节：移除原生 title，无障碍信息使用 aria-label -->
  <div
    class="weather-module"
    :class="[
      `weather-module--${props.layoutMode}`,
      { 'weather-module--constrained': isBudgetConstrained },
    ]"
    role="region"
    :aria-label="snapshot ? `${snapshot.location_name} ${snapshot.text} ${Math.round(snapshot.temp)}度` : '和风天气'"
    @pointerenter="handlePointerEnter"
    @pointerleave="handlePointerLeave"
  >
    <!-- 未配置 API Key 或暂无数据状态 -->
    <div v-if="!snapshot" class="weather-module--empty">
      <component :is="weatherIcon" class="weather-icon" aria-hidden="true" />
      <span class="weather-temp">--°</span>
    </div>

    <!-- 当分到的格子数受到挤压（budget < 2）或 square 形态时：统一降级为极简紧凑堆叠 -->
    <template v-else-if="props.layoutMode === 'square' || isBudgetConstrained">
      <component :is="weatherIcon" class="weather-icon" aria-hidden="true" />
      <span class="weather-temp">{{ Math.round(snapshot.temp) }}°</span>
    </template>

    <!-- 2. 四角形态 (corner): 放大方形，图文更饱满 -->
    <template v-else-if="props.layoutMode === 'corner'">
      <div class="weather-top">
        <component :is="weatherIcon" class="weather-icon" aria-hidden="true" />
        <span class="weather-text">{{ snapshot.text }}</span>
      </div>
      <div class="weather-bottom">
        <span class="weather-temp">{{ Math.round(snapshot.temp) }}°C</span>
        <span class="weather-loc">
          {{ snapshot.location_name.split('-').pop() }}
        </span>
      </div>
    </template>

    <!-- 3. 横条形态 (horizontal): 左右两格分栏 -->
    <template v-else-if="props.layoutMode === 'horizontal'">
      <div class="weather-col-left">
        <component :is="weatherIcon" class="weather-icon" aria-hidden="true" />
        <span class="weather-text">{{ snapshot.text }}</span>
      </div>
      <div class="weather-col-right">
        <span class="weather-temp">{{ Math.round(snapshot.temp) }}°</span>
        <span class="weather-sub">{{ snapshot.humidity }}% 湿</span>
      </div>
    </template>

    <!-- 4. 竖条形态 (vertical): 上下两格堆叠 -->
    <template v-else-if="props.layoutMode === 'vertical'">
      <div class="weather-row-top">
        <component :is="weatherIcon" class="weather-icon" aria-hidden="true" />
        <span class="weather-text">{{ snapshot.text }}</span>
      </div>
      <div class="weather-row-bottom">
        <span class="weather-temp">{{ Math.round(snapshot.temp) }}°</span>
        <span class="weather-sub">{{ snapshot.wind_scale }}级风</span>
      </div>
    </template>
  </div>
</template>
