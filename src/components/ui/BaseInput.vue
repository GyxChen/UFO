<script setup lang="ts">
interface Props {
  /** 当前值；配合 v-model 使用，数字输入可带 .number 修饰符。 */
  modelValue?: string | number
  /** Vue 透传的 v-model 修饰符；当前支持 .number。 */
  modelModifiers?: {
    /** 是否把输入值转换为数字。 */
    number?: boolean
  }
  /** 隐藏数字框两侧的上下微调箭头（设置页的时间类字段用：值有明确范围，箭头点击易误触）。 */
  noSpin?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  modelValue: '',
  modelModifiers: () => ({}),
  noSpin: false,
})

const emit = defineEmits<{
  /** 输入值变化，返回字符串或按 .number 转换后的数字。 */
  'update:modelValue': [value: string | number]
  /** 原生 change 事件，保留给失焦/回车后的提交逻辑。 */
  change: [event: Event]
}>()

/** 按 v-model 修饰符把原始输入转换成父组件需要的类型。 */
function normalizeValue(value: string): string | number {
  if (!props.modelModifiers.number || value === '') return value
  const numberValue = Number.parseFloat(value)
  return Number.isNaN(numberValue) ? value : numberValue
}

/** 输入时持续同步 v-model，保持与原生输入框一致的手感。 */
function onInput(event: Event) {
  const value = (event.target as HTMLInputElement).value
  emit('update:modelValue', normalizeValue(value))
}

/** 把原生 change 转发给父组件，用于提交或校验。 */
function onChange(event: Event) {
  emit('change', event)
}
</script>

<template>
  <input
    class="base-input"
    :class="{ 'base-input--no-spin': noSpin }"
    :value="modelValue"
    @input="onInput"
    @change="onChange"
  />
</template>

<style scoped>
.base-input {
  box-sizing: border-box;
  width: 100%;
  height: 38px;
  /*
   * ⚠️ 这里**不能**写 `margin: 0`：Tailwind 的 preflight 已经把所有元素的 margin 归零，
   * 这一条是多余的，而它的优先级（`.base-input[data-v-…]`）高过 `space-y-*` ——
   * 后者编译出来是 `:where(...)`，**优先级为 0**。于是「输入框 + 按钮」这类
   * `space-y-2` 排出来的间距会被静默吃掉，输入框与下一行贴在一起。
   * 归零交给 preflight，间距交给调用方的 gap / margin。
   */
  padding: 0 12px;
  border: 1px solid var(--ufo-border-strong);
  border-radius: 10px;
  outline: none;
  background: var(--ufo-input);
  color: var(--ufo-text);
  font: inherit;
  font-size: 13px;
  font-variant-numeric: tabular-nums;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.035),
    0 1px 2px rgba(0, 0, 0, 0.12);
  transition:
    background-color 150ms ease-out,
    border-color 150ms ease-out,
    box-shadow 150ms ease-out;
}

.base-input::placeholder {
  color: color-mix(in srgb, var(--ufo-muted) 76%, transparent);
  opacity: 1;
}

.base-input:hover:not(:disabled) {
  border-color: var(--ufo-border-strong);
  background: color-mix(in srgb, var(--ufo-input) 96%, white 4%);
}

.base-input:focus {
  border-color: var(--ufo-accent);
  box-shadow:
    0 0 0 3px var(--ufo-accent-soft),
    inset 0 1px 0 rgba(255, 255, 255, 0.045);
}

.base-input:disabled {
  cursor: not-allowed;
  opacity: 0.48;
}

.base-input[type='number']::-webkit-inner-spin-button,
.base-input[type='number']::-webkit-outer-spin-button {
  opacity: 0.58;
}

/* 隐藏上下箭头：Chromium 下 appearance:none 即整块消失。 */
.base-input--no-spin[type='number']::-webkit-inner-spin-button,
.base-input--no-spin[type='number']::-webkit-outer-spin-button {
  -webkit-appearance: none;
  appearance: none;
  margin: 0;
}

@media (prefers-reduced-motion: reduce) {
  .base-input {
    transition: none;
  }
}
</style>
