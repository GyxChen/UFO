<script setup lang="ts">
import { computed } from 'vue'

interface Props {
  /** 当前值；配合 v-model 使用。 */
  modelValue: number
  /** 可选最小值。 */
  min: number
  /** 可选最大值。 */
  max: number
  /** 步进；默认 1。 */
  step?: number
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
  </div>
</template>

<style scoped>
/* 外层只负责撑满可用宽度并留出可点高度（原生 range 的命中区偏小）。 */
.base-slider {
  display: flex;
  width: 100%;
  min-width: 0;
  height: 26px;
  align-items: center;
}

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
  width: 14px;
  height: 14px;
  margin-top: -4.5px;
  border: 0;
  border-radius: 999px;
  appearance: none;
  background: #f4f4f5;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.42);
}

.base-slider__input::-moz-range-thumb {
  width: 14px;
  height: 14px;
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
</style>