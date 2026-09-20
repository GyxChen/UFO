<script setup lang="ts">
import { computed } from 'vue'

/** 遮罩浓度：soft 盖在卡片上（还能透出下面的内容），solid 用于内容区（字多，需要压得更实）。 */
type LoadingTone = 'soft' | 'solid'

/** 加载文案的位置：'stack' 转圈在上、文字在下（默认），'inline' 两者并排（矮条状区域）。 */
type LoadingTextPlacement = 'stack' | 'inline'

interface Props {
  /** 是否正在加载；false 时整体不渲染，不留占位空壳。 */
  loading: boolean
  /** 加载文案；留空则只显示转圈。 */
  text?: string
  /** 遮罩浓度。 */
  tone?: LoadingTone
  /** 转圈与文字的排布方式。 */
  placement?: LoadingTextPlacement
}

const props = withDefaults(defineProps<Props>(), {
  text: '',
  tone: 'soft',
  placement: 'stack',
})

/** 汇总遮罩类，模板里只保留语义属性。 */
const overlayClass = computed(() => [
  `base-loading-overlay--${props.tone}`,
  `base-loading-overlay--${props.placement}`,
])
</script>

<template>
  <!--
    局部加载遮罩：定位在**最近的定位祖先**上（不是全屏）。

    用法：给要遮住的卡片 / 面板加 `relative`（或任意 position 非 static），把本组件放在它内部
    当最后一个子节点即可 —— 遮罩尺寸跟着容器走，圆角用 `border-radius: inherit` 跟着卡片，
    调用方不必再写一份尺寸与圆角。

    交互上**故意挡住点击**：网络请求在飞的时候重复点同一个按钮只会多打一次接口。
    无障碍走 `role="status"`（读屏会念文案），转圈本身标 `aria-hidden`。
  -->
  <Transition name="base-loading-fade">
    <div
      v-if="loading"
      class="base-loading-overlay"
      :class="overlayClass"
      role="status"
      aria-live="polite"
    >
      <span class="base-loading-overlay__spinner" aria-hidden="true" />
      <span v-if="text" class="base-loading-overlay__text">{{ text }}</span>
    </div>
  </Transition>
</template>

<style scoped>
.base-loading-overlay {
  position: absolute;
  z-index: 6;
  inset: 0;
  display: flex;
  box-sizing: border-box;
  align-items: center;
  justify-content: center;
  border-radius: inherit;
  cursor: progress;
}

/* 转圈在上、文字在下（默认）：卡片中央的经典形态 */
.base-loading-overlay--stack {
  flex-direction: column;
  gap: 9px;
}

/* 转圈与文字并排：矮条状区域（如一行工具栏）不会被撑高 */
.base-loading-overlay--inline {
  flex-direction: row;
  gap: 8px;
}

/* soft：卡片级遮罩 —— 透出一点底层内容，让用户知道「还是那块视图，只是正在更新」 */
.base-loading-overlay--soft {
  background: color-mix(in srgb, var(--ufo-surface) 72%, transparent);
  backdrop-filter: blur(2px);
}

/* solid：内容区遮罩 —— 底下是成片的文字，压得更实才不显脏 */
.base-loading-overlay--solid {
  background: color-mix(in srgb, var(--ufo-surface) 86%, transparent);
  backdrop-filter: blur(3px);
}

.base-loading-overlay__spinner {
  width: 16px;
  height: 16px;
  flex: 0 0 auto;
  border: 2px solid color-mix(in srgb, var(--ufo-accent) 26%, transparent);
  border-top-color: var(--ufo-accent);
  border-radius: 999px;
  animation: base-loading-spin 720ms linear infinite;
}

.base-loading-overlay__text {
  color: var(--ufo-muted);
  font-size: 12px;
  line-height: 1.3;
}

.base-loading-fade-enter-active,
.base-loading-fade-leave-active {
  transition: opacity 140ms ease-out;
}

.base-loading-fade-enter-from,
.base-loading-fade-leave-to {
  opacity: 0;
}

@keyframes base-loading-spin {
  to {
    transform: rotate(360deg);
  }
}

@media (prefers-reduced-motion: reduce) {
  .base-loading-fade-enter-active,
  .base-loading-fade-leave-active {
    transition: none;
  }

  .base-loading-overlay__spinner {
    animation-duration: 1.6s;
  }
}
</style>
