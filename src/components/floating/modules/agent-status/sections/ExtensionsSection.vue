<script setup lang="ts">
/**
 * Agent 状态灯设置 · 信息卡拓展：按 endpoint 配流程脚本，脚本返回值作为悬停信息卡的拓展字段。
 *
 * 面板本体（左右主从：endpoint 列表 + 脚本编辑器）是独立组件，这里只负责标题与说明。
 */
import EndpointExtensionPanel from '@/components/EndpointExtensionPanel.vue'
</script>

<template>
  <section class="module-page__section module-page__section--fill" aria-labelledby="agent-status-extension-title">
    <h2 id="agent-status-extension-title" class="module-page__section-title">信息卡拓展</h2>
    <p class="module-page__section-hint">
      按 endpoint 配流程脚本，脚本返回值会作为扩展字段显示在状态灯的悬停信息卡上。
    </p>
    <EndpointExtensionPanel />
  </section>
</template>