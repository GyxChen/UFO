<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import BaseButton from '@/components/ui/BaseButton.vue'
import BaseInput from '@/components/ui/BaseInput.vue'
import BaseSelect from '@/components/ui/BaseSelect.vue'
import SettingsField from '@/components/ui/SettingsField.vue'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import {
  dueBucket,
  dueTone,
  formatClockTime,
  formatDue,
  priorityLabel,
} from '../lib/todo-format'
import {
  MAX_REFRESH_INTERVAL_MIN,
  MIN_REFRESH_INTERVAL_MIN,
  todoSettings,
  type TodoSettings,
} from '../settings'
import { useTodoStore, type TodoTaskItem } from '../stores/todo'

const modules = useFloatingModulesStore()
const todoStore = useTodoStore()

/** 当前落盘的模块配置（外部改动会通过 watch 回灌到本地输入）。 */
const currentSettings = computed(() => todoSettings(modules.config))

/** 「全部清单」选项值：空串 = 不限定清单（与 `TodoSettings.project_id` 同口径）。 */
const ALL_PROJECTS = ''

/** 预览列表最多渲染几条（多了就滚，卡片高度是固定的）。 */
const PREVIEW_LIMIT = 30

// ---- 本地输入状态 ----

/** 新 Token 输入框：不回显已存 Token（改 Token = 输入新值再保存） */
const tokenInput = ref('')
/** 正在保存 / 清除 Token */
const isSavingToken = ref(false)
/** Token 操作的结果文案（成功或失败） */
const tokenMessage = ref<string | null>(null)

/** 清单筛选（下拉绑定值） */
const projectIdInput = ref(currentSettings.value.project_id)
/** 正在拉清单列表 */
const isLoadingProjects = ref(false)
/** 清单列表拉取失败的原因 */
const projectsError = ref<string | null>(null)

/** 刷新周期输入框 */
const refreshIntervalInput = ref(currentSettings.value.refresh_interval_min)

/** 正在测试连接 / 刷新预览 */
const isTesting = ref(false)
/** 测试连接的结果文案 */
const testMessage = ref<string | null>(null)

onMounted(() => {
  void todoStore.checkTokenStatus().then(hasToken => {
    // 有 Token 才值得去问清单列表（没 Token 时后端只会回「未配置」）
    if (hasToken) void handleLoadProjects()
  })
})

// 配置在别处（命令回灌 / 广播）被改动时，把本地输入同步过来
watch(
  currentSettings,
  value => {
    projectIdInput.value = value.project_id
    refreshIntervalInput.value = value.refresh_interval_min
  },
  { immediate: true },
)

/**
 * 下拉选项：全部清单 + 后端返回的清单；若已选中项还没出现在列表里（首次加载前 / 清单被删），
 * 用配置里存的名字补一条，免得下拉显示成空。
 */
const projectOptions = computed(() => {
  const options = [{ value: ALL_PROJECTS, label: '全部清单' }]
  const seen = new Set<string>()
  for (const project of todoStore.projects) {
    seen.add(project.id)
    options.push({ value: project.id, label: project.name })
  }
  const selected = currentSettings.value.project_id
  if (selected && !seen.has(selected)) {
    options.push({ value: selected, label: currentSettings.value.project_name || selected })
  }
  return options
})

/**
 * 保存常规设置项（清单筛选 / 刷新周期）。
 *
 * ⚠️ 先落盘再让 store 重新取数：后端缓存按筛选口径校验，口径变了必然重新拉。
 *
 * @param patch 本次要改的字段
 */
async function saveCommonSettings(patch: Partial<TodoSettings> = {}): Promise<void> {
  const projectId = projectIdInput.value
  const projectName = todoStore.projects.find(project => project.id === projectId)?.name ?? ''
  await modules.updateModuleSettings('todo', {
    project_id: projectId,
    // 「全部清单」时把展示名一起清掉，免得信息卡里显示已失效的旧清单名
    project_name: projectId ? projectName || currentSettings.value.project_name : '',
    refresh_interval_min: Math.min(
      Math.max(
        Math.round(Number(refreshIntervalInput.value) || MIN_REFRESH_INTERVAL_MIN),
        MIN_REFRESH_INTERVAL_MIN,
      ),
      MAX_REFRESH_INTERVAL_MIN,
    ),
    ...patch,
  })
}

/**
 * 保存个人 API Token 到系统凭据管理器。
 *
 * 输入框里的明文只在这一刻交给后端，立刻清空；界面永不回显已存 Token。
 */
async function handleSaveToken(): Promise<void> {
  const token = tokenInput.value.trim()
  if (!token) return
  isSavingToken.value = true
  tokenMessage.value = null
  try {
    await todoStore.saveToken(token)
    tokenInput.value = ''
    tokenMessage.value = 'API Token 已保存到系统凭据管理器'
    await handleLoadProjects()
  } catch (err) {
    tokenMessage.value = typeof err === 'string' ? err : '保存 Token 失败'
  } finally {
    isSavingToken.value = false
  }
}

/** 清除系统凭据管理器里的 Token 并登出（本地快照一并清空）。 */
async function handleDeleteToken(): Promise<void> {
  isSavingToken.value = true
  tokenMessage.value = null
  try {
    await todoStore.deleteToken()
    tokenInput.value = ''
    tokenMessage.value = 'API Token 已从系统凭据管理器删除'
  } catch (err) {
    tokenMessage.value = typeof err === 'string' ? err : '删除 Token 失败'
  } finally {
    isSavingToken.value = false
  }
}

/** 拉取清单列表，填进「清单筛选」下拉。 */
async function handleLoadProjects(): Promise<void> {
  isLoadingProjects.value = true
  projectsError.value = null
  try {
    await todoStore.refreshProjects()
  } catch (err) {
    projectsError.value = typeof err === 'string' ? err : '拉取清单失败'
  } finally {
    isLoadingProjects.value = false
  }
}

/** 测试连接：强制绕过后端缓存拉一次，成功时把最新数据灌进预览。 */
async function handleTestConnection(): Promise<void> {
  isTesting.value = true
  testMessage.value = null
  try {
    const snapshot = await todoStore.testConnection()
    testMessage.value = `连接成功，共 ${snapshot.total_incomplete} 项未完成`
  } catch (err) {
    testMessage.value = typeof err === 'string' ? err : '测试连接失败'
  } finally {
    isTesting.value = false
  }
}

/** 手动刷新预览（同样绕过缓存）。 */
async function handleRefreshPreview(): Promise<void> {
  isTesting.value = true
  testMessage.value = null
  try {
    await todoStore.fetchSnapshot(true)
    if (!todoStore.snapshot) {
      testMessage.value = todoStore.lastError ?? '拉取失败'
    }
  } finally {
    isTesting.value = false
  }
}

/**
 * 预览行的截止语义色。
 *
 * @param task 任务
 * @returns `red` / `yellow` / `muted`
 */
function previewTone(task: TodoTaskItem): string {
  const data = todoStore.snapshot
  return dueTone(dueBucket(task.due_at_ms, data?.updated_at_ms ?? Date.now(), task.has_time))
}

/**
 * 预览行的截止文案（带时刻）。
 *
 * @param task 任务
 * @returns 中文相对日期；未设置截止时为空串
 */
function previewDue(task: TodoTaskItem): string {
  const data = todoStore.snapshot
  if (!data || task.due_at_ms == null) return ''
  return formatDue(task.due_at_ms, data.updated_at_ms, task.has_time)
}

/** 预览列表当前展示的任务（按上限截断）。 */
const previewTasks = computed(() => (todoStore.snapshot?.list ?? []).slice(0, PREVIEW_LIMIT))
</script>

<template>
  <div class="general-cards">
    <!-- 左卡：Token 授权 + 清单筛选 + 刷新周期 + 测试连接 -->
    <section class="ufo-card p-4 space-y-4">
      <header class="border-b border-[var(--ufo-border)] pb-2">
        <h3 class="text-sm font-semibold text-[var(--ufo-text)]">API 授权与服务</h3>
        <p class="text-xs text-[var(--ufo-muted)]">
          API Token 将安全存储在系统凭据管理器中，不进入配置文件
        </p>
      </header>

      <SettingsField
        label="API Token"
        label-for="todo-api-token"
        :hint="
          todoStore.hasSavedToken
            ? '当前状态：已保存至凭据管理器。输入新值点击保存可替换。'
            : '在滴答网页版「设置 → 账户 → API Token」生成后粘贴到这里。'
        "
      >
        <div class="space-y-2">
          <BaseInput
            id="todo-api-token"
            v-model="tokenInput"
            type="password"
            placeholder="输入个人 API Token..."
            @keydown.enter="handleSaveToken"
          />
          <div class="flex items-center gap-2">
            <BaseButton
              variant="primary"
              :disabled="isSavingToken || !tokenInput.trim()"
              @click="handleSaveToken"
            >
              保存 Token
            </BaseButton>
            <BaseButton
              v-if="todoStore.hasSavedToken"
              variant="danger"
              :disabled="isSavingToken"
              @click="handleDeleteToken"
            >
              清除已存 Token
            </BaseButton>
          </div>
          <p v-if="tokenMessage" class="text-xs text-[var(--ufo-accent)]" role="status">
            {{ tokenMessage }}
          </p>
        </div>
      </SettingsField>

      <SettingsField
        label="清单筛选"
        label-id="todo-project-label"
        hint="只统计某个清单的未完成任务；默认统计全部清单"
      >
        <div class="flex items-center gap-2">
          <BaseSelect
            v-model="projectIdInput"
            :options="projectOptions"
            aria-labelledby="todo-project-label"
            @update:model-value="saveCommonSettings()"
          />
          <BaseButton
            variant="ghost"
            :disabled="isLoadingProjects || !todoStore.hasSavedToken"
            @click="handleLoadProjects"
          >
            {{ isLoadingProjects ? '拉取中...' : '刷新清单' }}
          </BaseButton>
        </div>
        <p v-if="projectsError" class="text-xs text-[var(--ufo-danger)]" role="status">
          {{ projectsError }}
        </p>
      </SettingsField>

      <SettingsField
        label="自动刷新周期（分钟）"
        label-for="todo-refresh-interval"
        hint="数据拉取间隔，范围 5~60 分钟（避免过度请求滴答接口）"
      >
        <div class="flex items-center gap-2">
          <div class="w-24">
            <BaseInput
              id="todo-refresh-interval"
              v-model="refreshIntervalInput"
              type="number"
              :min="MIN_REFRESH_INTERVAL_MIN"
              :max="MAX_REFRESH_INTERVAL_MIN"
              @blur="saveCommonSettings()"
            />
          </div>
          <span class="text-xs text-[var(--ufo-muted)]">分钟</span>
        </div>
      </SettingsField>

      <div class="pt-2 border-t border-[var(--ufo-border)] flex flex-col gap-2">
        <BaseButton
          variant="ghost"
          :disabled="isTesting || !todoStore.hasSavedToken"
          @click="handleTestConnection"
        >
          {{ isTesting ? '正在测试连接...' : '测试连接' }}
        </BaseButton>
        <p v-if="testMessage" class="text-xs text-[var(--ufo-accent)]" role="status">
          {{ testMessage }}
        </p>
      </div>
    </section>

    <!-- 右卡：未完成任务预览 -->
    <section class="ufo-card p-4 flex flex-col todo-preview-card">
      <header class="border-b border-[var(--ufo-border)] pb-2 mb-2">
        <h3 class="text-sm font-semibold text-[var(--ufo-text)]">
          未完成预览
          <span v-if="todoStore.snapshot" class="ml-1 text-[var(--ufo-accent)]">
            {{ todoStore.snapshot.total_incomplete }} 项
          </span>
        </h3>
        <p class="todo-preview__scope">
          {{
            (currentSettings.project_name || '全部清单') +
            ' · 按「截止时间 → 优先级」排序'
          }}
        </p>
        <div class="mt-2">
          <BaseButton
            size="sm"
            variant="ghost"
            :disabled="isTesting || !todoStore.hasSavedToken"
            @click="handleRefreshPreview"
          >
            刷新
          </BaseButton>
        </div>
      </header>

      <div v-if="previewTasks.length > 0" class="todo-preview__list">
        <div v-for="task in previewTasks" :key="task.id" class="todo-preview__row">
          <span
            class="todo-preview__dot"
            :class="`todo-preview__dot--${previewTone(task)}`"
            aria-hidden="true"
          />
          <span class="todo-preview__title">{{ task.title }}</span>
          <span class="todo-preview__meta" :class="`todo-preview__meta--${previewTone(task)}`">
            <span v-if="priorityLabel(task.priority)" class="todo-preview__priority">
              {{ priorityLabel(task.priority) }}
            </span>
            <span v-if="previewDue(task)">{{ previewDue(task) }}</span>
          </span>
        </div>
      </div>

      <div v-else class="todo-preview__empty">
        <p class="text-sm text-[var(--ufo-muted)]">
          {{
            todoStore.hasSavedToken
              ? todoStore.snapshot
                ? '暂无未完成任务'
                : '尚未拉到数据，请点上方「刷新」'
              : '未配置 Token，请先在左侧保存个人 API Token'
          }}
        </p>
        <p v-if="todoStore.snapshot" class="text-[11px] text-[var(--ufo-muted)] opacity-70">
          更新于 {{ formatClockTime(todoStore.snapshot.updated_at_ms) }}
        </p>
      </div>
    </section>
  </div>
</template>
