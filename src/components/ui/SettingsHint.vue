<script setup lang="ts">
/**
 * 设置页的「额外说明」触发件：一个问号图标，光标悬浮（或键盘聚焦）才展开一段解释文字。
 *
 * 为什么要收进图标而不是常驻一行灰字：设置页每个字段都带一句解释时，版面被这些次要文案
 * 撑长，真正要调的控件反而被推远。约定是「解释收进问号、状态读数留在明处」。
 *
 * 面板**必须** `Teleport` 到 body：主窗从 `.main-shell` 到 `.main-window--wide` 多层
 * `overflow: hidden`，各设置面板又各自内滚，留在原地的绝对定位浮层一定被裁掉。
 * 定位复用右键菜单那套纯函数 `placeContextMenu`（贴着锚点摆、越界夹回视口），几何不重写。
 *
 * 面板**始终挂在 DOM 上、靠 class 收放**（不是 `v-if` / `v-show`）：
 * 1. `aria-describedby` 指向的节点即使不可见，accname 规范仍会把它算进「描述」—— 读屏用户
 *    聚焦到图标就能听到全文，不必真的用鼠标悬停；
 * 2. `visibility: hidden` 的元素照样参与布局与测量（`display: none` 量出来全是 0），
 *    所以位置能在展开**之前**一次算准，不必「先显示再量再挪」那种闪动写法。
 */
import { onBeforeUnmount, ref, useId } from 'vue'
import { placeContextMenu } from '@/lib/context-menu'

interface Props {
  /** 悬浮展开的说明正文；空串 = 整个图标不渲染（没话可说就别放图标）。 */
  text: string
}

defineProps<Props>()

/** 触发按钮：量它的位置、判断「点的是不是自己」。 */
const triggerEl = ref<HTMLButtonElement | null>(null)
/** 说明面板。 */
const panelEl = ref<HTMLElement | null>(null)
/** 是否展开。 */
const open = ref(false)
/** 面板左上角的视口坐标。 */
const position = ref({ x: 0, y: 0 })

/** 同页多个图标的 aria 引用互不串线。 */
const panelId = `settings-hint-${useId()}`

/** 悬停到点才展开：掠过整片表单时不该一路闪光标。 */
const OPEN_DELAY_MS = 120
/** 离开后留一点余量：指针可以从图标挪到面板上接着读。 */
const CLOSE_DELAY_MS = 120
/** 面板与触发按钮之间的缝隙。 */
const ANCHOR_GAP_PX = 6

let openTimer: number | null = null
let closeTimer: number | null = null

/** 撤掉待展开 / 待收起的定时器（幂等）。 */
function clearTimers(): void {
  if (openTimer !== null) {
    window.clearTimeout(openTimer)
    openTimer = null
  }
  if (closeTimer !== null) {
    window.clearTimeout(closeTimer)
    closeTimer = null
  }
}

/**
 * 贴着触发按钮展开：下面放得下就落在下面，放不下且上面更宽就翻到上面，最后再夹进视口。
 *
 * ⚠️ 只夹取不翻转的话，卡片底部的字段会把面板顶到视口下沿并**盖住图标自己**（实测过）；
 * 而设置页的字段常常贴到窗口底部，所以翻转不是可选项。夹取仍保留作最后一道防线
 * （极端窄窗时宁可压住一点，也不能跑出视口）。
 *
 * 量尺寸用 `offsetWidth / offsetHeight` 而不是 `getBoundingClientRect()`：面板淡入中后者会量到
 * 中间态；opacity 过渡不影响 offset 系列，拿到的始终是最终排版尺寸。
 */
function showNow(): void {
  clearTimers()
  const trigger = triggerEl.value
  const panel = panelEl.value
  if (!trigger || !panel) return
  const rect = trigger.getBoundingClientRect()
  const size = { width: panel.offsetWidth, height: panel.offsetHeight }
  const below = rect.bottom + ANCHOR_GAP_PX
  const above = rect.top - size.height - ANCHOR_GAP_PX
  const fitsBelow = below + size.height <= window.innerHeight
  position.value = placeContextMenu({ x: rect.left, y: !fitsBelow && above >= 0 ? above : below }, size, {
    width: window.innerWidth,
    height: window.innerHeight,
  })
  if (open.value) return
  open.value = true
  armDismiss()
}

/** 悬停入口：等到点才展开。 */
function scheduleOpen(): void {
  // 已经开着：只撤销「待收起」。不撤的话，指针离开后又回到图标上（120ms 内）会被那次收起关掉。
  if (open.value) {
    clearTimers()
    return
  }
  if (openTimer !== null) return
  openTimer = window.setTimeout(() => {
    openTimer = null
    showNow()
  }, OPEN_DELAY_MS)
}

/** 收起：撤监听，面板靠 class 回到不可见态（DOM 留着给读屏）。 */
function closeNow(): void {
  clearTimers()
  open.value = false
  disarmDismiss()
}

/** 离开入口：留一点余量，指针挪到面板上时取消收起。 */
function scheduleClose(): void {
  if (!open.value) return
  clearTimers()
  closeTimer = window.setTimeout(() => {
    closeTimer = null
    closeNow()
  }, CLOSE_DELAY_MS)
}

/** 面板与图标都挂在外面（body / 各层容器），只能靠包含关系判断「是不是自己人」。 */
function isInsideHint(target: EventTarget | null): boolean {
  if (!(target instanceof Node)) return false
  return triggerEl.value?.contains(target) === true || panelEl.value?.contains(target) === true
}

function onDocumentPointerDown(event: PointerEvent): void {
  if (isInsideHint(event.target)) return
  closeNow()
}

/** Esc 收起；焦点留在图标上（不该像关菜单那样把键盘流打断）。 */
function onDocumentKeydown(event: KeyboardEvent): void {
  if (event.key !== 'Escape') return
  closeNow()
}

/**
 * 滚动 / 改窗口大小就收起。
 * ⚠️ `scroll` 必须走捕获阶段：面板是 `position: fixed`，不跟内部滚动容器一起走，
 * 底下一滚它就悬在错误位置（`.push-detail` / `.ep-detail` 这些都在自己滚）。
 */
function armDismiss(): void {
  document.addEventListener('pointerdown', onDocumentPointerDown, true)
  document.addEventListener('keydown', onDocumentKeydown, true)
  document.addEventListener('scroll', closeNow, true)
  window.addEventListener('resize', closeNow)
}

function disarmDismiss(): void {
  document.removeEventListener('pointerdown', onDocumentPointerDown, true)
  document.removeEventListener('keydown', onDocumentKeydown, true)
  document.removeEventListener('scroll', closeNow, true)
  window.removeEventListener('resize', closeNow)
}

/** 键盘聚焦：立刻展开（键盘用户不该依赖 hover）。 */
function onFocus(): void {
  showNow()
}

/**
 * 点击切换：触屏与没有 hover 的输入设备照样能读到。
 * ⚠️ 同时 `prevent`：图标会放在 `<label>` 旁边甚至里面，不拦的话这一下会顺带激活关联控件、
 * 并把周围文字选中。
 */
function onTriggerClick(): void {
  if (open.value) closeNow()
  else showNow()
}

onBeforeUnmount(disarmDismiss)
</script>

<template>
  <button
    v-if="text"
    ref="triggerEl"
    type="button"
    class="settings-hint"
    :class="{ 'settings-hint--open': open }"
    aria-label="说明"
    :aria-describedby="panelId"
    :aria-expanded="open ? 'true' : 'false'"
    @pointerenter="scheduleOpen"
    @pointerleave="scheduleClose"
    @focus="onFocus"
    @blur="closeNow"
    @click.stop.prevent="onTriggerClick"
    @pointerdown.prevent
  >
    <!-- 问号角标：inline SVG 走 currentColor，浅色主题自动跟随，省掉一支要反相的白色资产 -->
    <svg viewBox="0 0 16 16" aria-hidden="true" focusable="false">
      <circle cx="8" cy="8" r="6.5" />
      <path d="M6.3 6.2a1.8 1.8 0 0 1 3.5.6c0 1.2-1.8 1.5-1.8 2.7" />
      <circle class="settings-hint__dot" cx="8" cy="12" r="0.95" />
    </svg>
  </button>

  <!-- 挂到 body：见文件头「必须 Teleport」那段。常驻 DOM 是为了 aria-describedby 与尺寸可测 -->
  <Teleport v-if="text" to="body">
    <div
      :id="panelId"
      ref="panelEl"
      class="settings-hint__pop"
      :class="{ 'settings-hint__pop--open': open }"
      role="tooltip"
      :aria-hidden="open ? 'false' : 'true'"
      :style="{ left: `${position.x}px`, top: `${position.y}px` }"
      @pointerenter="clearTimers"
      @pointerleave="scheduleClose"
    >
      {{ text }}
    </div>
  </Teleport>
</template>

<style scoped>
/*
 * 触发图标：15×15 的描边按钮，与前面的标题同一行、**垂直居中**对齐（间距交给容器的 `gap`，组件自己不留 margin）。
 * 15px 是配着 14px 标题定的：16px 的圆盒在 14px 字旁会显大，15px 则正好与字高齐平。
 * 描边靠 currentColor，悬停 / 聚焦 / 展开三态一起提亮 —— 看得出「正开着的是它」。
 */
.settings-hint {
  display: inline-flex;
  width: 15px;
  height: 15px;
  flex-shrink: 0;
  align-items: center;
  justify-content: center;
  margin: 0;
  padding: 0;
  border: 0;
  border-radius: 50%;
  background: transparent;
  color: color-mix(in srgb, var(--ufo-muted) 76%, transparent);
  cursor: help;
  transition:
    color 120ms ease-out,
    background-color 120ms ease-out;
}

.settings-hint:hover,
.settings-hint:focus-visible,
.settings-hint--open {
  color: var(--ufo-text);
}

.settings-hint:focus-visible {
  outline: 2px solid var(--ufo-ring);
  outline-offset: 1px;
}

.settings-hint svg {
  width: 14px;
  height: 14px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.25;
  stroke-linecap: round;
}

/* 问号下面那个点要实心：描边圆点在 14px 下会糊成一团 */
.settings-hint .settings-hint__dot {
  fill: currentColor;
  stroke: none;
}

/*
 * 面板：与 `BaseSelect` 下拉、`BaseContextMenu` 同一套表面处理（surface-2 压暗 + 强边框 +
 * 三层黑投影），页面上的浮层因此看着是同一层东西。z-index 70 高于右键菜单的 60。
 * 不加 backdrop-filter（主窗禁毛玻璃）、不新增 token（浅色档由既有变量自动跟随）。
 *
 * 收起态用 `visibility: hidden` 而不是 `display: none`：前者保留盒子（可读屏引用、可量尺寸），
 * 后者量到 0 且会让 aria-describedby 解析不出；`pointer-events: none` 保证看不见时不挡底下点击。
 * `visibility` 的过渡延迟 = 淡出结束后才真的隐藏。
 */
.settings-hint__pop {
  position: fixed;
  z-index: 70;
  box-sizing: border-box;
  max-width: min(320px, calc(100vw - 16px));
  padding: 8px 10px;
  border: 1px solid var(--ufo-border-strong);
  border-radius: 10px;
  background: color-mix(in srgb, var(--ufo-surface-2) 96%, black 4%);
  box-shadow:
    0 18px 42px rgba(0, 0, 0, 0.42),
    0 4px 12px rgba(0, 0, 0, 0.24),
    inset 0 1px 0 rgba(255, 255, 255, 0.045);
  color: var(--ufo-text);
  font-size: 11.5px;
  line-height: 1.5;
  text-align: left;
  white-space: normal;
  overflow-wrap: break-word;
  /* 正文常是路径、文件名，要能选中复制（右键菜单相反，那里禁选） */
  user-select: text;
  -webkit-user-select: text;
  opacity: 0;
  visibility: hidden;
  pointer-events: none;
  transition:
    opacity 110ms ease-out,
    visibility 0s linear 110ms;
}

.settings-hint__pop--open {
  opacity: 1;
  visibility: visible;
  pointer-events: auto;
  transition:
    opacity 110ms ease-out,
    visibility 0s linear 0s;
}

@media (prefers-reduced-motion: reduce) {
  .settings-hint,
  .settings-hint__pop,
  .settings-hint__pop--open {
    transition: none;
  }
}
</style>
