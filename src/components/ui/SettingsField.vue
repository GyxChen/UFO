<script setup lang="ts">
/**
 * 设置页的字段原语：一行 = 左读右调。
 *
 * 默认版式（所有设置行统一）：
 * - 左列：标题行（14px/600 文本 + 问号角标，**垂直居中**对齐），标题下面可选一行状态读数；
 * - 右列：交互组件本身（默认插槽），贴右、相对左侧整块垂直居中。
 *
 * 两段文案的分工是硬约定（判据：控件自己说没说得出这件事）：
 * - `hint` = **解释**「这东西干什么 / 单位与取值范围 / 为什么」→ 收进问号角标，悬浮才展开；
 * - `state` = **控件读不出的当前事实**（密钥存没存进凭据管理器 / 日志占多少盘 / 选中了哪个城市）
 *   → 常驻在标题下面。⚠️ 与控件面值重复的读数（输入框旁边的「当前 5 秒」）一律不写 —— 控件就是当前值。
 * - `stacked` = 复合控件变体：输入框 + 按钮 + 状态行这类「一块小表单」装不进一行，控件整块落到
 *   标题下面占满宽度。只给这种复合块和页内表单用，普通设置行不许用。
 */
import SettingsHint from './SettingsHint.vue'

interface Props {
  /** 可见字段标签。 */
  label?: string
  /** 标签自身的 id，适合由控件通过 aria-labelledby 引用。 */
  labelId?: string
  /** 原生 label 的 for 目标，适合 input 等可直接关联的控件。 */
  labelFor?: string
  /** 解释性说明：渲染成问号图标，悬浮才展开。 */
  hint?: string
  /** 状态读数：常驻显示在标题下面（只放控件读不出的事实）。 */
  state?: string
  /** 复合控件变体：控件落到标题下面占满宽度（默认横排贴右）。 */
  stacked?: boolean
}

withDefaults(defineProps<Props>(), {
  label: '',
  labelId: undefined,
  labelFor: undefined,
  hint: '',
  state: '',
  stacked: false,
})
</script>

<template>
  <div class="settings-field" :class="{ 'settings-field--stacked': stacked }">
    <!--
      左列：标题行 + 读数行。
      ⚠️ 角标不能塞进 `<label>` 里：那一下点击会顺带激活 `for` 指向的控件（开关被误动）。
    -->
    <div v-if="label || hint || state" class="settings-field__text">
      <div v-if="label || hint" class="settings-field__head">
        <label v-if="label && labelFor" class="settings-field__label" :for="labelFor">
          {{ label }}
        </label>
        <span v-else-if="label" :id="labelId" class="settings-field__label">
          {{ label }}
        </span>

        <SettingsHint :text="hint" />
      </div>

      <span v-if="state" class="settings-field__state">{{ state }}</span>
    </div>

    <!-- 右列：交互组件（stacked 变体里落到标题下面占满宽度） -->
    <div class="settings-field__control">
      <slot />
    </div>
  </div>
</template>

<style scoped>
/*
 * 默认横排：第一列吃掉剩余宽度（标题 + 角标 + 读数），第二列按控件自身宽度贴右。
 * 控件列刻意用 `auto` 而不是 `1fr` —— 数字输入被拉满整行反而更难读。
 */
.settings-field {
  display: grid;
  min-width: 0;
  grid-template-columns: minmax(0, 1fr) auto;
  column-gap: 12px;
  align-items: center;
}

/* 复合控件变体：单列，控件整块落到标题下面占满宽度。 */
.settings-field--stacked {
  grid-template-columns: minmax(0, 1fr);
  row-gap: 7px;
}

.settings-field__text {
  display: grid;
  min-width: 0;
  gap: 3px;
}

/* 标题行：文本在左、角标紧跟，两者**垂直居中**对齐（14px 标题下 baseline 会让角标视觉下坠）。 */
.settings-field__head {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 6px;
}

.settings-field__label {
  min-width: 0;
  color: var(--ufo-text);
  font-size: 14px;
  font-weight: 600;
  line-height: 1.3;
}

/* 读数在标题正下方：11.5px 灰字，长句自然折行、左对齐，不挤压控件列。 */
.settings-field__state {
  color: var(--ufo-muted);
  font-size: 11.5px;
  line-height: 1.45;
  min-width: 0;
  overflow-wrap: break-word;
}

.settings-field__control {
  justify-self: end;
  min-width: 0;
}

.settings-field--stacked .settings-field__control {
  justify-self: stretch;
}
</style>
