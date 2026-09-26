<script setup lang="ts">
/**
 * Agent 状态灯设置 · 会话记录：跨重启保留的会话存档（列表 / 单条删除 / 清除全部）。
 *
 * 面板本体是独立组件，这里只负责标题与说明。
 */
import SessionRecordsPanel from '../panels/SessionRecordsPanel.vue'
</script>

<template>
  <section class="module-page__section module-page__section--fill" aria-label="会话记录">
    <SessionRecordsPanel />
  </section>
</template>