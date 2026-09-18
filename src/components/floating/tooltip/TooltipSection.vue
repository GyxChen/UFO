<script setup lang="ts">
/**
 * 信息卡的分组容器：顶部一条强分隔线，把一组相关行（如某个模块的拓展字段）
 * 与上方基础信息、下方提问块切开。是否渲染由调用方 v-if 决定，容器本身不做判断。
 */
</script>

<template>
  <div class="tooltip-section">
    <slot />
  </div>
</template>