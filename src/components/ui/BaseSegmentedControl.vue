<script setup lang="ts">
interface SegmentedOption {
  /** 选项值，对应 v-model 的值。 */
  value: string
  /** 显示文案。 */
  label: string
  /** 是否禁用该选项。 */
  disabled?: boolean
}

interface Props {
  /** 当前选中值；配合 v-model 使用。 */
  modelValue: string
  /** 可选项列表。 */
  options: readonly SegmentedOption[]
  /** 原生 radio group 名称。 */
  name: string
  /** radiogroup 的无障碍名称来源。 */
  ariaLabelledby?: string
}

const props = withDefaults(defineProps<Props>(), {
  ariaLabelledby: undefined,
})

const emit = defineEmits<{
  /** 选中值变化。 */
  'update:modelValue': [value: string]
  /** 原生 change 事件，保留给需要立即提交的页面。 */
  change: [event: Event]
}>()

/** 仅在值实际变化时同步 v-model 并通知父组件。 */
function onChange(option: SegmentedOption, event: Event) {
  if (option.disabled || option.value === props.modelValue) return
  emit('update:modelValue', option.value)
  emit('change', event)
}
</script>

<template>
  <div class="base-segmented" role="radiogroup" :aria-labelledby="ariaLabelledby">
    <label
      v-for="option in options"
      :key="option.value"
      class="base-segmented__option"
      :class="{
        'base-segmented__option--selected': option.value === modelValue,
        'base-segmented__option--disabled': option.disabled,
      }"
    >
      <input
        type="radio"
        :name="name"
        :value="option.value"
        :checked="option.value === modelValue"
        :disabled="option.disabled"
        @change="onChange(option, $event)"
      />
      <span>{{ option.label }}</span>
    </label>
  </div>
</template>

<style scoped>
.base-segmented {
  display: flex;
  width: 100%;
  padding: 3px;
  border: 1px solid var(--ufo-border);
  border-radius: 11px;
  background: color-mix(in srgb, var(--ufo-input) 94%, black 6%);
  box-shadow:
    inset 0 2px 5px rgba(0, 0, 0, 0.18),
    0 1px 0 rgba(255, 255, 255, 0.025);
}

.base-segmented__option {
  position: relative;
  display: inline-flex;
  min-width: 0;
  min-height: 30px;
  flex: 1;
  align-items: center;
  justify-content: center;
  padding: 0 11px;
  border-radius: 8px;
  color: var(--ufo-muted);
  font-size: 12px;
  font-weight: 500;
  /* 作为设置行右列控件时列宽按内容算：文案绝不折行（两行高的段控件会把整行撑歪） */
  white-space: nowrap;
  cursor: pointer;
  transition:
    background-color 150ms ease-out,
    box-shadow 150ms ease-out,
    color 150ms ease-out;
}

.base-segmented__option:hover {
  background: var(--ufo-overlay-raised);
  color: var(--ufo-text);
}

.base-segmented__option input {
  position: absolute;
  width: 1px;
  height: 1px;
  opacity: 0;
  pointer-events: none;
}

.base-segmented__option--selected {
  background: var(--ufo-accent-soft);
  color: var(--ufo-accent-text);
  font-weight: 600;
  box-shadow:
    inset 0 0 0 1px rgba(52, 211, 153, 0.22),
    0 1px 3px rgba(0, 0, 0, 0.18);
}

.base-segmented__option:has(input:focus-visible) {
  outline: 2px solid var(--ufo-ring);
  outline-offset: 1px;
}

.base-segmented__option--disabled {
  cursor: not-allowed;
  opacity: 0.42;
}

@media (prefers-reduced-motion: reduce) {
  .base-segmented__option {
    transition: none;
  }
}
</style>
