<script setup lang="ts">
/**
 * Agent 状态灯设置 · 信息卡拓展：按 endpoint 配流程脚本，脚本返回值作为悬停信息卡的拓展字段。
 *
 * 面板本体（左右主从：endpoint 列表 + 脚本编辑器）是独立组件，这里只负责标题与说明。
 */
import EndpointExtensionPanel from '../panels/EndpointExtensionPanel.vue'
</script>

<template>
  <section class="module-page__section module-page__section--fill" aria-label="信息卡拓展">
    <EndpointExtensionPanel />
  </section>
</template>