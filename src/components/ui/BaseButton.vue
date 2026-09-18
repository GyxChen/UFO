<script setup lang="ts">
import { computed } from 'vue'

/** 按钮视觉层级；主操作使用 primary，次操作使用 ghost，危险操作使用 danger。 */
type ButtonVariant = 'primary' | 'ghost' | 'danger'

/** 按钮尺寸；sm 用于列表行等紧凑位置。 */
type ButtonSize = 'sm' | 'md'

interface Props {
  /** 按钮语义类型，默认 button，避免表单里的普通按钮误触提交。 */
  type?: 'button' | 'submit' | 'reset'
  /** 视觉层级。 */
  variant?: ButtonVariant
  /** 控件尺寸。 */
  size?: ButtonSize
  /** 是否禁用；loading 时也会自动禁用，避免重复触发。 */
  disabled?: boolean
  /** 是否显示加载指示；仅负责视觉与禁用，不改变业务文案。 */
  loading?: boolean
  /** 危险操作的二次确认态，用于切换警示底色。 */
  armed?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  type: 'button',
  variant: 'ghost',
  size: 'md',
  disabled: false,
  loading: false,
  armed: false,
})

/** 汇总按钮状态类，模板里只保留语义属性。 */
const buttonClass = computed(() => [
  `base-button--${props.size}`,
  `base-button--${props.variant}`,
  {
    'base-button--armed': props.armed,
    'base-button--loading': props.loading,
  },
])
</script>

<template>
  <button
    :type="type"
    class="base-button"
    :class="buttonClass"
    :disabled="disabled || loading"
    :aria-busy="loading || undefined"
  >
    <span v-if="loading" class="base-button__spinner" aria-hidden="true" />
    <slot />
  </button>
</template>

<style scoped>
.base-button {
  position: relative;
  display: inline-flex;
  min-width: 0;
  height: 38px;
  align-items: center;
  justify-content: center;
  gap: 7px;
  padding: 0 14px;
  border: 1px solid transparent;
  border-radius: 10px;
  font: inherit;
  font-size: 13px;
  font-weight: 600;
  line-height: 1;
  white-space: nowrap;
  cursor: pointer;
  user-select: none;
  outline: none;
  -webkit-tap-highlight-color: transparent;
  transition:
    background-color 150ms ease-out,
    border-color 150ms ease-out,
    box-shadow 150ms ease-out,
    color 150ms ease-out,
    transform 150ms ease-out,
    opacity 150ms ease-out;
}

.base-button:hover:not(:disabled) {
  transform: translateY(-1px);
}

.base-button:active:not(:disabled) {
  transform: translateY(0) scale(0.98);
  transition-duration: 70ms;
}

.base-button:focus-visible {
  outline: 2px solid var(--ufo-ring);
  outline-offset: 2px;
}

.base-button:disabled {
  cursor: not-allowed;
  opacity: 0.45;
  transform: none;
}

.base-button--md {
  height: 38px;
}

.base-button--sm {
  height: 30px;
  padding-inline: 10px;
  border-radius: 9px;
  font-size: 12px;
}

.base-button--primary {
  border-color: color-mix(in srgb, var(--ufo-accent) 72%, white 10%);
  background: var(--ufo-accent);
  color: var(--ufo-accent-on);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.28),
    0 6px 16px rgba(52, 211, 153, 0.12);
}

.base-button--primary:hover:not(:disabled) {
  background: color-mix(in srgb, var(--ufo-accent) 92%, white 8%);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.34),
    0 8px 20px rgba(52, 211, 153, 0.18);
}

.base-button--ghost {
  border-color: var(--ufo-border-strong);
  background: color-mix(in srgb, var(--ufo-surface-2) 72%, transparent);
  color: var(--ufo-text);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.025);
}

.base-button--ghost:hover:not(:disabled) {
  border-color: rgba(255, 255, 255, 0.24);
  background: color-mix(in srgb, var(--ufo-surface-2) 88%, white 4%);
}

.base-button--danger {
  border-color: color-mix(in srgb, var(--ufo-danger) 42%, transparent);
  background: color-mix(in srgb, var(--ufo-danger) 7%, transparent);
  color: var(--ufo-danger);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.025);
}

.base-button--danger:hover:not(:disabled) {
  border-color: color-mix(in srgb, var(--ufo-danger) 60%, transparent);
  background: color-mix(in srgb, var(--ufo-danger) 14%, transparent);
}

.base-button--danger.base-button--armed {
  border-color: color-mix(in srgb, var(--ufo-danger) 62%, transparent);
  background: color-mix(in srgb, var(--ufo-danger) 22%, transparent);
  box-shadow: 0 0 0 3px rgba(248, 113, 113, 0.08);
  font-weight: 600;
}

.base-button__spinner {
  width: 13px;
  height: 13px;
  flex: 0 0 auto;
  border: 2px solid currentColor;
  border-right-color: transparent;
  border-radius: 999px;
  animation: base-button-spin 700ms linear infinite;
  opacity: 0.78;
}

@keyframes base-button-spin {
  to {
    transform: rotate(360deg);
  }
}

@media (prefers-reduced-motion: reduce) {
  .base-button {
    transition: none;
  }

  .base-button:hover:not(:disabled),
  .base-button:active:not(:disabled) {
    transform: none;
  }

  .base-button__spinner {
    animation-duration: 1.4s;
  }
}
</style>
