<script setup lang="ts">
/**
 * 设置页的字段原语：一行标签（可带说明图标与状态读数）+ 控件本身（默认插槽）。
 *
 * 两个字段的分工是硬约定（判据：换个人来看这行该不该常驻）：
 * - `hint` = **解释**「这东西干什么 / 取值范围 / 为什么要这样填」→ 收进标签旁的问号图标，
 *   光标悬浮或键盘聚焦才展开（见 `SettingsHint.vue`）。
 * - `state` = **当前状态读数**（已保存 / 未保存 / 已选中哪个）→ 常驻显示，不许藏进 hover：
 *   用户扫一眼就该知道这个字段现在是什么情形，要点开才看到等于没写。
 *
 * 版面默认竖排（标签行在上、控件在下）。控件很短而说明很长时（一个数字输入 + 一长串读数）
 * 竖排会把控件甩到下一行的左边、右边空一片，这时给 `inline`：标题与读数留在左、控件贴右。
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
  /** 状态读数：常驻显示在标签行右侧，不走悬浮。 */
  state?: string
  /** 横排：标题与读数留在左、控件贴右（默认竖排）。 */
  inline?: boolean
}

withDefaults(defineProps<Props>(), {
  label: '',
  labelId: undefined,
  labelFor: undefined,
  hint: '',
  state: '',
  inline: false,
})
</script>

<template>
  <div class="settings-field" :class="{ 'settings-field--inline': inline }">
    <!--
      标签、图标、读数同一行（`.settings-field` 是 grid，每个子节点占一行）。
      ⚠️ 图标不能塞进 `<label>` 里：那一下点击会顺带激活 `for` 指向的控件（开关被误动）。
    -->
    <div v-if="label || hint || state" class="settings-field__head">
      <label v-if="label && labelFor" class="settings-field__label" :for="labelFor">
        {{ label }}
      </label>
      <span v-else-if="label" :id="labelId" class="settings-field__label">
        {{ label }}
      </span>

      <SettingsHint :text="hint" />

      <span v-if="state" class="settings-field__state">{{ state }}</span>
    </div>

    <slot />
  </div>
</template>

<style scoped>
.settings-field {
  display: grid;
  min-width: 0;
  gap: 7px;
}

/* 标签行：文本在左、图标紧跟、读数顶到右边；放不下就整体换行（读数常是短句）。 */
.settings-field__head {
  display: flex;
  min-width: 0;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 2px 8px;
}

/*
 * 横排：第一列吃掉剩余宽度（标签 + 角标 + 读数），第二列按控件自身宽度贴右。
 * 控件列刻意用 `auto` 而不是 `1fr` —— 数字输入被拉满整行反而更难读。
 */
.settings-field--inline {
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  column-gap: 12px;
}

.settings-field__label {
  min-width: 0;
  color: var(--ufo-muted);
  font-size: 12px;
  line-height: 1.35;
}

.settings-field__state {
  /* 读数靠右：与左边那组「标签 + 图标」拉开，视觉上不像标签的一部分 */
  margin-left: auto;
  color: color-mix(in srgb, var(--ufo-muted) 86%, transparent);
  font-size: 11px;
  line-height: 1.35;
  text-align: right;
}
</style>
