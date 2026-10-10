<script setup>
import { state } from './state.js'
import { useHost } from './host.js'

/** 自己的独立信息卡：悬停本包格子时弹出，尺寸由入口的 `tooltipSize` 决定。 */
const host = useHost()
</script>

<template>
  <div class="tpl-tip">
    <dl class="tpl-tip__rows">
      <div class="tpl-tip__row">
        <dt>组件</dt>
        <dd>{{ host.plugin.name }} {{ host.plugin.version }}</dd>
      </div>
      <div class="tpl-tip__row">
        <dt>灯态</dt>
        <dd>{{ state.phase || '还没等到事件' }}</dd>
      </div>
      <div class="tpl-tip__row">
        <dt>endpoint</dt>
        <dd class="tpl-mono">{{ state.endpoint || '—' }}</dd>
      </div>
      <div class="tpl-tip__row">
        <dt>我存的话</dt>
        <dd>{{ state.note || '（还没存过）' }}</dd>
      </div>
    </dl>
  </div>
</template>
