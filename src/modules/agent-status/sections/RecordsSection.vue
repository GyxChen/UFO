<script setup lang="ts">
/**
 * Agent 状态灯设置 · 会话记录：跨重启保留的会话存档（列表 / 单条删除 / 清除全部）。
 *
 * 面板本体是独立组件，这里只负责标题与说明。
 */
import SessionRecordsPanel from '../panels/SessionRecordsPanel.vue'
</script>

<template>
  <section class="module-page__section module-page__section--fill" aria-labelledby="agent-status-records-title">
    <h2 id="agent-status-records-title" class="module-page__section-title">会话记录</h2>
    <p class="module-page__section-hint">
      跨重启保留的会话存档：重启或 <code>--resume</code> 后灯位、token、提问都能续上。
    </p>
    <SessionRecordsPanel />
  </section>
</template>