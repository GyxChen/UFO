<script setup lang="ts">
interface Props {
  /** 当前开关状态；配合 v-model 使用。 */
  modelValue: boolean
  /** 是否禁用。 */
  disabled?: boolean
  /** 读屏名称；视觉上通常由相邻文案承担。 */
  ariaLabel?: string
  /** 鼠标悬停提示；为空时不渲染原生提示。 */
  title?: string
}

withDefaults(defineProps<Props>(), {
  disabled: false,
  ariaLabel: undefined,
  title: undefined,
})

const emit = defineEmits<{
  /** 开关状态变化。 */
  'update:modelValue': [value: boolean]
  /** 原生 change 事件，保留给需要失败回滚的页面。 */
  change: [event: Event]
}>()

/** 先同步 v-model，再把事件交给父组件执行异步更新。 */
function onChange(event: Event) {
  emit('update:modelValue', (event.target as HTMLInputElement).checked)
  emit('change', event)
}
</script>

<template>
  <label class="base-switch" :class="{ 'base-switch--disabled': disabled }" :title="title">
    <input
      class="base-switch__input"
      type="checkbox"
      :checked="modelValue"
      :disabled="disabled"
      :aria-label="ariaLabel"
      @change="onChange"
    />
    <span class="base-switch__track" aria-hidden="true">
      <span class="base-switch__thumb" />
    </span>
  </label>
</template>

<style scoped>
.base-switch {
  position: relative;
  display: inline-flex;
  width: 46px;
  height: 26px;
  flex: 0 0 auto;
}

.base-switch__input {
  position: absolute;
  inset: 0;
  z-index: 2;
  width: 100%;
  height: 100%;
  margin: 0;
  cursor: pointer;
  opacity: 0;
}

.base-switch__track {
  position: absolute;
  inset: 0;
  border: 1px solid var(--ufo-border-strong);
  border-radius: 999px;
  background: var(--ufo-switch-track);
  box-shadow: inset 0 2px 5px rgba(0, 0, 0, 0.28);
  transition:
    background-color 160ms ease-out,
    border-color 160ms ease-out,
    box-shadow 160ms ease-out;
}

.base-switch__thumb {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 18px;
  height: 18px;
  border-radius: 999px;
  background: var(--ufo-control-thumb);
  box-shadow:
    0 2px 5px rgba(0, 0, 0, 0.34),
    inset 0 -1px 0 rgba(0, 0, 0, 0.12);
  transition: transform 160ms ease-out;
}

.base-switch__input:checked + .base-switch__track {
  border-color: color-mix(in srgb, var(--ufo-accent) 78%, white 8%);
  background: var(--ufo-accent);
  box-shadow:
    inset 0 1px 1px rgba(0, 0, 0, 0.16),
    0 0 0 3px var(--ufo-accent-soft);
}

.base-switch__input:checked + .base-switch__track .base-switch__thumb {
  transform: translateX(20px);
}

.base-switch__input:hover + .base-switch__track {
  border-color: var(--ufo-border-strong);
}

.base-switch__input:focus-visible + .base-switch__track {
  outline: 2px solid var(--ufo-ring);
  outline-offset: 2px;
}

.base-switch--disabled {
  opacity: 0.48;
}

.base-switch--disabled .base-switch__input {
  cursor: not-allowed;
}

@media (prefers-reduced-motion: reduce) {
  .base-switch__track,
  .base-switch__thumb {
    transition: none;
  }
}
</style>
