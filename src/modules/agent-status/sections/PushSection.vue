<script setup lang="ts">
/**
 * Agent 状态灯设置 · 消息推送：把灯态变化推到手机。
 *
 * 面板本体（上半区总开关 + 下半区双栏主从）是独立组件，这里只负责标题与说明。
 * 版面按 `reference/mock/push/push-a-dual-pane.html`（方案 A）实现，与「信息卡拓展」同构。
 */
import PushChannelPanel from '../panels/PushChannelPanel.vue'
</script>

<template>
  <section class="module-page__section module-page__section--fill" aria-label="消息推送">
    <PushChannelPanel />
  </section>
</template>
