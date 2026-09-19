import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { ref, watch, type WatchStopHandle } from 'vue'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import { isModuleEnabled } from '@/lib/floating-modules'
import { todoSettings } from '../settings'

/**
 * 滴答清单里的一条未完成任务。字段与 Rust 侧 `models.rs` 的 `TodoTaskItem` 一一对应。
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
}

/**
 * 待办快照：未完成列表整体。由 Rust `get_todo_snapshot` 返回并广播 `todo-snapshot`。
 */
export interface TodoSnapshot {
  /** 未完成任务，按「有截止的在前（截止早的在前）→ 优先级高 → 标题」排好 */
  list: TodoTaskItem[]
  /** 未完成任务总数（= list 长度；滴答单次最多返回 200 条） */
  total_incomplete: number
  /** 快照构建时刻（毫秒），相对日期一律以它为参照 */
  updated_at_ms: number
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
      { deep: true, immediate: true },
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
    fetchSnapshot,
    start,
    stop,
    checkTokenStatus,
    saveToken,
    deleteToken,
    refreshProjects,
    testConnection,
  }
})
