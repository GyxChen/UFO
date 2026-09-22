<script setup lang="ts">
import { computed } from 'vue'

/** 一枚刻度：轨道上的一个参照取值，及其下方的标注文本。 */
interface SliderTick {
  /** 刻度对应的取值（超出 min..max 会被夹回边界）。 */
  value: number
  /** 刻度下方的标注（如 `3×3`）；不给就只画刻度点。 */
  label?: string
}

interface Props {
  /** 当前值；配合 v-model 使用。 */
  modelValue: number
  /** 可选最小值。 */
  min: number
  /** 可选最大值。 */
  max: number
  /** 步进；默认 1。 */
  step?: number
  /** 轨道下方的刻度（可选）：离散档位滑块用它把每一档标出来，连续取值不用传。 */
  ticks?: SliderTick[]
  /** 是否禁用。 */
  disabled?: boolean
  /** 读屏名称；视觉标签通常由相邻的 SettingsField 承担。 */
  ariaLabel?: string
  /** 读屏名称的引用 id，与 SettingsField 的 labelId 配对。 */
  ariaLabelledby?: string
  /** 读屏播报的取值文本（如 `45 像素`），给纯数字补上单位。 */
  ariaValueText?: string
}

const props = withDefaults(defineProps<Props>(), {
  step: 1,
  ticks: undefined,
  disabled: false,
  ariaLabel: undefined,
  ariaLabelledby: undefined,
  ariaValueText: undefined,
})

const emit = defineEmits<{
  /** 拖动过程中的连续取值（只驱动界面，不落盘）。 */
  'update:modelValue': [value: number]
  /** 松手 / 键盘调整完成后的提交事件；页面用它落盘，避免拖动中反复写配置。 */
  change: [event: Event]
}>()

/** 已填充比例（0–100），写成 CSS 变量交给轨道画进度色。 */
const fillPercent = computed(() => {
  const span = props.max - props.min
  if (!Number.isFinite(span) || span <= 0) return 0
  const ratio = ((props.modelValue - props.min) / span) * 100
  return Math.min(100, Math.max(0, ratio))
})

/**
 * 带位置信息的刻度列表：`left` 是刻度点在轨道上的横向位置（0–100%）。
 * 原生 range 的拇指中心随值移动，两端各占半个拇指宽 —— 刻度要跟拇指中心对齐就得做同样的缩进
 * （`calc(<比例>% + (0.5 − 比例) × 拇指宽)`），否则首尾刻度会悬在轨道外。
 * 比例在这里算好（偏移系数四舍五入去浮点噪声），拇指宽交给 CSS（`--base-slider-thumb-size`）。
 */
const positionedTicks = computed(() => {
  const span = props.max - props.min
  if (!props.ticks?.length || !Number.isFinite(span) || span <= 0) return []
  return props.ticks.map(tick => {
    const value = Math.min(props.max, Math.max(props.min, tick.value))
    const ratio = (value - props.min) / span
    const offset = Math.round((0.5 - ratio) * 1000) / 1000
    return { ...tick, left: ratio * 100, offset, active: value === props.modelValue }
  })
})

/** 拖动过程中同步取值；只更新本地状态，提交交给 change。 */
function onInput(event: Event) {
  const value = Number((event.target as HTMLInputElement).value)
  if (!Number.isFinite(value)) return
  emit('update:modelValue', value)
}

/** 松手 / 键盘调整后转发原生 change，由父组件决定何时落盘。 */
function onChange(event: Event) {
  emit('change', event)
}
</script>

<template>
  <div class="base-slider" :style="{ '--base-slider-fill': `${fillPercent}%` }">
    <input
      class="base-slider__input"
      type="range"
      :value="modelValue"
      :min="min"
      :max="max"
      :step="step"
      :disabled="disabled"
      :aria-label="ariaLabel"
      :aria-labelledby="ariaLabelledby"
      :aria-valuetext="ariaValueText"
      @input="onInput"
      @change="onChange"
    />
    <!-- 刻度与标注叠在滑块下方；pointer-events:none 让点击/拖拽穿透到原生 range。 -->
    <div v-if="positionedTicks.length" class="base-slider__ticks" aria-hidden="true">
      <span
        v-for="tick in positionedTicks"
        :key="tick.value"
        class="base-slider__tick"
        :class="{ 'base-slider__tick--active': tick.active }"
        :style="{ left: `calc(${tick.left}% + (${tick.offset} * var(--base-slider-thumb-size)))` }"
      >
        <i class="base-slider__tick-mark" />
        <span v-if="tick.label" class="base-slider__tick-label">{{ tick.label }}</span>
      </span>
    </div>
  </div>
</template>

<style scoped>
/*
 * 外层撑满可用宽度；不传刻度时高度就是滑块行本身（26px），
 * 传了刻度才由刻度行把整体撑高，调用方无需关心两种形态的布局差异。
 * 拇指直径收进变量：刻度定位的 calc（模板里）与拇指样式共用一份，改一处即两处同步。
 */
.base-slider {
  --base-slider-thumb-size: 14px;
  display: flex;
  flex-direction: column;
  width: 100%;
  min-width: 0;
  align-items: stretch;
}

/* 滑块行：留出可点高度（原生 range 的命中区偏小）。 */
.base-slider__input {
  width: 100%;
  height: 26px;
  margin: 0;
  padding: 0;
  border: 0;
  outline: none;
  appearance: none;
  background: transparent;
  cursor: pointer;
}

/* 轨道：底色为输入底，已选段用强调色填充（--base-slider-fill 由模板按当前值写入）。 */
.base-slider__input::-webkit-slider-runnable-track {
  height: 5px;
  border-radius: 999px;
  background: linear-gradient(
    to right,
    var(--ufo-accent) 0 var(--base-slider-fill),
    var(--ufo-input) var(--base-slider-fill) 100%
  );
  box-shadow: inset 0 0 0 1px var(--ufo-border-strong);
}

.base-slider__input::-moz-range-track {
  height: 5px;
  border-radius: 999px;
  background: linear-gradient(
    to right,
    var(--ufo-accent) 0 var(--base-slider-fill),
    var(--ufo-input) var(--base-slider-fill) 100%
  );
  box-shadow: inset 0 0 0 1px var(--ufo-border-strong);
}

/* 拇指：白圆点，向上移 (5 − 14) / 2 使其与轨道垂直居中。 */
.base-slider__input::-webkit-slider-thumb {
  width: var(--base-slider-thumb-size);
  height: var(--base-slider-thumb-size);
  margin-top: -4.5px;
  border: 0;
  border-radius: 999px;
  appearance: none;
  background: #f4f4f5;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.42);
}

.base-slider__input::-moz-range-thumb {
  width: var(--base-slider-thumb-size);
  height: var(--base-slider-thumb-size);
  border: 0;
  border-radius: 999px;
  background: #f4f4f5;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.42);
}

.base-slider__input:hover:not(:disabled)::-webkit-slider-thumb {
  background: #ffffff;
}

.base-slider__input:focus-visible::-webkit-slider-thumb {
  box-shadow:
    0 1px 3px rgba(0, 0, 0, 0.42),
    0 0 0 3px var(--ufo-ring);
}

.base-slider__input:focus-visible::-moz-range-thumb {
  box-shadow:
    0 1px 3px rgba(0, 0, 0, 0.42),
    0 0 0 3px var(--ufo-ring);
}

.base-slider__input:disabled {
  cursor: not-allowed;
  opacity: 0.48;
}

/*
 * 刻度行：叠在轨道正下方。负上边距把刻度点贴到轨道底边，
 * pointer-events:none 让点击 / 拖拽直接穿透到上面的原生 range。
 */
.base-slider__ticks {
  position: relative;
  height: 16px;
  margin-top: -8px;
  pointer-events: none;
  user-select: none;
}

/* 单枚刻度：以 left 定位点为轴水平居中（left 已含拇指宽偏移，与拇指中心对齐）。 */
.base-slider__tick {
  position: absolute;
  top: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 3px;
  transform: translateX(-50%);
}

/* 刻度点：2×4 竖线，落在轨道底边上。 */
.base-slider__tick-mark {
  width: 2px;
  height: 4px;
  border-radius: 1px;
  background: var(--ufo-border-strong);
}

/* 当前档的刻度加亮，与轨道的强调色填充呼应。 */
.base-slider__tick--active .base-slider__tick-mark {
  background: var(--ufo-accent);
}

/* 刻度标注：刻度下面的尺寸文字（如 3×3）。 */
.base-slider__tick-label {
  font-size: 10px;
  line-height: 1;
  white-space: nowrap;
  color: var(--ufo-muted);
  font-variant-numeric: tabular-nums;
}

.base-slider__tick--active .base-slider__tick-label {
  color: var(--ufo-text);
  font-weight: 600;
}
</style>