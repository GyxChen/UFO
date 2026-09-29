<script setup lang="ts">
/**
 * 通用右键菜单：贴着光标弹出的**自绘浮层**（不是 Tauri 的系统原生菜单）。
 *
 * 为什么自绘：原生菜单是操作系统画的另一个窗口，观感与 Matte 面板割裂，且它接管输入 ——
 * 「点菜单外面会发生什么」由系统说了算。自绘之后这一层才管得住下面这三条：
 *
 * 1. **定位**：受控位置（调用方给光标的视口坐标），摆不下时夹回视口内（`placeContextMenu`）；
 * 2. **关得干净**：点菜单外面 / Esc / 点某一项 / 滚轮 / 改窗口大小都关，关完焦点还给原处；
 * 3. **不误触底下的 UI**：见 `onDocumentPointerDown` 那一段 —— 这是最容易做错的地方。
 *
 * 它是**受控组件**：`open` / `x` / `y` / `items` 全由调用方给，关闭只发 `update:open(false)`，
 * 自己不藏 —— 否则「菜单开着没有」会有两份状态，早晚对不上。
 */
import { computed, nextTick, onBeforeUnmount, ref, useId, watch } from 'vue'
import { placeContextMenu, type ContextMenuItem, type ContextMenuPoint } from '@/lib/context-menu'

interface Props {
  /** 是否展开（v-model:open）。 */
  open: boolean
  /** 打开位置：触发右键那一刻的光标视口坐标（`clientX`）。 */
  x: number
  /** 打开位置：触发右键那一刻的光标视口坐标（`clientY`）。 */
  y: number
  /** 菜单项，顺序即渲染顺序，也是键盘导航顺序。 */
  items: readonly ContextMenuItem[]
  /** 菜单的无障碍名称。 */
  ariaLabel?: string
}

const props = withDefaults(defineProps<Props>(), {
  ariaLabel: '上下文菜单',
})

const emit = defineEmits<{
  /** 点了某一项（置灰项不发）。⚠️ 先发 select 再发关闭，调用方才能在 handler 里读到点的是哪一条。 */
  select: [id: string]
  /** 展开状态变化（v-model:open）。 */
  'update:open': [open: boolean]
}>()

/** 菜单面板元素：量尺寸、判断「点在不在菜单里」都靠它。 */
const panelEl = ref<HTMLElement | null>(null)
/** 菜单左上角的视口坐标：先按光标给，量完实际尺寸再夹进视口。 */
const position = ref<ContextMenuPoint>({ x: 0, y: 0 })
/** 键盘高亮的项下标（-1 = 没有可点的项）。 */
const activeIndex = ref(-1)
/** 打开菜单前焦点所在的元素，关闭后还回去。 */
let restoreFocusEl: HTMLElement | null = null
/** 吞掉「关闭手势」后续事件的兜底定时器，见 `armSwallow`。 */
let swallowTimer: number | null = null

/** 每个实例生成稳定 id，同页多个菜单的 aria 引用才不会串线。 */
const menuId = `base-context-menu-${useId()}`

/** 可参与键盘导航的项下标（置灰项跳过）。 */
const enabledIndexes = computed(() => props.items.flatMap((item, index) => (item.disabled ? [] : [index])))

/** 项的 DOM id，供 aria-activedescendant 引用。 */
function itemId(index: number): string {
  return `${menuId}-item-${index}`
}

/** 目标元素是不是菜单自己（菜单挂在 body 下，只能靠包含关系判断）。 */
function isInsideMenu(target: EventTarget | null): boolean {
  return target instanceof Node && panelEl.value?.contains(target) === true
}

/** 键盘高亮回到第一个可点的项（没有可点项就是 -1）。 */
function resetActive(): void {
  activeIndex.value = enabledIndexes.value[0] ?? -1
}

/** 关菜单：只发事件，`open` 由调用方改（受控组件）。 */
function close(): void {
  emit('update:open', false)
}

/**
 * 量出菜单实际尺寸，把位置夹进视口。
 *
 * ⚠️ 用 `offsetWidth / offsetHeight` 而不是 `getBoundingClientRect()`：入场动画带 scale，
 * 后者量到的是「正在放大的过程」里的尺寸，菜单会被摆偏几个像素。
 */
async function measureAndPlace(): Promise<void> {
  await nextTick()
  const panel = panelEl.value
  if (!panel) return
  position.value = placeContextMenu(
    { x: props.x, y: props.y },
    { width: panel.offsetWidth, height: panel.offsetHeight },
    { width: window.innerWidth, height: window.innerHeight }
  )
}

/** 打开时的收尾：记住原焦点、听全局事件、量尺寸，并把焦点拿过来（键盘要能用）。 */
async function activate(): Promise<void> {
  restoreFocusEl =
    document.activeElement instanceof HTMLElement && document.activeElement !== document.body
      ? document.activeElement
      : null
  position.value = { x: props.x, y: props.y }
  resetActive()

  document.addEventListener('pointerdown', onDocumentPointerDown, true)
  document.addEventListener('contextmenu', onDocumentContextMenu, true)
  document.addEventListener('keydown', onDocumentKeydown, true)
  document.addEventListener('wheel', onDocumentWheel, { capture: true, passive: true })
  window.addEventListener('resize', close)

  await nextTick()
  panelEl.value?.focus()
  await measureAndPlace()
}

/** 关闭时的收尾：撤掉全局监听、把焦点还给打开前的元素。 */
function deactivate(): void {
  document.removeEventListener('pointerdown', onDocumentPointerDown, true)
  document.removeEventListener('contextmenu', onDocumentContextMenu, true)
  document.removeEventListener('keydown', onDocumentKeydown, true)
  document.removeEventListener('wheel', onDocumentWheel, true)
  window.removeEventListener('resize', close)
  disarmSwallow()
  activeIndex.value = -1
  // preventScroll：菜单是浮层，还焦点时不该把底下的列表滚一下
  restoreFocusEl?.focus({ preventScroll: true })
  restoreFocusEl = null
}

watch(
  () => props.open,
  value => {
    if (value) void activate()
    else deactivate()
  },
  { immediate: true }
)

// 开着的时候换了位置 / 换了项（右键了另一条目标）：重新量一次，高亮收回第一项
watch(
  () => [props.x, props.y, props.items] as const,
  () => {
    if (!props.open) return
    resetActive()
    void measureAndPlace()
  }
)

onBeforeUnmount(deactivate)

/**
 * 菜单外面按下的指针：**只关菜单，不做别的**。
 *
 * 关掉是显然的；难的是「不能顺手把底下的 UI 也点了」—— pointerdown 之后浏览器还会照常派发
 * pointerup 与 click，那时菜单已经撤了、露出来的是底下的按钮，那一下就会真的按下去
 * （表现为「点空白处关闭菜单」顺带触发了别的操作）。所以在**捕获阶段**就把这次手势吃掉：
 * `preventDefault` 挡掉默认行为（聚焦 / 起选），`stopPropagation` 让事件到不了目标元素，
 * 再补一个一次性的 click / pointerup 拦截（`armSwallow`）兜住后半程。
 */
function onDocumentPointerDown(event: PointerEvent): void {
  // 又按下了一次 = 上一次手势已经结束：先撤掉上一次的拦截，免得误伤这一次
  disarmSwallow()
  // 右键留给 contextmenu：那条路要让底下的行自己弹自己的菜单
  if (event.button === 2) return
  if (isInsideMenu(event.target)) return
  dismissGesture(event)
}

/** 关掉菜单，并把这一次手势整个吃掉（原因见 `onDocumentPointerDown`）。 */
function dismissGesture(event: Event): void {
  close()
  event.preventDefault()
  event.stopPropagation()
  armSwallow()
}

/**
 * 菜单外面再次右键：先关掉自己，**不拦**这一次事件 —— 底下的行照常弹它自己的菜单
 * （行上的 `@contextmenu.prevent`），于是「换一条目标」是一次右键而不是两下。
 */
function onDocumentContextMenu(event: MouseEvent): void {
  if (isInsideMenu(event.target)) return
  close()
}

/**
 * Esc 关菜单。
 * 挂在 document 而不是面板上：焦点万一被别的东西抢走（异步聚焦），面板就收不到按键了。
 */
function onDocumentKeydown(event: KeyboardEvent): void {
  if (event.key !== 'Escape') return
  event.preventDefault()
  event.stopPropagation()
  close()
}

/**
 * 滚轮：菜单贴着光标，底下一滚它就悬在原地对不上号了 —— 直接关掉。
 * 滚在菜单自己身上（项多到要滚）时不关：那是在滚它。
 * ⚠️ 不 `preventDefault`：滚动本身要照常发生，关菜单只是顺带。
 */
function onDocumentWheel(event: WheelEvent): void {
  if (isInsideMenu(event.target)) return
  close()
}

/** 关闭手势的后续事件最多拦这么久（正常由 click 或下一次按下收尾，这里只是兜底）。 */
const SWALLOW_MS = 1000

/**
 * 给「关闭菜单的那一次手势」补一次性的拦截：拦 click 与 pointerup。
 *
 * 为什么需要：捕获阶段已经 `stopPropagation` 了 pointerdown，但 click 是浏览器按
 * 「按下 + 抬起」自己合成的，不受它影响 —— 不拦的话这一次点击仍然会落到菜单底下
 * 刚露出来的按钮上。pointerup 同理（有些控件在 pointerup 上做动作）。
 */
function armSwallow(): void {
  disarmSwallow()
  document.addEventListener('click', swallowEvent, true)
  document.addEventListener('pointerup', swallowEvent, true)
  swallowTimer = window.setTimeout(disarmSwallow, SWALLOW_MS)
}

/** 吃掉一次后续事件；`click` 是这次手势的最后一站，拦到它就收工。 */
function swallowEvent(event: Event): void {
  /*
   * 键盘敲出来的 click（Enter / Space 触发按钮，`detail === 0`）不是这次指针手势的后半程：
   * 拦截可能还挂着（上一次指针手势没等到 click 就结束了，例如把指针拖出了窗口），
   * 放它过去并收工 —— 否则被吃掉的就不是「关闭菜单那一下」，而是用户后来的键盘操作。
   */
  if (event.type === 'click' && event instanceof MouseEvent && event.detail === 0) {
    disarmSwallow()
    return
  }
  event.preventDefault()
  event.stopPropagation()
  if (event.type === 'click') disarmSwallow()
}

/** 撤掉拦截（幂等）。 */
function disarmSwallow(): void {
  document.removeEventListener('click', swallowEvent, true)
  document.removeEventListener('pointerup', swallowEvent, true)
  if (swallowTimer !== null) {
    window.clearTimeout(swallowTimer)
    swallowTimer = null
  }
}

/** 在可点的项之间移动高亮（循环，置灰项跳过）。 */
function moveActive(step: 1 | -1): void {
  const indexes = enabledIndexes.value
  if (!indexes.length) return
  const current = indexes.indexOf(activeIndex.value)
  const next =
    current < 0 ? (step === 1 ? 0 : indexes.length - 1) : (current + step + indexes.length) % indexes.length
  activeIndex.value = indexes[next]
}

/**
 * 点（或键盘触发）某一项：先回传 id、再关菜单。
 *
 * ⚠️ 顺序不能反：调用方要在 `select` 里读到「点的是哪一条目标」，
 * 而关闭会把那份上下文清掉。
 */
function selectItem(index: number): void {
  const item = props.items[index]
  if (!item || item.disabled) return
  emit('select', item.id)
  close()
}

/** 触发当前高亮项。 */
function selectActive(): void {
  if (activeIndex.value >= 0) selectItem(activeIndex.value)
}

/** 鼠标划到某项：高亮跟过去，与键盘共用同一个 `activeIndex`。 */
function onItemPointerEnter(index: number): void {
  if (!props.items[index]?.disabled) activeIndex.value = index
}

/** 面板上的键盘交互：方向键 / Home / End 移动高亮，Enter / Space 触发。 */
function onPanelKeydown(event: KeyboardEvent): void {
  const indexes = enabledIndexes.value
  if (event.key === 'ArrowDown') moveActive(1)
  else if (event.key === 'ArrowUp') moveActive(-1)
  else if (event.key === 'Home') activeIndex.value = indexes[0] ?? -1
  else if (event.key === 'End') activeIndex.value = indexes[indexes.length - 1] ?? -1
  else if (event.key === 'Enter' || event.key === ' ') selectActive()
  else if (event.key === 'Tab') {
    // Tab 不拦：焦点照常往下一处走，菜单跟着关（原生菜单也是这个口径）
    close()
    return
  } else return
  event.preventDefault()
}
</script>

<template>
  <!-- 挂到 body：卡片有自己的 overflow 与层叠上下文，留在原地会被裁掉或被后面的卡片盖住 -->
  <Teleport to="body">
    <Transition name="base-context-menu">
      <div
        v-if="open"
        :id="menuId"
        ref="panelEl"
        class="base-context-menu"
        role="menu"
        :aria-label="ariaLabel"
        :aria-activedescendant="activeIndex >= 0 ? itemId(activeIndex) : undefined"
        tabindex="-1"
        :style="{ left: `${position.x}px`, top: `${position.y}px` }"
        @keydown="onPanelKeydown"
        @contextmenu.prevent
      >
        <button
          v-for="(item, index) in items"
          :id="itemId(index)"
          :key="item.id"
          type="button"
          role="menuitem"
          tabindex="-1"
          class="base-context-menu__item"
          :class="{
            'base-context-menu__item--active': index === activeIndex,
            'base-context-menu__item--danger': item.tone === 'danger',
          }"
          :disabled="item.disabled"
          @pointerenter="onItemPointerEnter(index)"
          @mousedown.prevent
          @click="selectItem(index)"
        >
          {{ item.label }}
        </button>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
/*
 * 面板：与 `BaseSelect` 的下拉同一套表面处理（surface-2 压暗 + 强边框 + 大投影），
 * 页面上的浮层因此看起来是同一层东西。
 * z-index 60：高于 BaseSelect 的 40（菜单该盖住已经打开的下拉）。
 */
.base-context-menu {
  position: fixed;
  z-index: 60;
  display: grid;
  box-sizing: border-box;
  min-width: 168px;
  max-width: min(280px, calc(100vw - 16px));
  /* 项多到一屏放不下时内部滚，配合 placeContextMenu 的夹取保证左上角始终可见 */
  max-height: calc(100vh - 16px);
  gap: 2px;
  overflow-y: auto;
  padding: 5px;
  border: 1px solid var(--ufo-border-strong);
  border-radius: 12px;
  outline: none;
  background: color-mix(in srgb, var(--ufo-surface-2) 96%, black 4%);
  box-shadow:
    0 18px 42px rgba(0, 0, 0, 0.42),
    0 4px 12px rgba(0, 0, 0, 0.24),
    inset 0 1px 0 rgba(255, 255, 255, 0.045);
  user-select: none;
  -webkit-user-select: none;
  scrollbar-gutter: stable;
  scrollbar-width: var(--ufo-scrollbar-w);
  transform-origin: top left;
}

/*
 * 菜单项：原生按钮重置成一行文字。34px 高、整行可点（比纯文字目标好按得多）。
 * 置灰用原生 `disabled`：读屏与键盘都据此跳过它，不必再自己写一套。
 */
.base-context-menu__item {
  display: flex;
  box-sizing: border-box;
  width: 100%;
  min-height: 34px;
  align-items: center;
  padding: 7px 10px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--ufo-text);
  font: inherit;
  font-size: 12.5px;
  text-align: left;
  white-space: nowrap;
  cursor: pointer;
  transition:
    background-color 120ms ease-out,
    color 120ms ease-out;
}

/* 悬浮与键盘高亮共用一个视觉态：鼠标和键盘因此看不出是两套实现 */
.base-context-menu__item:hover:not(:disabled),
.base-context-menu__item--active {
  background: var(--ufo-overlay-raised);
}

/* 焦点环画在内侧：菜单贴着光标弹出，外描边会被视口边缘切掉 */
.base-context-menu__item:focus-visible {
  outline: 2px solid var(--ufo-ring);
  outline-offset: -2px;
}

.base-context-menu__item:disabled {
  cursor: not-allowed;
  opacity: 0.42;
}

/*
 * 破坏性操作：红字 + 与上面的普通项拉开距离并压一条分隔线。
 * 分隔线用伪元素而不是额外 DOM —— 菜单项是纯数据渲染的，不该为了画条线让调用方多传字段。
 */
.base-context-menu__item--danger {
  position: relative;
  margin-top: 7px;
  color: var(--ufo-danger);
}

.base-context-menu__item--danger::before {
  content: '';
  position: absolute;
  top: -4px;
  right: 2px;
  left: 2px;
  height: 1px;
  background: var(--ufo-border);
}

.base-context-menu__item--danger:hover:not(:disabled),
.base-context-menu__item--danger.base-context-menu__item--active {
  background: color-mix(in srgb, var(--ufo-danger) 14%, transparent);
}

/* 从光标处展开：位移 + 淡入，不放大（菜单是「长出来的」，不是「弹出来的」） */
.base-context-menu-enter-active {
  transition:
    opacity 110ms ease-out,
    transform 110ms ease-out;
}

/* 退场比入场短：关闭要跟手 */
.base-context-menu-leave-active {
  pointer-events: none;
  transition:
    opacity 80ms ease-out,
    transform 80ms ease-out;
}

.base-context-menu-enter-from,
.base-context-menu-leave-to {
  opacity: 0;
  transform: scale(0.96);
}

@media (prefers-reduced-motion: reduce) {
  .base-context-menu-enter-active,
  .base-context-menu-leave-active {
    transition: none;
  }

  .base-context-menu__item {
    transition: none;
  }
}
</style>
