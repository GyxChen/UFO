<script setup lang="ts">
/**
 * 独立面板窗的公共外壳：圆角实色面板 + 可拖标题栏，正文由调用方填。
 *
 * 为什么要有这一层：`script-editor` 与 `status-log` 都是 `decorations: false` 的窗口，
 * 系统不给边框、标题栏与关闭按钮，这三样必须自己画，而两个窗口画的是同一样东西。
 * 标题栏与主窗**同一版式**：左端 UFO 图标 + 应用名，右端那枚 × 关闭图标（同一个 `close.svg`、
 * 同一圈 hover / focus 态），中间才是这一扇窗自己的标题与那句常驻说明。
 *
 * ⚠️ 外壳不认识任何窗口自己的字段，也不管点了 × 之后要做什么（收起自己、回报发起方
 * 是调用方的事）—— 它只把动作转成 `close` 事件。
 */
import { startCurrentWindowDrag } from '@/lib/window'
import closeIcon from '@/assets/icon/close.svg'
import logoIcon from '@/assets/icon/ufo_logo.png'

const props = withDefaults(
  defineProps<{
    /** 标题栏里的本窗标题，带上下文（例如 `编辑脚本 · https://…`）。 */
    title: string
    /** 标题栏里那句常驻说明；空串 = 不占位。 */
    hint?: string
    /** × 的无障碍名称：它做的是「收起这扇窗」，图标本身说不清去向。 */
    closeLabel?: string
  }>(),
  { hint: '', closeLabel: '关闭窗口' }
)

const emit = defineEmits<{
  /** 点了 ×：调用方负责收尾（隐藏自己 + 回报发起方）。 */
  close: []
}>()

/** 标题栏拖窗：左键按下交给系统移动无边框窗口，按钮自身不参与。 */
async function onTitlebarPointerDown(event: PointerEvent) {
  if (event.button !== 0) return
  const target = event.target as HTMLElement | null
  if (target?.closest('button')) return
  try {
    await startCurrentWindowDrag()
  } catch (error) {
    console.error('failed to start dragging panel window', error)
  }
}
</script>

<template>
  <div class="panel-shell">
    <header class="panel-bar" @pointerdown="onTitlebarPointerDown">
      <span class="panel-bar__brand">
        <!-- 可见文字已写出「UFO」，图标只作装饰，alt 留空避免读屏重复播报。 -->
        <img :src="logoIcon" alt="" class="panel-bar__logo" aria-hidden="true" />
        <span class="panel-bar__name">UFO</span>
      </span>
      <h1 class="panel-bar__title">{{ props.title }}</h1>
      <!-- 提示行常驻（空串也要占住这条 flex 位），否则窄窗下关闭钮会挤到标题旁边。 -->
      <p class="panel-bar__hint">{{ props.hint }}</p>
      <button
        type="button"
        class="panel-bar__close"
        :aria-label="props.closeLabel"
        :title="props.closeLabel"
        @click.stop="emit('close')"
        @pointerdown.stop
      >
        <img :src="closeIcon" alt="" class="panel-bar__close-icon ufo-mono-icon" aria-hidden="true" />
      </button>
    </header>

    <div class="panel-body">
      <slot />
    </div>
  </div>
</template>
