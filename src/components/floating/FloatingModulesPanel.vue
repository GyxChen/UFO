<script setup lang="ts">
/**
 * 主窗「悬浮窗设置」页（方案 C）：左边是组件列表（启用开关 / 需求 / 顺序 / 齿轮）+ 格子大小卡，
 * 右边是悬浮窗在三种形态下的实时预览。
 *
 * 列表里的每一行都来自注册表（新增组件自动出现）；顺序写进配置的 `order`，
 * 与悬浮窗里的先后顺序是同一个字段。齿轮与「点悬浮窗上的组件」都打开同一个设置页。
 */
import { computed, ref, watch } from 'vue'
import { FLOATING_MODULE_DEFINITIONS } from '@/modules'
import FloatingShapePreview from './FloatingShapePreview.vue'
import { FLOATING_BASE_CELL_PX, MAX_CELL_PX, MIN_CELL_PX, clampCellPx } from '@/lib/floating-layout'
import {
  hasModuleSettings,
  orderedModuleDefinitions,
  reorderIds,
  type FloatingModuleDefinition,
} from '@/lib/floating-modules'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import BaseButton from '@/components/ui/BaseButton.vue'
import BaseSlider from '@/components/ui/BaseSlider.vue'
import BaseSwitch from '@/components/ui/BaseSwitch.vue'
import SettingsField from '@/components/ui/SettingsField.vue'
import gearIcon from '@/assets/gear.svg'

const emit = defineEmits<{
  /** 请求打开某个组件的设置页（由主窗口切到对应的 tab）。 */
  'open-settings': [moduleId: string]
}>()

const modules = useFloatingModulesStore()

/** 列表顺序 = 配置里的 order；含未启用的组件，否则没法再打开它们。 */
const definitions = computed(() => orderedModuleDefinitions(modules.config, FLOATING_MODULE_DEFINITIONS))
/** 当前形态下的运行时数据（各组件占几格、窗口共几格）；还没收到广播时为 null。 */
const currentRuntime = computed(() => {
  const snapshot = modules.runtime
  return snapshot ? snapshot.byLayout[snapshot.layout] : null
})
/** 已启用的组件数。 */
const enabledCount = computed(
  () => definitions.value.filter(definition => modules.moduleEnabled(definition.id, definition.defaultEnabled)).length
)
/** 组件列表的顺序 id，拖拽 / 键盘排序都改它。 */
const orderIds = computed(() => definitions.value.map(definition => definition.id))

/**
 * 格子大小输入值（物理像素）。
 *
 * 拖动时只改这个本地值（预览实时跟着变），松手才落盘 —— 否则滑块每动一像素
 * 都要写一次配置文件、还要让真窗口跟着重新吸附一次。
 */
const cellPxInput = ref(modules.cellPx)

// 配置从后端或别的窗口回来时同步滑块（首次加载完成、或取值被后端夹过）。
watch(
  () => modules.cellPx,
  value => {
    cellPxInput.value = value
  }
)

/** 松手 / 键盘调整后落盘；失败或取值没变时把滑块退回后端已知值。 */
async function commitCellPx() {
  const next = clampCellPx(cellPxInput.value)
  if (next === modules.cellPx) {
    cellPxInput.value = modules.cellPx
    return
  }
  try {
    await modules.updateCellPx(next)
  } catch {
    cellPxInput.value = modules.cellPx
  }
}

/** 恢复默认格边长（与 tauri.conf.json 的窗口初值同一口径）。 */
function resetCellPx() {
  cellPxInput.value = FLOATING_BASE_CELL_PX
  void commitCellPx()
}

/** 正在被拖拽的行 id（HTML5 拖拽）。 */
const draggingId = ref<string | null>(null)
/** 拖拽悬停到的行下标，用于画插入位置。 */
const dragOverIndex = ref<number | null>(null)

/** 某组件此刻占几格；未启用或没数据时显示占位。 */
function demandOf(definition: FloatingModuleDefinition): number | null {
  if (!modules.moduleEnabled(definition.id, definition.defaultEnabled)) return null
  return currentRuntime.value?.demands[definition.id] ?? null
}

/** 切换组件启用状态；失败时把开关回滚到后端已知状态。 */
async function toggleModule(definition: FloatingModuleDefinition, event: Event) {
  const input = event.target as HTMLInputElement
  try {
    await modules.updateModuleEnabled(definition.id, input.checked)
  } catch {
    input.checked = modules.moduleEnabled(definition.id, definition.defaultEnabled)
  }
}

/** 把 from 位置的组件移动到 to 位置并落盘。 */
async function moveModule(from: number, to: number) {
  const next = reorderIds(orderIds.value, from, to)
  if (next.join() === orderIds.value.join()) return
  try {
    await modules.updateModuleOrder(next)
  } catch {
    // 写入失败时配置未变，列表自然回到原顺序，无需回滚。
  }
}

/** 拖拽开始：记下被拖的行，拖拽期间只在同层内排序。 */
function onDragStart(definition: FloatingModuleDefinition, event: DragEvent) {
  draggingId.value = definition.id
  event.dataTransfer?.setData('text/plain', definition.id)
  if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move'
}

/** 拖到某一行上方：记录插入位置（阻止默认行为才允许放下）。 */
function onDragOver(index: number, event: DragEvent) {
  if (!draggingId.value) return
  event.preventDefault()
  dragOverIndex.value = index
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
}

/** 放下：把拖拽行移动到目标行位置。 */
async function onDrop(index: number) {
  const from = draggingId.value ? orderIds.value.indexOf(draggingId.value) : -1
  draggingId.value = null
  dragOverIndex.value = null
  if (from < 0) return
  await moveModule(from, index)
}

/** 拖拽取消（拖到列表外松手）。 */
function onDragEnd() {
  draggingId.value = null
  dragOverIndex.value = null
}

/** 键盘排序（拖拽的无障碍替代）：↑/↓ 把这一行上移 / 下移一位。 */
async function onHandleKeydown(index: number, event: KeyboardEvent) {
  const delta = event.key === 'ArrowUp' ? -1 : event.key === 'ArrowDown' ? 1 : 0
  if (!delta) return
  event.preventDefault()
  await moveModule(index, index + delta)
}
</script>

<template>
  <div class="module-catalog">
    <div class="module-catalog__column">
      <section class="ufo-card module-catalog__list" aria-labelledby="module-catalog-title">
        <div class="ufo-card__header">
          <h2 id="module-catalog-title" class="ufo-card__title">悬浮窗组件</h2>
          <span class="ufo-card__hint">{{ enabledCount }} / {{ definitions.length }} 已启用</span>
        </div>

        <ul class="module-catalog__rows">
          <li
            v-for="(definition, index) in definitions"
            :key="definition.id"
            class="module-row"
            :class="{
              'module-row--off': !modules.moduleEnabled(definition.id, definition.defaultEnabled),
              'module-row--dragging': draggingId === definition.id,
              'module-row--drop-target': dragOverIndex === index && draggingId !== definition.id,
            }"
            @dragover="onDragOver(index, $event)"
            @drop="onDrop(index)"
          >
            <span class="module-row__order" aria-hidden="true">
              {{ modules.moduleEnabled(definition.id, definition.defaultEnabled) ? index + 1 : '–' }}
            </span>

            <div class="module-row__copy">
              <span class="module-row__label">{{ definition.label }}</span>
              <span class="module-row__hint">{{ definition.description }}</span>
            </div>

            <span class="module-row__demand">
              {{ demandOf(definition) === null ? '—' : `${demandOf(definition)} 格` }}
            </span>

            <button
              v-if="hasModuleSettings(definition)"
              type="button"
              class="module-row__gear"
              :aria-label="`${definition.label}的设置`"
              :title="`${definition.label}的设置`"
              @click="emit('open-settings', definition.id)"
            >
              <img :src="gearIcon" alt="" aria-hidden="true" />
            </button>

            <BaseSwitch
              :model-value="modules.moduleEnabled(definition.id, definition.defaultEnabled)"
              :aria-label="`启用 ${definition.label}`"
              :title="`启用 ${definition.label}`"
              @change="toggleModule(definition, $event)"
            />

            <button
              type="button"
              class="module-row__handle"
              aria-label="拖拽排序（或用上下方向键）"
              title="拖拽排序（或用上下方向键）"
              draggable="true"
              @dragstart="onDragStart(definition, $event)"
              @dragend="onDragEnd"
              @keydown="onHandleKeydown(index, $event)"
            >
              ⋮⋮
            </button>
          </li>
        </ul>

        <p class="module-catalog__summary" role="status">
          已启用 {{ enabledCount }} 个组件 · 需求 {{ currentRuntime?.windowCells ?? '—' }} 格 · 窗口尺寸随贴边形态变化
        </p>
      </section>

      <!--
        格子大小自成一卡，紧贴组件列表下方：它是**整个悬浮窗**的尺寸基准（同级组件共享一格边长），
        不属于任何一个组件，也不只是预览卡的附属项。右卡只留预览。
      -->
      <section class="ufo-card module-catalog__cell-card" aria-labelledby="module-cell-size-title">
        <div class="ufo-card__header">
          <h2 id="module-cell-size-title" class="ufo-card__title">格子大小</h2>
          <span class="ufo-card__hint">所有组件共用</span>
        </div>

        <SettingsField
          hint="每一格的边长；横条 / 竖条按格数成倍放大，方形与四角保持正方形。拖动时预览实时变化，松手后悬浮窗按新尺寸重新吸附。"
        >
          <div class="module-catalog__cell-size">
            <BaseSlider
              v-model="cellPxInput"
              :min="MIN_CELL_PX"
              :max="MAX_CELL_PX"
              :step="1"
              aria-labelledby="module-cell-size-title"
              :aria-value-text="`${cellPxInput} 像素`"
              @change="commitCellPx"
            />
            <span class="module-catalog__cell-value">{{ cellPxInput }}px</span>
            <BaseButton
              variant="ghost"
              size="sm"
              :disabled="cellPxInput === FLOATING_BASE_CELL_PX"
              title="恢复默认格子大小"
              @click="resetCellPx"
            >
              默认
            </BaseButton>
          </div>
        </SettingsField>
      </section>
    </div>

    <section class="ufo-card module-catalog__preview" aria-labelledby="module-preview-title">
      <div class="ufo-card__header">
        <h2 id="module-preview-title" class="ufo-card__title">悬浮窗预览</h2>
        <span class="ufo-card__hint">贴边即变形</span>
      </div>

      <FloatingShapePreview
        :snapshot="modules.runtime"
        :definitions="definitions"
        :cell-px="cellPxInput"
      />
    </section>
  </div>
</template>