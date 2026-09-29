<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, useId, watch } from 'vue'

defineOptions({ inheritAttrs: false })

interface SelectOption {
  /** 选项值，对应 v-model。 */
  value: string
  /** 选项显示文案。 */
  label: string
  /** 选项说明（可选）：菜单里跟在名字下面，说清「选了这个会发生什么」。 */
  description?: string
  /** 是否禁用该选项。 */
  disabled?: boolean
}

interface Props {
  /** 当前选中值；配合 v-model 使用。 */
  modelValue?: string
  /** 下拉选项，渲染顺序与键盘导航顺序一致。 */
  options: readonly SelectOption[]
  /** 是否禁用整个下拉。 */
  disabled?: boolean
  /** combobox 的无障碍名称来源。 */
  ariaLabelledby?: string
  /** 没有可见标签时使用的无障碍名称。 */
  ariaLabel?: string
}

const props = withDefaults(defineProps<Props>(), {
  modelValue: '',
  disabled: false,
  ariaLabelledby: undefined,
  ariaLabel: undefined,
})

const emit = defineEmits<{
  /** 选中项变化。 */
  'update:modelValue': [value: string]
  /** 选中项变化后触发，便于页面立即提交。 */
  change: [value: string]
}>()

/** 组件根节点，用于判断点击是否发生在下拉之外。 */
const rootEl = ref<HTMLElement | null>(null)
/** 触发器，关闭菜单后把焦点还回来。 */
const triggerEl = ref<HTMLButtonElement | null>(null)
/** 下拉是否展开。 */
const open = ref(false)
/** 当前键盘高亮的选项下标。 */
const activeIndex = ref(-1)

/** 每个实例生成稳定 id，避免同页多个下拉的 aria 引用互相串线。 */
const selectId = useId()
const menuId = `base-select-menu-${selectId}`

/** 当前选中项。 */
const selectedOption = computed(() => props.options.find(option => option.value === props.modelValue))

/** 当前选中项下标；没有匹配项时为 -1。 */
const selectedIndex = computed(() => props.options.findIndex(option => option.value === props.modelValue))

/** 可参与键盘导航的选项下标。 */
const enabledIndexes = computed(() => props.options.flatMap((option, index) => (option.disabled ? [] : [index])))

/** 选项的 DOM id，供 aria-activedescendant 引用。 */
function optionId(index: number) {
  return `${menuId}-option-${index}`
}

/** 打开菜单，并把高亮项落到当前选中项或第一个可用项。 */
function openMenu(index = selectedIndex.value) {
  if (props.disabled || !props.options.length) return
  activeIndex.value = index >= 0 && !props.options[index]?.disabled ? index : (enabledIndexes.value[0] ?? -1)
  open.value = true
}

/** 关闭菜单；需要时把键盘焦点还给触发器。 */
function closeMenu(restoreFocus = false) {
  open.value = false
  if (restoreFocus) void nextTick(() => triggerEl.value?.focus())
}

/** 鼠标点击触发器时切换菜单。 */
function toggleMenu() {
  if (open.value) closeMenu()
  else openMenu()
}

/** 按方向键在可用选项之间移动，并用循环导航避免键盘陷阱。 */
function moveActive(direction: 1 | -1) {
  const indexes = enabledIndexes.value
  if (!indexes.length) return
  if (!open.value) {
    openMenu(direction === 1 ? indexes[0] : indexes[indexes.length - 1])
    return
  }
  const current = indexes.indexOf(activeIndex.value)
  const next =
    current < 0 ? (direction === 1 ? 0 : indexes.length - 1) : (current + direction + indexes.length) % indexes.length
  activeIndex.value = indexes[next]
}

/** 选中当前高亮项；值未变化时只关闭菜单。 */
function selectActive() {
  const option = props.options[activeIndex.value]
  if (!option || option.disabled) return
  if (option.value !== props.modelValue) {
    emit('update:modelValue', option.value)
    emit('change', option.value)
  }
  closeMenu(true)
}

/** 处理触发器的键盘交互，支持方向键、Home/End、Enter/Space 与 Escape。 */
function onTriggerKeydown(event: KeyboardEvent) {
  if (props.disabled) return
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault()
    moveActive(event.key === 'ArrowDown' ? 1 : -1)
    return
  }
  if (event.key === 'Home' || event.key === 'End') {
    if (!open.value) return
    event.preventDefault()
    activeIndex.value = event.key === 'Home' ? (enabledIndexes.value[0] ?? -1) : (enabledIndexes.value.at(-1) ?? -1)
    return
  }
  if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault()
    if (open.value) selectActive()
    else openMenu()
    return
  }
  if (event.key === 'Escape' && open.value) {
    event.preventDefault()
    closeMenu(true)
    return
  }
  if (event.key === 'Tab') closeMenu()
}

/** 鼠标悬浮时同步键盘高亮，让两种输入方式保持一致。 */
function onOptionPointerEnter(index: number) {
  if (!props.options[index]?.disabled) activeIndex.value = index
}

/** 点击选项后立即提交并关闭菜单。 */
function onOptionClick(index: number) {
  activeIndex.value = index
  selectActive()
}

/** 点击组件外部时关闭菜单，避免浮层残留在页面上。 */
function onDocumentPointerDown(event: PointerEvent) {
  const target = event.target
  if (target instanceof Node && rootEl.value?.contains(target)) return
  closeMenu()
}

watch(open, value => {
  if (value) document.addEventListener('pointerdown', onDocumentPointerDown, true)
  else document.removeEventListener('pointerdown', onDocumentPointerDown, true)
})

onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', onDocumentPointerDown, true)
})
</script>

<template>
  <div
    ref="rootEl"
    class="base-select"
    :class="{
      'base-select--open': open,
      'base-select--disabled': disabled,
    }"
  >
    <button
      ref="triggerEl"
      type="button"
      class="base-select__trigger"
      role="combobox"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :aria-controls="menuId"
      :aria-labelledby="ariaLabelledby"
      :aria-label="ariaLabel"
      :aria-activedescendant="open && activeIndex >= 0 ? optionId(activeIndex) : undefined"
      :disabled="disabled"
      v-bind="$attrs"
      @click="toggleMenu"
      @keydown="onTriggerKeydown"
    >
      <span class="base-select__value" :class="{ 'base-select__value--placeholder': !selectedOption }">
        {{ selectedOption?.label ?? '请选择' }}
      </span>
      <span class="base-select__chevron" aria-hidden="true">
        <svg viewBox="0 0 24 24">
          <path
            d="m6 9 6 6 6-6"
            fill="none"
            stroke="currentColor"
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
          />
        </svg>
      </span>
    </button>

    <Transition name="base-select-menu">
      <div
        v-if="open"
        :id="menuId"
        class="base-select__menu"
        role="listbox"
        :aria-labelledby="ariaLabelledby"
        :aria-label="ariaLabel"
      >
        <button
          v-for="(option, index) in options"
          :id="optionId(index)"
          :key="option.value"
          type="button"
          class="base-select__option"
          :class="{
            'base-select__option--active': index === activeIndex,
            'base-select__option--selected': option.value === modelValue,
            'base-select__option--disabled': option.disabled,
          }"
          role="option"
          tabindex="-1"
          :aria-selected="option.value === modelValue"
          :disabled="option.disabled"
          @pointerenter="onOptionPointerEnter(index)"
          @mousedown.prevent
          @click="onOptionClick(index)"
        >
          <span class="base-select__option-main">
            <span class="base-select__option-label">{{ option.label }}</span>
            <span v-if="option.description" class="base-select__option-desc">{{ option.description }}</span>
          </span>
          <svg class="base-select__check" viewBox="0 0 24 24" aria-hidden="true">
            <path
              d="m5 12 4.5 4.5L19 7"
              fill="none"
              stroke="currentColor"
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2.2"
            />
          </svg>
        </button>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.base-select {
  position: relative;
  width: 100%;
}

/*
 * 触发器（A 方案 · 紧凑下拉）：34px 高、单一描边、箭头不再压在灰色小方块上。
 * 现状的「丑」来自一个控件叠了三层框（黑底 + 边框 + 箭头底块），这里只留「底 + 边」两层。
 */
.base-select__trigger {
  display: flex;
  box-sizing: border-box;
  width: 100%;
  height: 34px;
  align-items: center;
  gap: 5px;
  padding: 0 7px 0 11px;
  border: 1px solid var(--ufo-border-strong);
  border-radius: 9px;
  outline: none;
  background: var(--ufo-input);
  color: var(--ufo-text);
  font: inherit;
  font-size: 12.5px;
  font-weight: 500;
  text-align: left;
  cursor: pointer;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.035),
    0 1px 2px rgba(0, 0, 0, 0.12);
  transition:
    background-color 150ms ease-out,
    border-color 150ms ease-out,
    box-shadow 150ms ease-out;
}

.base-select__trigger:hover:not(:disabled) {
  border-color: var(--ufo-border-strong);
  background: color-mix(in srgb, var(--ufo-input) 96%, white 4%);
}

.base-select__trigger:focus-visible,
.base-select--open .base-select__trigger {
  border-color: var(--ufo-accent);
  box-shadow:
    0 0 0 3px var(--ufo-accent-soft),
    inset 0 1px 0 rgba(255, 255, 255, 0.045);
}

.base-select__trigger:disabled {
  cursor: not-allowed;
  opacity: 0.48;
}

.base-select__value {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.base-select__value--placeholder {
  color: var(--ufo-muted);
}

/* 裸箭头：只有一支描边 chevron，hover / 展开时变强调色并翻转，不再有底块；盒子贴图标，不留空腔 */
.base-select__chevron {
  display: inline-flex;
  width: 13px;
  height: 13px;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  color: var(--ufo-muted);
  transition:
    color 150ms ease-out,
    transform 160ms ease-out;
}

.base-select__chevron svg {
  width: 13px;
  height: 13px;
}

.base-select__trigger:hover .base-select__chevron,
.base-select--open .base-select__chevron {
  color: var(--ufo-accent-text);
}

.base-select--open .base-select__chevron {
  transform: rotate(180deg);
}

/*
 * 菜单：带说明后比触发器宽，按内容展开并**右对齐**（R）——
 * 下拉常钉在设置行右缘，而 `.main-window` 是 overflow-x-hidden，往右长会被窗口裁掉。
 */
.base-select__menu {
  position: absolute;
  z-index: 40;
  top: calc(100% + 6px);
  right: 0;
  display: grid;
  box-sizing: border-box;
  width: max-content;
  min-width: 100%;
  max-width: 320px;
  max-height: 236px;
  gap: 2px;
  overflow-y: auto;
  padding: 5px;
  border: 1px solid var(--ufo-border-strong);
  border-radius: 12px;
  background: color-mix(in srgb, var(--ufo-surface-2) 96%, black 4%);
  box-shadow:
    0 18px 42px rgba(0, 0, 0, 0.42),
    0 4px 12px rgba(0, 0, 0, 0.24),
    inset 0 1px 0 rgba(255, 255, 255, 0.045);
  scrollbar-gutter: stable;
  scrollbar-width: var(--ufo-scrollbar-w);
}

.base-select__option {
  display: flex;
  box-sizing: border-box;
  width: 100%;
  min-height: 38px;
  align-items: center;
  gap: 9px;
  padding: 8px 9px 8px 10px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--ufo-text);
  font: inherit;
  font-size: 12.5px;
  text-align: left;
  cursor: pointer;
  transition:
    background-color 120ms ease-out,
    color 120ms ease-out;
}

.base-select__option:hover,
.base-select__option--active {
  background: var(--ufo-overlay-raised);
}

.base-select__option--selected {
  background: var(--ufo-accent-soft);
  color: var(--ufo-accent-text);
  font-weight: 600;
}

.base-select__option--selected:hover,
.base-select__option--selected.base-select__option--active {
  background: color-mix(in srgb, var(--ufo-accent-soft) 82%, white 4%);
}

.base-select__option--disabled {
  cursor: not-allowed;
  opacity: 0.42;
}

/* 名字 + 说明两列叠放；说明只有一行，读不出「选了这个会发生什么」才是这个菜单存在的意义 */
.base-select__option-main {
  display: grid;
  min-width: 0;
  flex: 1;
  gap: 1px;
}

.base-select__option-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.base-select__option-desc {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 10.5px;
  font-weight: 400;
  color: var(--ufo-muted);
}

.base-select__option--selected .base-select__option-desc {
  color: color-mix(in srgb, var(--ufo-accent-text) 72%, transparent);
}

.base-select__check {
  width: 14px;
  height: 14px;
  flex: 0 0 auto;
  opacity: 0;
  transform: scale(0.8);
  transition:
    opacity 120ms ease-out,
    transform 120ms ease-out;
}

.base-select__option--selected .base-select__check {
  opacity: 1;
  transform: scale(1);
}

.base-select-menu-enter-active,
.base-select-menu-leave-active {
  transition:
    opacity 140ms ease-out,
    transform 140ms ease-out;
}

.base-select-menu-enter-from,
.base-select-menu-leave-to {
  opacity: 0;
  transform: translateY(-5px) scale(0.985);
}

@media (prefers-reduced-motion: reduce) {
  .base-select__trigger,
  .base-select__chevron,
  .base-select__option,
  .base-select__check,
  .base-select-menu-enter-active,
  .base-select-menu-leave-active {
    transition: none;
  }
}
</style>
