import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { ref, watch, type WatchStopHandle } from 'vue'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import { isModuleEnabled } from '@/lib/floating-modules'
import { localDayStartMs, localUtcOffsetMinutes } from '../lib/todo-format'
import { todoSettings } from '../settings'

/**
 * 滴答清单里的一条任务（未完成或已完成，视 `completed_at_ms` 而定）。
 * 字段与 Rust 侧 `models.rs` 的 `TodoTaskItem` 一一对应。
 */
export interface TodoTaskItem {
  /** 任务 id */
  id: string
  /** 任务标题 */
  title: string
  /** 所属清单 id */
  project_id: string
  /** 所属清单名（取自 `GET /open/v1/project`，查不到时为「清单」） */
  project_name: string
  /** 优先级：5 高 / 3 中 / 1 低 / 0 无（排序已在后端按截止时间 + 优先级完成） */
  priority: number
  /** 截止毫秒时间戳（UTC epoch）；未设置截止为 null */
  due_at_ms: number | null
  /** 是否有具体时刻（false = 只到天粒度，界面不显示时分） */
  has_time: boolean
  /** 子任务（勾选清单）总条数 */
  items_total: number
  /** 子任务已完成条数 */
  items_done: number
  /** 任务完成毫秒时间戳（UTC epoch）；未完成任务为 null */
  completed_at_ms: number | null
  /** 父任务 id（滴答父子子任务的 parentId）；顶层任务为 null */
  parent_id: string | null
  /**
   * 子任务树（嵌套）。后端发的是**扁平列表**（三档计数 / 悬浮徽标不受影响），
   * 这一个字段由面板按 `parent_id` 组装出来，用于平铺展示子目标结构。
   */
  subtasks: TodoTaskItem[]
}

/**
 * 待办快照：未完成列表 + 近 7 天完成列表。由 Rust `get_todo_snapshot` 返回并广播 `todo-snapshot`。
 */
export interface TodoSnapshot {
  /** 未完成任务，按「有截止的在前（截止早的在前）→ 优先级高 → 标题」排好 */
  list: TodoTaskItem[]
  /** 未完成任务总数（= list 长度；滴答单次最多返回 200 条） */
  total_incomplete: number
  /** 近 7 天完成的任务，按完成时间倒序、最多 30 条 */
  completed: TodoTaskItem[]
  /** 快照构建时刻（毫秒），相对日期一律以它为参照 */
  updated_at_ms: number
}

/**
 * 新建任务的入参。
 *
 * 截止时间**不在**这里：新建一律落在「本机今天」（全天），由 store 调命令时现算，
 * 免得调用方（设置页 / 将来的其它入口）各算一套时区口径。
 */
export interface TodoCreateInput {
  /** 任务标题（调用前已 trim，不应为空） */
  title: string
  /** 优先级：5 高 / 3 中 / 1 低（其它值由后端夹取） */
  priority: number
}

/**
 * 滴答清单里的一个清单（项目），用于设置页的「清单筛选」下拉。
 */
export interface TodoProject {
  /** 清单 id（收件箱固定为 `inbox`） */
  id: string
  /** 清单名 */
  name: string
}

/**
 * 待办模块的 Pinia 状态。
 *
 * 数据链路与天气一致：常驻的 `floating` 窗口是唯一的轮询 owner，它的命令调用
 * （`get_todo_snapshot`）会顺带把快照广播给所有窗口；主窗 / 提示窗只监听广播，
 * 缺数据时补齐一次，不跑自己的定时器（见模块开发规范第 9.1 节）。
 */
export const useTodoStore = defineStore('todo', () => {
  const snapshot = ref<TodoSnapshot | null>(null)
  const isLoading = ref(false)
  const lastError = ref<string | null>(null)
  const hasSavedToken = ref(false)
  const projects = ref<TodoProject[]>([])
  /** 正在提交「完成」的任务 id：信息卡据此把该行标成提交中并挡住重复点击 */
  const pendingTaskIds = ref<string[]>([])
  /** 正在提交「新建任务」表单 */
  const isCreating = ref(false)

  let timerId: number | null = null
  let stopListener: UnlistenFn | undefined
  let stopConfigWatch: WatchStopHandle | undefined

  const windowLabel = getCurrentWindow().label
  // 只允许常驻的悬浮窗当轮询 owner（主窗 / 提示窗随时可能隐藏）。
  const isOwnerWindow = windowLabel === 'floating'

  /**
   * 接收来自本窗口命令或其它窗口广播的最新快照。
   *
   * @param data 待办快照
   */
  function applySnapshot(data: TodoSnapshot): void {
    snapshot.value = data
    lastError.value = null
  }

  /**
   * 拉取最新待办快照。
   *
   * @param forceRefresh 是否强制绕过后端缓存真打一次滴答接口
   * @returns 成功时返回快照，失败（未配 Token / 网络 / 鉴权）时返回 null 并记录 `lastError`
   */
  async function fetchSnapshot(forceRefresh = false): Promise<TodoSnapshot | null> {
    const modulesStore = useFloatingModulesStore()
    if (!todoSettings(modulesStore.config).has_token) {
      lastError.value = '尚未配置滴答清单 API Token'
      return null
    }

    isLoading.value = true
    try {
      const data = await invoke<TodoSnapshot>('get_todo_snapshot', { forceRefresh })
      applySnapshot(data)
      return data
    } catch (err) {
      lastError.value = typeof err === 'string' ? err : String(err)
      return null
    } finally {
      isLoading.value = false
    }
  }

  /**
   * 重启定时轮询（只有 owner 窗口会调）。下限 5 分钟，避免过度请求滴答接口。
   *
   * @param intervalMin 刷新周期（分钟）
   */
  function restartPolling(intervalMin: number): void {
    stopPolling()
    const intervalMs = Math.max(intervalMin, 5) * 60 * 1000
    timerId = window.setInterval(() => {
      void fetchSnapshot(false)
    }, intervalMs)
  }

  /** 停止定时轮询（幂等）。 */
  function stopPolling(): void {
    if (timerId !== null) {
      window.clearInterval(timerId)
      timerId = null
    }
  }

  /**
   * 查询系统凭据管理器里是否已有 Token（只回布尔，不回显明文）。
   *
   * @returns 有 Token 时为 true；读取失败按「没有」处理
   */
  async function checkTokenStatus(): Promise<boolean> {
    try {
      const status = await invoke<boolean>('get_todo_secret_status')
      hasSavedToken.value = status
      return status
    } catch {
      hasSavedToken.value = false
      return false
    }
  }

  /**
   * 保存个人 API Token 到系统凭据管理器，并立刻按新 Token 拉一次数据（仅主窗口调用）。
   *
   * @param token 滴答网页版「设置 → 账户 → API Token」生成的令牌；空串等于删除
   */
  async function saveToken(token: string): Promise<void> {
    const trimmed = token.trim()
    if (!trimmed) {
      await deleteToken()
      return
    }
    await invoke('set_todo_token', { token: trimmed })
    hasSavedToken.value = true
    const modulesStore = useFloatingModulesStore()
    await modulesStore.updateModuleSettings('todo', { has_token: true })
    void fetchSnapshot(true)
  }

  /**
   * 清除已保存的 Token 并丢掉本地快照（仅主窗口调用）。
   */
  async function deleteToken(): Promise<void> {
    await invoke('delete_todo_token')
    hasSavedToken.value = false
    snapshot.value = null
    projects.value = []
    const modulesStore = useFloatingModulesStore()
    await modulesStore.updateModuleSettings('todo', { has_token: false })
  }

  /**
   * 拉取清单列表，供设置页的「清单筛选」下拉使用（仅主窗口调用）。
   *
   * @returns 清单数组；失败时抛出错误交给调用方展示
   */
  async function refreshProjects(): Promise<TodoProject[]> {
    const data = await invoke<TodoProject[]>('list_todo_projects')
    projects.value = data
    return data
  }

  /**
   * 完成一条或多条任务（信息卡点任务行 / 设置页点勾选框触发）。
   *
   * 多条是给「最后一个子目标完成，父目标自动完成」用的（`lib/todo-tree.ts` 的
   * `cascadeTargets` 算出祖先链）：一枪把整条链发过去，后端在**同一次**重拉里完成并广播，
   * 而不是一条一次往返 —— 后者每完成一条父目标都要重扫一遍列表，树越深越慢。
   *
   * 后端完成 + 重新拉一次快照后把新快照返回（并广播给所有窗口），所以这里直接灌进 store
   * 就够 —— 不需要前端自己从列表里抠掉那几条，三档计数也不会算错。
   * 提交期间把 id 记进 `pendingTaskIds`，重复点击会被挡下。
   *
   * @param tasks 要完成的任务（顺序即提交顺序，父目标应排在它的子目标之后）
   * @throws 后端返回的错误文案（网络 / 鉴权 / 任务已被删除）
   */
  async function completeTasks(tasks: readonly TodoTaskItem[]): Promise<void> {
    const fresh = tasks.filter(task => !pendingTaskIds.value.includes(task.id))
    if (fresh.length === 0) return
    const ids = fresh.map(task => task.id)
    pendingTaskIds.value = [...pendingTaskIds.value, ...ids]
    try {
      // Tauri v2 会把 camelCase 参数名映射到 Rust 侧的 snake_case。
      const data = await invoke<TodoSnapshot>('complete_todo_task', {
        targets: fresh.map(task => ({ projectId: task.project_id, taskId: task.id })),
      })
      applySnapshot(data)
    } finally {
      pendingTaskIds.value = pendingTaskIds.value.filter(id => !ids.includes(id))
    }
  }

  /**
   * 完成单条任务（悬停信息卡里点任务行触发）。
   *
   * @param task 要完成的任务
   */
  async function completeTask(task: TodoTaskItem): Promise<void> {
    await completeTasks([task])
  }

  /**
   * 取消完成（重新打开）一条任务（设置页「完成情况」里点已完成任务触发）。
   *
   * 与 [`completeTask`] 对称：后端把 status 置回未完成、重新拉一次快照后把新快照返回（并广播
   * 给所有窗口），这里直接灌进 store 就够。任务会从「已完成」组回到「未完成」组，三档计数
   * 与悬浮窗徽标一并更新。`pendingTaskIds` 同样用于挡掉重复点击。
   *
   * @param task 要取消完成的任务
   * @throws 后端返回的错误文案（网络 / 鉴权 / 任务已被删除）
   */
  async function reopenTask(task: TodoTaskItem): Promise<void> {
    if (pendingTaskIds.value.includes(task.id)) return
    pendingTaskIds.value = [...pendingTaskIds.value, task.id]
    try {
      const data = await invoke<TodoSnapshot>('reopen_todo_task', {
        projectId: task.project_id,
        taskId: task.id,
      })
      applySnapshot(data)
    } finally {
      pendingTaskIds.value = pendingTaskIds.value.filter(id => id !== task.id)
    }
  }

  /**
   * 把若干条任务顺延到今天（设置页右键菜单「顺延到今天」/ 卡头「全部顺延」）。
   *
   * 与「完成」同一套配合方式：一批一起发（后端一次改完、一次重拉），避免几十条逾期目标
   * 各打一轮接口。截止时间在**前端**算成本机今天零点 + 本机时区偏移再交给后端 ——
   * 后端没有（也不该有）用户时区的可靠信息，在那边猜等于给跨时区用户挪到昨天或明天。
   *
   * @param tasks 要顺延的任务（空数组直接返回，不发命令）
   * @throws 后端返回的错误文案（未配 Token / 网络 / 鉴权）
   */
  async function postponeTasks(tasks: readonly TodoTaskItem[]): Promise<void> {
    const fresh = tasks.filter(task => !pendingTaskIds.value.includes(task.id))
    if (fresh.length === 0) return
    const ids = fresh.map(task => task.id)
    pendingTaskIds.value = [...pendingTaskIds.value, ...ids]
    const now = Date.now()
    try {
      const data = await invoke<TodoSnapshot>('postpone_todo_tasks', {
        targets: fresh.map(task => ({ projectId: task.project_id, taskId: task.id })),
        dueAtMs: localDayStartMs(now),
        offsetMinutes: localUtcOffsetMinutes(now),
      })
      applySnapshot(data)
    } finally {
      pendingTaskIds.value = pendingTaskIds.value.filter(id => !ids.includes(id))
    }
  }

  /**
   * 删除一条任务（设置页右键菜单「删除目标」）。
   *
   * ⚠️ **不可撤销**：二次确认由调用方负责，这里只管发命令 + 灌新快照。
   *
   * @param task 要删除的任务
   * @throws 后端返回的错误文案（未配 Token / 网络 / 鉴权）
   */
  async function deleteTask(task: TodoTaskItem): Promise<void> {
    if (pendingTaskIds.value.includes(task.id)) return
    pendingTaskIds.value = [...pendingTaskIds.value, task.id]
    try {
      const data = await invoke<TodoSnapshot>('delete_todo_task', {
        projectId: task.project_id,
        taskId: task.id,
      })
      applySnapshot(data)
    } finally {
      pendingTaskIds.value = pendingTaskIds.value.filter(id => id !== task.id)
    }
  }

  /**
   * 新建一条「今天」的待办（设置页「新增任务」表单触发）。
   *
   * 截止时间在**前端**算成本机今天零点 + 本机时区偏移再交给后端：后端没有（也不该有）
   * 用户时区的可靠信息，在那边猜等于给跨时区用户建到昨天或明天。
   *
   * @param input 标题 + 优先级
   * @returns 建完后的最新快照（已灌进 store）
   * @throws 后端返回的错误文案（未配 Token / 网络 / 参数被拒）
   */
  async function createTask(input: TodoCreateInput): Promise<TodoSnapshot> {
    const now = Date.now()
    isCreating.value = true
    try {
      const data = await invoke<TodoSnapshot>('create_todo_task', {
        title: input.title.trim(),
        priority: input.priority,
        dueAtMs: localDayStartMs(now),
        offsetMinutes: localUtcOffsetMinutes(now),
      })
      applySnapshot(data)
      return data
    } finally {
      isCreating.value = false
    }
  }

  /**
   * 测试连接：用已存 Token 强制拉一次快照（仅主窗口设置页调用）。
   *
   * @returns 最新快照
   */
  async function testConnection(): Promise<TodoSnapshot> {
    return await invoke<TodoSnapshot>('test_todo_connection')
  }

  /** 根据当前配置同步组件工作状态（启用 / 禁用、Token 有无、刷新周期变化）。 */
  function syncFromConfig(): void {
    const modulesStore = useFloatingModulesStore()
    const enabled = isModuleEnabled(modulesStore.config, 'todo', false)
    const settings = todoSettings(modulesStore.config)

    // 关掉组件 / 没配 Token = 停止后台工作（规范第 1.8 节：关掉就真的停）。
    if (!enabled || !settings.has_token) {
      stopPolling()
      return
    }

    if (!isOwnerWindow) {
      // 非 owner 窗口只做一次只读补齐，不跑定时器。
      if (!snapshot.value) {
        void fetchSnapshot(false)
      }
      return
    }

    restartPolling(settings.refresh_interval_min)
    void fetchSnapshot(false)
  }

  /**
   * 启动生命周期（由模块定义的 `activate` 调用，三个窗口各调一次）。
   *
   * 等着外壳配置就绪再决定要不要开工：配置异步到达，拿兜底配置直接打网是错的。
   */
  async function start(): Promise<void> {
    const modulesStore = useFloatingModulesStore()
    await modulesStore.start()

    if (!stopListener) {
      try {
        stopListener = await listen<TodoSnapshot>('todo-snapshot', event => {
          applySnapshot(event.payload)
        })
      } catch (error) {
        console.warn('todo-snapshot listen failed', error)
      }
    }

    stopConfigWatch ??= watch(
      () => [isModuleEnabled(modulesStore.config, 'todo', false), todoSettings(modulesStore.config)],
      () => {
        syncFromConfig()
      },
      { deep: true, immediate: true }
    )
  }

  /** 停止生命周期（幂等：重复调用不会留下第二个监听或定时器）。 */
  function stop(): void {
    stopPolling()
    stopListener?.()
    stopListener = undefined
    stopConfigWatch?.()
    stopConfigWatch = undefined
  }

  return {
    snapshot,
    isLoading,
    lastError,
    hasSavedToken,
    projects,
    pendingTaskIds,
    isCreating,
    fetchSnapshot,
    completeTask,
    completeTasks,
    reopenTask,
    postponeTasks,
    deleteTask,
    createTask,
    start,
    stop,
    checkTokenStatus,
    saveToken,
    deleteToken,
    refreshProjects,
    testConnection,
  }
})
