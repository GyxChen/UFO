<script setup>
import { state } from './state.js'

/**
 * 插进「状态灯」信息卡尾部的那一块（清单 `surfaces.extension.into` 指定插进谁）。
 *
 * 提示窗给这一块的行数上限写在清单里（`maxRows`），超出的部分会被裁 ——
 * 所以这里只放两行，并且要短。来源标注（「插件 · 名字」）由宿主画，不用自己加。
 */
</script>

<template>
  <div class="tpl-ext">
    <p class="tpl-ext__line">最近一次转灯：{{ state.phase }}</p>
    <p class="tpl-ext__line tpl-ext__muted tpl-mono">{{ state.endpoint || '没有 endpoint 读数' }}</p>
  </div>
</template>
