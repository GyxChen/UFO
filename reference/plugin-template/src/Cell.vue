<script setup>
import { computed } from 'vue'
import { state } from './state.js'

/**
 * 悬浮窗格子里的那一块。
 *
 * ⚠️ 槽位尺寸由宿主决定：方形未贴边时一格可能只有十几像素高，横条 / 竖条下又完全不同。
 * 所以这里只用 `100%` 与容器查询单位（`cqw` / `cqh`），不写死像素。
 * 样式不在这个文件里 —— 见 `src/styles.css` 顶部那句「一个包只有一个 JS 产物」。
 */
const tone = computed(() => {
  if (!state.phase) return '#64748b'
  return state.phase === 'running' ? '#f59e0b' : '#22c55e'
})
const letter = computed(() => (state.phase ? state.phase.slice(0, 1).toUpperCase() : '·'))
</script>

<template>
  <div class="tpl-cell" :style="{ '--tpl-tone': tone }" :aria-label="state.phase || '还没等到事件'">
    <span class="tpl-cell__dot" aria-hidden="true">{{ letter }}</span>
  </div>
</template>
