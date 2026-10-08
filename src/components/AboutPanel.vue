<script setup lang="ts">
/**
 * 版本卡：「悬浮窗设置」页左列最下面那张（跟在格子大小卡之后），**目前只有版本号**。
 *
 * 版本号取自 `package.json`（构建期由 vite 注入 `VITE_APP_VERSION`），
 * 所以这里不写死任何数字 —— 发版只改 package.json。
 */
import { appVersion } from '@/lib/app-info'

/** 当前应用版本号；注入链断了时是占位符「—」。 */
const version = appVersion()
</script>

<template>
  <section class="ufo-card" aria-labelledby="about-version-title">
    <div class="ufo-card__header">
      <h2 id="about-version-title" class="ufo-card__title">版本</h2>
      <!-- 等宽 + 等宽数字：版本号是数字串，等宽才不会在刷新时抖动。 -->
      <span class="about-panel__version">{{ version }}</span>
    </div>
  </section>
</template>
