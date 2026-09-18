<script setup lang="ts">
/**
 * 信息卡的一行：左标签 + 右值。
 *
 * 提示窗的内容由各组件自己写，这个组件把「外壳那一行的样子」固化下来
 * （间距 / 省略号 / 等宽数值 / 语义色），其它组件不必再抄一遍 class。
 */
withDefaults(
  defineProps<{
    /** 左侧标签；不传就只渲染值（需要自定义排版时用默认插槽） */
    label?: string
    /** 右侧值（简单文本）；需要富内容时改用默认插槽 */
    value?: string | number
    /** 值的悬停提示（完整原文）；省略 = 不设 title */
    title?: string
    /** 值是否用等宽字体（数字 / 地址） */
    mono?: boolean
    /** 值的语义色；`default` = 主文本色 */
    tone?: 'default' | 'green' | 'yellow' | 'red' | 'muted'
  }>(),
  { label: '', value: '', title: undefined, mono: false, tone: 'default' }
)
</script>

<template>
  <div class="tooltip-row">
    <span v-if="label" class="tooltip-row__label">{{ label }}</span>
    <span
      class="tooltip-row__value"
      :class="[{ 'tooltip-row__value--mono': mono }, tone !== 'default' && `tooltip-row__value--${tone}`]"
      :title="title"
    >
      <slot>{{ value }}</slot>
    </span>
  </div>
</template>