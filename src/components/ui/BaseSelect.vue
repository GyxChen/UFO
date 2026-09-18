<script setup lang="ts">
defineOptions({ inheritAttrs: false })

interface Props {
  /** 当前选中值；配合 v-model 使用。 */
  modelValue?: string | number
  /** 是否禁用。 */
  disabled?: boolean
}

withDefaults(defineProps<Props>(), {
  modelValue: '',
  disabled: false,
})

const emit = defineEmits<{
  /** 选中项变化。 */
  'update:modelValue': [value: string | number]
  /** 原生 change 事件，保留给需要立即提交的页面。 */
  change: [event: Event]
}>()

/** 同步选中值后，再把事件交给父组件触发业务更新。 */
function onChange(event: Event) {
  emit('update:modelValue', (event.target as HTMLSelectElement).value)
  emit('change', event)
}
</script>

<template>
  <div class="base-select" :class="{ 'base-select--disabled': disabled }">
    <select class="base-select__control" :value="modelValue" :disabled="disabled" v-bind="$attrs" @change="onChange">
      <slot />
    </select>
    <svg class="base-select__icon" viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="m6 9 6 6 6-6"
        fill="none"
        stroke="currentColor"
        stroke-linecap="round"
        stroke-linejoin="round"
        stroke-width="2"
      />
    </svg>
  </div>
</template>

<style scoped>
.base-select {
  position: relative;
  width: 100%;
}

.base-select__control {
  box-sizing: border-box;
  width: 100%;
  height: 38px;
  margin: 0;
  padding: 0 36px 0 12px;
  appearance: none;
  border: 1px solid var(--ufo-border-strong);
  border-radius: 10px;
  outline: none;
  background: var(--ufo-input);
  color: var(--ufo-text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.035),
    0 1px 2px rgba(0, 0, 0, 0.12);
  transition:
    background-color 150ms ease-out,
    border-color 150ms ease-out,
    box-shadow 150ms ease-out;
}

.base-select__control:hover:not(:disabled) {
  border-color: rgba(255, 255, 255, 0.24);
  background: color-mix(in srgb, var(--ufo-input) 96%, white 4%);
}

.base-select__control:focus-visible {
  border-color: var(--ufo-accent);
  box-shadow:
    0 0 0 3px var(--ufo-accent-soft),
    inset 0 1px 0 rgba(255, 255, 255, 0.045);
}

.base-select__control:disabled {
  cursor: not-allowed;
  opacity: 0.48;
}

.base-select__icon {
  position: absolute;
  top: 50%;
  right: 12px;
  width: 14px;
  height: 14px;
  color: var(--ufo-muted);
  pointer-events: none;
  transform: translateY(-50%);
  transition:
    color 150ms ease-out,
    transform 150ms ease-out;
}

.base-select:hover .base-select__icon,
.base-select:focus-within .base-select__icon {
  color: var(--ufo-text);
}

.base-select:focus-within .base-select__icon {
  color: var(--ufo-accent);
}

.base-select :deep(option) {
  background: var(--ufo-input);
  color: var(--ufo-text);
}

.base-select--disabled {
  opacity: 0.48;
}

@media (prefers-reduced-motion: reduce) {
  .base-select__control,
  .base-select__icon {
    transition: none;
  }
}
</style>
