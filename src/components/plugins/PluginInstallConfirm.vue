<script setup lang="ts">
/**
 * 装前确认弹窗：把清单声明的每一项能力逐条摊给用户看，用户点头才写入安装记录。
 *
 * 三条规矩都在这一个文件里：
 * 1. **披露照原文渲染**（`disclosure` 来自 Rust 注册表），界面上不改写；
 * 2. 有 `danger` 档条目时，确认钮**默认不聚焦**且延迟 3 秒才可点 —— 这一档含
 *    「能读你发给 AI 的提问原文」，划过去比读进去容易得多；
 * 3. 出口只有 Esc 与「取消」两条，不弹原生框（`window.confirm` 被插件拦掉了）。
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import BaseButton from '@/components/ui/BaseButton.vue'
import PluginCapabilityIcon from './PluginCapabilityIcon.vue'
import { formatBytes } from '@/lib/plugin-format'
import type { CapabilityFactJson, PluginRecordJson } from '@/lib/plugin-types'

const props = defineProps<{
  /** `preview_plugin` 的返回：将要写入安装记录的那条。 */
  record: PluginRecordJson
  /** id → 能力事实（徽标语义与披露原文）。 */
  factOf: (id: string) => CapabilityFactJson | undefined
}>()

const emit = defineEmits<{
  /** 用户确认安装。 */
  confirm: []
  /** 用户取消（什么都不写）。 */
  cancel: []
}>()

/** 逐条披露的清单：挂载点在前、能力在后（与徽标同一顺序）。 */
const rows = computed(() => {
  const manifest = props.record.manifest
  if (!manifest) return []
  const surfaces = manifest.surfaces
  const ids = [
    ...(surfaces.cell != null ? ['cell'] : []),
    ...(surfaces.tooltip?.own ? ['tooltip-own'] : []),
    ...(surfaces.extension ? ['tooltip-extension'] : []),
    ...(surfaces.settings ? ['settings'] : []),
    ...manifest.permissions,
  ]
  return ids
    .map(id => props.factOf(id))
    .filter((fact): fact is CapabilityFactJson => fact != null)
})

/** 有 danger 档才需要延迟：普通本地能力不该让用户干等。 */
const hasDanger = computed(() => rows.value.some(row => row.tone === 'danger'))

/** 确认钮是否可点。 */
const armed = ref(!hasDanger.value)
let armTimer: ReturnType<typeof setTimeout> | undefined

onMounted(() => {
  if (armed.value) return
  armTimer = setTimeout(() => {
    armed.value = true
  }, 3000)
})

onBeforeUnmount(() => {
  if (armTimer !== undefined) clearTimeout(armTimer)
})

/** Esc = 取消（与「取消」按钮同一条路）。 */
function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') emit('cancel')
}
</script>

<template>
  <div class="plugin-confirm" tabindex="-1" @keydown="onKeydown">
    <div class="plugin-confirm__scrim" @click="emit('cancel')"></div>

    <section class="plugin-confirm__panel" role="dialog" aria-modal="true" aria-labelledby="plugin-confirm-title">
      <header class="plugin-confirm__head">
        <h3 id="plugin-confirm-title" class="plugin-confirm__title">安装 {{ record.name }}</h3>
        <p class="plugin-confirm__meta">
          id <code>{{ record.id }}</code> · v{{ record.version }} · {{ formatBytes(record.bytes) }} ·
          {{ record.files }} 个文件
        </p>
      </header>

      <p class="plugin-confirm__lead">
        组件不是扩展：装上它等于**你会主动运行的一段程序**。下面每一项是它能碰到的宿主能力，装完随时可以在这里停用或卸载。
      </p>

      <ul class="plugin-confirm__rows">
        <li v-for="row in rows" :key="row.id" class="plugin-confirm__row" :data-tone="row.tone">
          <span class="plugin-confirm__icon"><PluginCapabilityIcon :icon="row.icon" /></span>
          <span class="plugin-confirm__label">{{ row.label }}</span>
          <span class="plugin-confirm__text">{{ row.disclosure }}</span>
        </li>
      </ul>

      <!-- 出网白名单单独一行：它是「会连哪些域名」的直接答案，埋在披露里读不出来。 -->
      <p v-if="record.manifest?.hosts.length" class="plugin-confirm__hosts">
        只放行这些域名：{{ record.manifest.hosts.join(' · ') }}
      </p>

      <p v-if="record.warnings.length" class="plugin-confirm__warn" role="status">
        清单里有 {{ record.warnings.length }} 处被夹取或忽略：{{ record.warnings.join('；') }}
      </p>

      <footer class="plugin-confirm__actions">
        <BaseButton size="sm" autofocus @click="emit('cancel')">取消</BaseButton>
        <BaseButton
          variant="primary"
          size="sm"
          :disabled="!armed"
          :title="armed ? '确认安装' : '先读完上面这几条'"
          @click="emit('confirm')"
        >
          {{ armed ? '安装' : '请读完披露（3 秒）' }}
        </BaseButton>
      </footer>
    </section>
  </div>
</template>

<style scoped>
/* SFC 里的样式用普通 CSS：`@apply` 只在入口 CSS 或带 `@reference` 的独立文件里能用。 */
.plugin-confirm {
  position: fixed;
  inset: 0;
  z-index: 50;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
}

.plugin-confirm__scrim {
  position: absolute;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
}

.plugin-confirm__panel {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 12px;
  width: 100%;
  max-width: 560px;
  padding: 16px;
  background: var(--ufo-surface);
  border: 1px solid var(--ufo-border-strong);
  border-radius: var(--radius-ufo);
  box-shadow: 0 18px 48px rgba(0, 0, 0, 0.45);
}

.plugin-confirm__head {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.plugin-confirm__title {
  font-size: 13px;
  font-weight: 600;
}

.plugin-confirm__meta {
  font-size: 11px;
  color: var(--ufo-muted);
}

.plugin-confirm__lead {
  font-size: 11.5px;
  line-height: 1.6;
  color: var(--ufo-muted);
}

.plugin-confirm__rows {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.plugin-confirm__row {
  display: grid;
  grid-template-columns: 16px minmax(0, 140px) 1fr;
  align-items: start;
  gap: 8px;
  padding: 6px 8px;
  border-radius: 8px;
  background: var(--ufo-surface-2);
}

.plugin-confirm__row[data-tone='danger'] .plugin-confirm__label {
  color: var(--ufo-status-red-text);
}

.plugin-confirm__row[data-tone='caution'] .plugin-confirm__label {
  color: var(--ufo-status-yellow-text);
}

.plugin-confirm__icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  color: var(--ufo-muted);
}

.plugin-confirm__label {
  font-size: 12px;
  font-weight: 600;
}

.plugin-confirm__text {
  font-size: 11px;
  line-height: 1.6;
  color: var(--ufo-muted);
}

.plugin-confirm__hosts {
  font-size: 11px;
  line-height: 1.6;
  color: var(--ufo-muted);
}

.plugin-confirm__warn {
  font-size: 11px;
  line-height: 1.6;
  color: var(--ufo-status-yellow-text);
}

.plugin-confirm__actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}
</style>
