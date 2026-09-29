<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import BaseButton from '@/components/ui/BaseButton.vue'
import BaseContextMenu from '@/components/ui/BaseContextMenu.vue'
import BaseInput from '@/components/ui/BaseInput.vue'
import BaseLoadingOverlay from '@/components/ui/BaseLoadingOverlay.vue'
import BaseSegmentedControl from '@/components/ui/BaseSegmentedControl.vue'
import BaseSelect from '@/components/ui/BaseSelect.vue'
import SettingsField from '@/components/ui/SettingsField.vue'
import SettingsHint from '@/components/ui/SettingsHint.vue'
import type { ContextMenuItem } from '@/lib/context-menu'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import {
  PRIORITY_HIGH,
  PRIORITY_LOW,
  PRIORITY_MEDIUM,
  dueBucket,
  dueTone,
  formatClockTime,
  formatDue,
  formatMonthDay,
  isOverdue,
  localDayStartMs,
} from '../lib/todo-format'
import {
  buildTodoTree,
  cascadeTargets,
  flattenTree,
  leafNode,
  subtreeProgress,
  type TodoTreeRow,
} from '../lib/todo-tree'
import { MAX_REFRESH_INTERVAL_MIN, MIN_REFRESH_INTERVAL_MIN, todoSettings, type TodoSettings } from '../settings'
import { useTodoStore, type TodoTaskItem } from '../stores/todo'
import TodoTaskRow from './TodoTaskRow.vue'
import { TASK_MENU_DELETE, TASK_MENU_POSTPONE, buildTaskMenuItems } from './task-menu'

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

/** 「API 授权与服务」卡的遮罩文案：Token 与清单两类网络操作共用一个遮罩。 */
const authBusyText = computed(() => (isSavingToken.value ? '正在保存 Token…' : '正在拉取清单…'))

/** 刷新周期输入框 */
const refreshIntervalInput = ref(currentSettings.value.refresh_interval_min)

/** 正在测试连接 / 刷新预览 */
const isTesting = ref(false)
/** 测试连接的结果文案 */
const testMessage = ref<string | null>(null)

// ---- 新增任务 ----

/** 「今天」的零点毫秒：新建任务固定落在今天（全天），这里只用于界面展示。 */
const todayStart = computed(() => localDayStartMs())

/** 新任务标题输入 */
const newTaskTitle = ref('')
/** 新任务优先级（分段控件绑定字符串，提交时转成滴答的 5 / 3 / 1） */
const newTaskPriority = ref(String(PRIORITY_MEDIUM))
/** 新增成功的结果文案 */
const createMessage = ref<string | null>(null)
/** 新增失败的原因 */
const createError = ref<string | null>(null)

/** 优先级选项：没配 Token 时整组禁用（提交必然失败）。 */
const priorityOptions = computed(() =>
  [
    { value: String(PRIORITY_HIGH), label: '高' },
    { value: String(PRIORITY_MEDIUM), label: '中' },
    { value: String(PRIORITY_LOW), label: '低' },
  ].map(option => ({ ...option, disabled: !todoStore.hasSavedToken }))
)

/** 能不能提交：有 Token + 标题非空 + 没有正在提交的任务。 */
const canCreateTask = computed(
  () => todoStore.hasSavedToken && newTaskTitle.value.trim().length > 0 && !todoStore.isCreating
)

/**
 * 提交「新增任务」。
 *
 * 截止固定为**本机今天（全天）**：时区换算在 store 调命令时现算；成功后后端回最新快照，
 * 悬浮窗三面小旗与右侧预览会同时刷新。
 */
async function handleCreateTask(): Promise<void> {
  const title = newTaskTitle.value.trim()
  if (!title || !todoStore.hasSavedToken || todoStore.isCreating) return
  createMessage.value = null
  createError.value = null
  try {
    const snapshot = await todoStore.createTask({ title, priority: Number(newTaskPriority.value) })
    newTaskTitle.value = ''
    createMessage.value = `已加入「${currentSettings.value.project_name || '收件箱'}」，当前共 ${snapshot.total_incomplete} 项未完成`
  } catch (err) {
    createError.value = typeof err === 'string' ? err : '新建任务失败'
  }
}

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
  { immediate: true }
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
      Math.max(Math.round(Number(refreshIntervalInput.value) || MIN_REFRESH_INTERVAL_MIN), MIN_REFRESH_INTERVAL_MIN),
      MAX_REFRESH_INTERVAL_MIN
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

/** 「完成情况」里任务操作（完成 / 取消完成）的成功反馈。 */
const statusActionMessage = ref<string | null>(null)
/** 「完成情况」里任务操作的失败原因。 */
const statusActionError = ref<string | null>(null)
/** 正在提交任务操作：视图遮罩据此显示，并挡掉并发点击。 */
const statusActionPending = ref(false)

/** 某任务是否正在提交（完成 / 取消完成）：提交中把该行置灰，挡住重复点击。 */
function isTaskPending(taskId: string): boolean {
  return todoStore.pendingTaskIds.includes(taskId)
}

/**
 * 「完成情况」视图是否正在等网络：遮罩只盖这一块卡片，不遮整页
 * （页面别的卡片各管自己那摊请求，见各自的 `BaseLoadingOverlay`）。
 */
const isStatusBusy = computed(() => statusActionPending.value || isTesting.value || todoStore.isLoading)

/** 「完成情况」遮罩的文案：区分「在提交一条操作」与「在拉数据」。 */
const statusBusyText = computed(() => (statusActionPending.value ? '正在提交…' : '正在加载…'))

/**
 * 点一条任务的**勾选框**：未完成 → 完成，已完成 → 取消完成。
 *
 * ⚠️ 只有勾选框能触发（点标题不做任何事）：行里其它地方都点不动完成态，
 * 免得「想选个字 / 想看看标题」就把任务勾掉了。
 *
 * 完成时按 [`cascadeTargets`] 一路往上推：「最后一个子目标完成，父目标自动完成」，
 * 父目标完成又可能让它自己的父目标满足条件。整条链**一次**提交给后端
 * （`todoStore.completeTasks`），避免每层都重拉一遍列表。
 *
 * @param row 被勾的行（`row.ancestors` 是这条任务从根到父级的祖先链）
 */
async function handleToggleTask(row: TodoTreeRow): Promise<void> {
  const task = row.node.task
  if (statusActionPending.value || isTaskPending(task.id)) return
  statusActionMessage.value = null
  statusActionError.value = null
  statusActionPending.value = true
  try {
    if (task.completed_at_ms != null) {
      await todoStore.reopenTask(task)
      statusActionMessage.value = `已取消完成「${task.title}」`
      return
    }
    const cascade = cascadeTargets(row.ancestors, task.id)
    await todoStore.completeTasks([task, ...cascade])
    statusActionMessage.value = cascade.length
      ? `已完成「${task.title}」，并连带完成 ${cascade.length} 个父目标`
      : `已完成「${task.title}」`
  } catch (err) {
    statusActionError.value = typeof err === 'string' ? err : '操作失败'
  } finally {
    statusActionPending.value = false
  }
}

/**
 * 「完成情况」里点一条已完成任务的**勾选框**：取消完成，把它重新打开。
 *
 * 取消完成**不连带**：`reopenTask` 只把那一条置回未完成，父目标若因此「子目标没完成完」
 * 也无需回滚 —— 它本来就已经完成了，不动它更符合预期（用户只点了一条）。
 *
 * @param task 要取消完成的任务
 */
async function handleReopenTask(task: TodoTaskItem): Promise<void> {
  statusActionMessage.value = null
  statusActionError.value = null
  statusActionPending.value = true
  try {
    await todoStore.reopenTask(task)
    statusActionMessage.value = `已取消完成「${task.title}」`
  } catch (err) {
    statusActionError.value = typeof err === 'string' ? err : '取消完成失败'
  } finally {
    statusActionPending.value = false
  }
}

/**
 * 某条目标此刻算不算逾期。
 *
 * 口径与行的红色截止文字同源（都走 `isOverdue`）：全天任务当天不算逾期，
 * 要看「今天」就得按本机时区算 —— 所以参照时刻取快照的 `updated_at_ms`。
 */
function isTaskOverdue(task: TodoTaskItem): boolean {
  const data = todoStore.snapshot
  if (!data) return false
  return isOverdue(task.due_at_ms, data.updated_at_ms, task.has_time)
}

/** 逾期且未完成的目标：「全部顺延」的作用对象，也决定右键菜单里「顺延到今天」能不能点。 */
const overdueTasks = computed(() =>
  (todoStore.snapshot?.list ?? []).filter(task => isTaskOverdue(task))
)

/**
 * 把一批目标顺延到今天（右键菜单的单条与卡头「全部顺延」走同一条路，只是数组长度不同）。
 *
 * 只对**逾期**目标有意义，所以调用方传进来的都是逾期的；命令本身不管这个判断。
 *
 * @param tasks 要顺延的目标
 */
async function postponeTasks(tasks: readonly TodoTaskItem[]): Promise<void> {
  if (tasks.length === 0 || statusActionPending.value) return
  statusActionMessage.value = null
  statusActionError.value = null
  statusActionPending.value = true
  try {
    await todoStore.postponeTasks(tasks)
    statusActionMessage.value =
      tasks.length === 1
        ? `已把「${tasks[0].title}」顺延到今天`
        : `已把 ${tasks.length} 个逾期目标顺延到今天`
  } catch (err) {
    statusActionError.value = typeof err === 'string' ? err : '顺延失败'
  } finally {
    statusActionPending.value = false
  }
}

/**
 * 待确认删除的目标：右键选了「删除目标」先落在这里，等用户在确认条上点「删除」。
 *
 * 删除不可撤销，而右键菜单里塞不进二次确认 —— 所以拆成「选菜单 → 卡头确认条」两步。
 * ⚠️ 不用 `window.confirm`：项目装了 `tauri-plugin-prevent-default`，原生对话框会被拦下。
 */
const pendingDelete = ref<TodoTaskItem | null>(null)

/**
 * 这条目标此刻能不能顺延：只有**逾期且未完成**的才有意义
 * （已完成的目标没有「顺延」可言，不逾期的也不用顺延）。
 *
 * @param task 目标
 */
function canPostponeTask(task: TodoTaskItem): boolean {
  return task.completed_at_ms == null && isTaskOverdue(task)
}

/**
 * 右键菜单的目标与落点；null = 菜单没开。
 *
 * ⚠️ 位置与「作用在谁身上」一起存下来，而不是让菜单自己记：菜单是受控组件（`open` 由这里给），
 * 而且动作必须作用在**打开菜单时那一条**上 —— 菜单开着的时候列表可能刚刷新过（顺序会变），
 * 按「鼠标当前在哪一行」反查容易点错目标。
 */
const menuTarget = ref<{ task: TodoTaskItem; x: number; y: number } | null>(null)

/** 菜单项：只有「顺延」的可用性随目标状态变。 */
const menuItems = computed<ContextMenuItem[]>(() =>
  buildTaskMenuItems({ canPostpone: menuTarget.value ? canPostponeTask(menuTarget.value.task) : false })
)

/**
 * 右键一条目标：在光标处打开自绘菜单（顺延 / 删除）。
 *
 * ⚠️ 不走系统原生菜单（Tauri 的 menu API）：那是操作系统画的另一个窗口，观感与 Matte 面板割裂，
 * 而且它接管输入 —— 「点菜单外面只关菜单、不触发底下的 UI」这条做不到。
 *
 * @param task 被右键的目标
 * @param event 触发右键的鼠标事件（取光标的视口坐标当菜单落点）
 */
function handleRowContextMenu(task: TodoTaskItem, event: MouseEvent): void {
  // 正在提交别的操作时不开菜单：菜单里的动作这会儿也点不动
  if (statusActionPending.value) return
  menuTarget.value = { task, x: event.clientX, y: event.clientY }
}

/** 关掉右键菜单（点空白处 / Esc / 点完某一项都走这里）。 */
function closeTaskMenu(): void {
  menuTarget.value = null
}

/**
 * 菜单自报展开状态变化：只有「关」需要处理 —— 清掉目标，`open` 随之变回 false。
 *
 * @param open 菜单新的展开状态
 */
function handleMenuOpenChange(open: boolean): void {
  if (!open) closeTaskMenu()
}

/**
 * 点了菜单里的一项。
 *
 * ⚠️ 动作作用在**打开菜单时那一条目标**上，不是「鼠标此刻底下的目标」：菜单一开一关之间
 * 列表可能刚刷新过（顺序会变），按位置反查容易点错行。
 *
 * @param id 菜单项 id（见 `task-menu.ts` 的两个常量）
 */
function handleMenuSelect(id: string): void {
  const task = menuTarget.value?.task
  if (!task) return
  if (id === TASK_MENU_POSTPONE) {
    void postponeTasks([task])
    return
  }
  if (id === TASK_MENU_DELETE) {
    // 删除不可撤销，菜单里塞不下二次确认：先落到卡头的确认条（见 `pendingDelete`）
    pendingDelete.value = task
  }
}

/** 确认条里点「删除」：真正把目标删掉（不可撤销）。 */
async function confirmDelete(): Promise<void> {
  const task = pendingDelete.value
  if (!task || statusActionPending.value) return
  statusActionMessage.value = null
  statusActionError.value = null
  statusActionPending.value = true
  try {
    await todoStore.deleteTask(task)
    pendingDelete.value = null
    statusActionMessage.value = `已删除「${task.title}」`
  } catch (err) {
    statusActionError.value = typeof err === 'string' ? err : '删除失败'
  } finally {
    statusActionPending.value = false
  }
}

/** 确认条里点「取消」：什么都不做，只是退出待删除状态。 */
function cancelDelete(): void {
  pendingDelete.value = null
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

/** 已完成任务（近 7 天，后端已按完成时间倒序）。 */
const completedTasks = computed(() => todoStore.snapshot?.completed ?? [])
/** 已完成条数。 */
const totalCompleted = computed(() => completedTasks.value.length)

/**
 * 处于折叠态的任务 id（按任务记，刷新后仍保持）。
 *
 * 只装 id 不装节点：快照每轮都是新对象，存节点会立刻过期；存 id 则跨刷新有效。
 */
const collapsedTaskIds = ref<ReadonlySet<string>>(new Set())

/**
 * 展开 / 收起某条目标的子目标。
 *
 * @param taskId 目标 id
 */
function toggleCollapsed(taskId: string): void {
  const next = new Set(collapsedTaskIds.value)
  if (next.has(taskId)) next.delete(taskId)
  else next.add(taskId)
  collapsedTaskIds.value = next
}

/** 未完成组的树（子目标挂在父目标下，含已勾掉的子目标 —— 进度 `N/M` 靠它们）。 */
const incompleteTree = computed(() =>
  buildTodoTree(todoStore.snapshot?.list ?? [], completedTasks.value, PREVIEW_LIMIT)
)

/** 未完成组要渲染的行序列（折叠的分支整枝跳过，视图只用一个 `v-for`）。 */
const incompleteRows = computed(() => flattenTree(incompleteTree.value, collapsedTaskIds.value))

/** 已完成组要渲染的行（后端是扁平列表，包成没有子目标的叶子节点即可复用行组件）。 */
const completedRows = computed(() => completedTasks.value.map(leafNode))

/**
 * 未完成行的子目标进度文案（如 `1/3`）；没有子目标时返回空串。
 *
 * @param row 行
 * @returns 进度文案；无子目标为空串
 */
function subtaskLabel(row: TodoTreeRow): string {
  const progress = subtreeProgress(row.node)
  if (progress.total === 0) return ''
  return `子目标 ${progress.done}/${progress.total}`
}

/**
 * 已完成任务的完成时间文案：`今天 HH:MM` / `昨天 HH:MM` / `MM-DD HH:MM`（本地时区）。
 *
 * @param task 已完成任务
 * @param nowMs 参考时刻（用快照的 `updated_at_ms`，保证同一次渲染口径一致）
 * @returns 可直接展示的完成时间
 */
function completedTime(task: TodoTaskItem, nowMs: number): string {
  const ms = task.completed_at_ms
  if (ms == null) return ''
  const dayOf = (t: number) => {
    const date = new Date(t)
    date.setHours(0, 0, 0, 0)
    return date.getTime()
  }
  const pad = (n: number) => String(n).padStart(2, '0')
  const date = new Date(ms)
  const time = `${pad(date.getHours())}:${pad(date.getMinutes())}`
  const dayDiff = Math.round((dayOf(ms) - dayOf(nowMs)) / 86_400_000)
  if (dayDiff === 0) return `今天 ${time}`
  if (dayDiff === -1) return `昨天 ${time}`
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${time}`
}
</script>

<template>
  <div class="general-cards todo-general">
    <!-- 新增任务卡：横跨两列（表单一行放得下，挤进半列会换行得很难看） -->
    <section class="ufo-card relative p-4 todo-create-card">
      <BaseLoadingOverlay :loading="todoStore.isCreating" text="正在添加任务…" />
      <header class="border-b border-[var(--ufo-border)] pb-2">
        <h3 class="text-sm font-semibold text-[var(--ufo-text)]">新增任务</h3>
        <p class="text-xs text-[var(--ufo-muted)]">默认记为今天的任务（全天）· 截止 {{ formatMonthDay(todayStart) }}</p>
      </header>

      <div class="flex flex-wrap items-end gap-3">
        <div class="min-w-[220px] flex-1">
          <!-- 页内表单（不是设置行）：控件在标题下面占满，走 stacked -->
          <SettingsField stacked label="任务标题" label-for="todo-new-task-title">
            <BaseInput
              id="todo-new-task-title"
              v-model="newTaskTitle"
              placeholder="要做什么…（回车即可添加）"
              :disabled="!todoStore.hasSavedToken"
              @keydown.enter="handleCreateTask"
            />
          </SettingsField>
        </div>

        <div class="w-[200px]">
          <SettingsField stacked label="优先级" label-id="todo-new-task-priority-label">
            <BaseSegmentedControl
              v-model="newTaskPriority"
              name="todo-new-task-priority"
              :options="priorityOptions"
              aria-labelledby="todo-new-task-priority-label"
            />
          </SettingsField>
        </div>

        <BaseButton
          variant="primary"
          :disabled="!canCreateTask"
          :loading="todoStore.isCreating"
          @click="handleCreateTask"
        >
          添加
        </BaseButton>
      </div>

      <p v-if="!todoStore.hasSavedToken" class="text-xs text-[var(--ufo-muted)]">
        先在下面保存 API Token，才能新增任务
      </p>
      <p v-if="createMessage" class="text-xs text-[var(--ufo-accent)]" role="status">{{ createMessage }}</p>
      <p v-if="createError" class="text-xs text-[var(--ufo-danger)]" role="status">{{ createError }}</p>
    </section>

    <!-- 左卡：Token 授权 + 清单筛选 + 刷新周期 + 测试连接（列宽固定，见 todo.css） -->
    <section class="ufo-card relative p-4 todo-auth-card">
      <BaseLoadingOverlay
        :loading="isSavingToken || isLoadingProjects"
        :text="authBusyText"
        tone="solid"
      />

      <!--
        卡内滚动区：左卡与右卡等高（高度由 grid 行给定），内容比它高时滚这里 ——
        否则最下面的「测试连接」会被挤出卡片外面。
        ⚠️ 滚动放在这一层、不放卡片自己身上：卡片里的加载遮罩是 `position: absolute; inset: 0`，
        卡片一旦成了滚动容器，遮罩就会跟着内容一起滚走。
      -->
      <div class="todo-auth-card__body">
        <header class="ufo-heading-row border-b border-[var(--ufo-border)] pb-2">
          <h3 class="text-sm font-semibold text-[var(--ufo-text)]">API 授权与服务</h3>
          <SettingsHint text="API Token 将安全存储在系统凭据管理器中，不进入配置文件" />
        </header>

        <!-- Token 管理是「输入 + 按钮 + 状态行」的复合块，装不进一行，走 stacked -->
        <SettingsField
          stacked
          label="API Token"
          label-for="todo-api-token"
          :state="todoStore.hasSavedToken ? '已保存至凭据管理器' : '未保存'"
          :hint="
            todoStore.hasSavedToken
              ? '输入新值后点「保存 Token」即可替换。'
              : '在滴答网页版「设置 → 账户 → API Token」生成后粘贴到这里。'
          "
        >
          <!--
            用 flex + gap 而不是 `space-y-*`：space-y 编译出来是 `:where(...)`（**优先级为 0**），
            会被组件自己的 `margin: 0` 静默盖掉 —— 输入框与按钮之间就一点空隙都不剩了。
          -->
          <div class="flex flex-col gap-2">
            <BaseInput
              id="todo-api-token"
              v-model="tokenInput"
              type="password"
              placeholder="输入个人 API Token..."
              @keydown.enter="handleSaveToken"
            />
            <div class="flex items-center gap-2">
              <BaseButton variant="primary" :disabled="isSavingToken || !tokenInput.trim()" @click="handleSaveToken">
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

        <!-- 清单筛选：下拉 + 刷新按钮 + 错误行，复合块走 stacked -->
        <SettingsField stacked label="清单筛选" label-id="todo-project-label" hint="只统计某个清单的未完成任务；默认统计全部清单">
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
        </SettingsField>

        <div class="pt-2 border-t border-[var(--ufo-border)] flex flex-col gap-2">
          <BaseButton variant="ghost" :disabled="isTesting || !todoStore.hasSavedToken" @click="handleTestConnection">
            {{ isTesting ? '正在测试连接...' : '测试连接' }}
          </BaseButton>
          <p v-if="testMessage" class="text-xs text-[var(--ufo-accent)]" role="status">
            {{ testMessage }}
          </p>
        </div>
      </div>
    </section>

    <!-- 右卡：完成情况（未完成在上、近 7 天完成在下；目标有子目标时折成可折叠的树） -->
    <section class="ufo-card relative flex flex-col p-4 todo-status-card">
      <!--
        加载遮罩只盖这块「完成情况」视图（不是全屏）：刷新、提交完成都在这里等结果，
        用户看得见「哪一块在忙」，页面别的卡片照常可用。
      -->
      <BaseLoadingOverlay :loading="isStatusBusy" :text="statusBusyText" tone="solid" />

      <header class="border-b border-[var(--ufo-border)] pb-2 mb-2 shrink-0">
        <h3 class="text-sm font-semibold text-[var(--ufo-text)]">
          完成情况
          <span v-if="todoStore.snapshot" class="ml-1 text-[var(--ufo-accent)]">
            未完成 {{ todoStore.snapshot.total_incomplete }} · 已完成 {{ totalCompleted }}
          </span>
        </h3>
        <p class="todo-status__scope">
          {{ (currentSettings.project_name || '全部清单') + ' · 点勾选框即完成 · 右键顺延 / 删除 · 最后一个子目标完成时父目标自动完成' }}
        </p>
        <div class="mt-2 flex flex-wrap items-center gap-2">
          <BaseButton
            size="sm"
            variant="ghost"
            :disabled="isTesting || !todoStore.hasSavedToken"
            @click="handleRefreshPreview"
          >
            刷新
          </BaseButton>
          <BaseButton
            size="sm"
            variant="ghost"
            :disabled="statusActionPending || overdueTasks.length === 0"
            :aria-label="`全部顺延：把 ${overdueTasks.length} 个逾期目标挪到今天`"
            @click="postponeTasks(overdueTasks)"
          >
            全部顺延
            <span v-if="overdueTasks.length" class="todo-status__btn-count">{{ overdueTasks.length }}</span>
          </BaseButton>
          <span v-if="statusActionMessage" class="todo-status__action-msg" role="status">{{ statusActionMessage }}</span>
          <span v-if="statusActionError" class="todo-status__action-msg todo-status__action-msg--error" role="status">{{ statusActionError }}</span>
        </div>

        <!--
          删除的二次确认：右键菜单里塞不下确认步骤，所以先选菜单、再在这里确认。
          删除不可撤销，文案里带上标题与「无法恢复」。
        -->
        <div v-if="pendingDelete" class="todo-status__confirm" role="status">
          <span class="todo-status__confirm-text">删除「{{ pendingDelete.title }}」？删除后无法恢复。</span>
          <BaseButton size="sm" variant="danger" :disabled="statusActionPending" @click="confirmDelete">
            删除
          </BaseButton>
          <BaseButton size="sm" variant="ghost" :disabled="statusActionPending" @click="cancelDelete">
            取消
          </BaseButton>
        </div>
      </header>

      <div v-if="!todoStore.hasSavedToken" class="todo-status__empty todo-status__empty--center">
        未配置 Token，请先在左侧保存个人 API Token
      </div>

      <div v-else-if="!todoStore.snapshot" class="todo-status__empty todo-status__empty--center">
        尚未拉到数据，请点上方「刷新」
      </div>

      <div v-else class="todo-status">
        <!-- 未完成组：目标 → 子目标（可折叠）。优先级在勾选框边框上，点勾选框即完成 -->
        <section class="todo-status__group">
          <h4 class="todo-status__group-title">
            未完成
            <span>{{ todoStore.snapshot.total_incomplete }}</span>
          </h4>

          <ul v-if="incompleteRows.length" class="todo-status__list">
            <TodoTaskRow
              v-for="row in incompleteRows"
              :key="row.node.task.id"
              :node="row.node"
              :depth="row.depth"
              :collapsed="collapsedTaskIds.has(row.node.task.id)"
              :pending="isTaskPending(row.node.task.id)"
              :meta-tone="previewTone(row.node.task)"
              @toggle="handleToggleTask(row)"
              @toggle-collapse="toggleCollapsed(row.node.task.id)"
              @contextmenu.prevent="handleRowContextMenu(row.node.task, $event)"
            >
              <template #meta>
                <span v-if="subtaskLabel(row)">{{ subtaskLabel(row) }}</span>
                <span v-if="previewDue(row.node.task)">{{ previewDue(row.node.task) }}</span>
              </template>
            </TodoTaskRow>
          </ul>
          <p v-else class="todo-status__empty">暂无未完成任务</p>
        </section>

        <!-- 已完成组：扁平列表（后端不挂子目标树），点勾选框即取消完成 -->
        <section class="todo-status__group">
          <h4 class="todo-status__group-title">
            已完成（近 7 天）
            <span>{{ totalCompleted }}</span>
          </h4>

          <ul v-if="completedRows.length" class="todo-status__list">
            <TodoTaskRow
              v-for="node in completedRows"
              :key="node.task.id"
              :node="node"
              :show-priority="false"
              :pending="isTaskPending(node.task.id)"
              @toggle="handleReopenTask(node.task)"
              @contextmenu.prevent="handleRowContextMenu(node.task, $event)"
            >
              <template #meta>
                {{ completedTime(node.task, todoStore.snapshot.updated_at_ms) }}
              </template>
            </TodoTaskRow>
          </ul>
          <p v-else class="todo-status__empty">近 7 天没有已完成任务</p>
        </section>

        <p class="todo-status__updated">更新于 {{ formatClockTime(todoStore.snapshot.updated_at_ms) }}</p>
      </div>
    </section>

    <!--
      目标的右键菜单：自绘浮层，自己 Teleport 到 body，所以放在这里不会影响上面的 grid 布局。
      「点空白处只关菜单、不误触底下的 UI」由组件自己保证（见 `BaseContextMenu` 的说明）。
    -->
    <BaseContextMenu
      :open="menuTarget !== null"
      :x="menuTarget?.x ?? 0"
      :y="menuTarget?.y ?? 0"
      :items="menuItems"
      aria-label="目标操作"
      @select="handleMenuSelect"
      @update:open="handleMenuOpenChange"
    />
  </div>
</template>
