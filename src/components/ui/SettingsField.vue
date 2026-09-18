<script setup lang="ts">
interface Props {
  /** 可见字段标签。 */
  label?: string
  /** 标签自身的 id，适合由控件通过 aria-labelledby 引用。 */
  labelId?: string
  /** 原生 label 的 for 目标，适合 input 等可直接关联的控件。 */
  labelFor?: string
  /** 控件下方的辅助说明。 */
  hint?: string
}

withDefaults(defineProps<Props>(), {
  label: '',
  labelId: undefined,
  labelFor: undefined,
  hint: '',
})
</script>

<template>
  <div class="settings-field">
    <label v-if="label && labelFor" class="settings-field__label" :for="labelFor">
      {{ label }}
    </label>
    <span v-else-if="label" :id="labelId" class="settings-field__label">
      {{ label }}
    </span>

    <slot />

    <p v-if="hint" class="settings-field__hint">{{ hint }}</p>
  </div>
</template>

<style scoped>
.settings-field {
  display: grid;
  min-width: 0;
  gap: 7px;
}

.settings-field__label {
  color: var(--ufo-muted);
  font-size: 12px;
  line-height: 1.35;
}

.settings-field__hint {
  margin: -1px 0 0;
  padding: 0;
  color: color-mix(in srgb, var(--ufo-muted) 86%, transparent);
  font-size: 11px;
  line-height: 1.45;
}
</style>
