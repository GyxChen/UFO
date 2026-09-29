import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  STATUS_LOG_EVENT,
  emptyLogUsage,
  type ExportedLog,
  type LogDiskUsage,
  type LogPosition,
  type StatusLogCount,
  type StatusLogEntry,
  type StatusLogPage,
  type StatusLogQuery,
} from '../lib/status-log'

let stopListener: UnlistenFn | undefined

/** 详情条目一次最多读多少行：折叠态只露摘要那几条，其余靠「看更早的」与浮层。 */
export const PEEK_WINDOW = 50

/** 浮层右栏「同一条信号的其它行」一次最多读多少行（与 PEEK_WINDOW 同数不同用途）。 */
export const TRACE_WINDOW = 50

/** 空页：后端还没答话时也要有个结构完整的对象可渲染（面板不许判 undefined）。 */
function emptyPage(): StatusLogPage {
  return {
    head: 0,
    dir: '',
    usage: emptyLogUsage(),
    next_before: null,
    exhausted: true,
    scanned_bytes: 0,
    entries: [],
  }
}

/**
 * 收到的载荷必须是页形状：事件可能被旧二进制或别的窗口发歪，
 * 结构不对就整份丢掉（比按半个对象更新状态安全）。
 *
 * `usage` 也在这道校验里：它是设置页那个占用读数的唯一来源，缺了它界面会显示「还没写过日志」，
 * 那是**错误的读数**而不是空白 —— 所以宁可不认这一页。
 */
function isPage(value: unknown): value is StatusLogPage {
  const page = value as StatusLogPage | null
  return (
    !!page &&
    typeof page.head === 'number' &&
    typeof page.exhausted === 'boolean' &&
    Array.isArray(page.entries) &&
    typeof page.dir === 'string' &&
    !!page.usage &&
    typeof page.usage.bytes === 'number' &&
    typeof page.usage.files === 'number'
  )
}

/**
 * 转灯日志 store（主窗「会话记录」页的数据源）。
 *
 * 三条来源都汇到同一份状态：初始 `get_status_log`、monitor 每轮广播的新行、
 * 用户往上翻页。⚠️ 只有本 store `start()` 过（也就是那一页挂着）后端才广播 ——
 * `set_status_log_view` 决定 monitor 每轮要不要发那一批，没人看时零 IPC。
 */
export const useStatusLogStore = defineStore('status-log', {
  state: () => ({
    /** 面板顺序：新→旧（与后端 `StatusLogPage.entries` 同序）。 */
    entries: [] as StatusLogEntry[],
    /** 环形水位：只用来判断「这一轮有没有新行进来过」，据此触发重算计数。 */
    head: 0,
    /** 日志目录：常驻显示，用户才知道去哪翻历史。 */
    dir: '',
    /** 目录占用读数（字节 / 文件数 / 最早一天 / 是否被容量线削过）：设置页常驻显示。 */
    usage: emptyLogUsage() as LogDiskUsage,
    /** 下一页的锚点（最近一次**分页**结果的 `next_before`）；null = 还没翻过页。 */
    nextBefore: null as LogPosition | null,
    /** 已经翻到目录头：「看更早的」据此收起。 */
    exhausted: true,
    /** 是否已经 `start()` 过（幂等，重复调不重新订阅）。 */
    started: false,
    /** 首次拉取是否完成：区分「真的没有日志」与「还没读到」。 */
    loaded: false,
    /** IPC 通道状态。 */
    connection: 'starting' as 'starting' | 'connected' | 'degraded',
    /** 跟随最新：关掉后新行不再挤进视图，攒着的条数走 `pending`。 */
    following: true,
    /**
     * 列表是否**钉在顶部**（由组件的 scroll 回调写）。
     *
     * ⚠️ 只有在顶部时才让实时行前插：用户翻到中间看某一段时，前插会把那一整段挤走，
     * 而等高行的前插锚定补偿要读旧首行位置再回滚，是这类列表最容易错的一块 —— 用「钉顶才跟」换掉它。
     */
    pinned: true,
    /** 暂停跟随期间收到的条数。 */
    pending: 0,
    /** 翻页在途标记，避免连点重复请求。 */
    loadingMore: false,
    /**
     * 「记录详情」里那个条目用的行（只按当前选中会话查，最多 [`PEEK_WINDOW`] 条）。
     *
     * 与 `entries`（浮层的过滤视图）分开存：两处查询条件不同，共用一份会互相顶掉。
     */
    peek: [] as StatusLogEntry[],
    /**
     * 最近一次按盘计数的结果（精确值 + 它是截至哪一刻的数）。
     *
     * ⚠️ 条数**不来自** `entries.length`：那是「已经翻进来的行数」，与「这个会话有多少条」
     * 是两回事 —— 把前者写成后者，就是当年那句「本会话 50+ 条」的成因。
     */
    count: null as StatusLogCount | null,
    /** 计数进行到哪一步：四态各有自己的文案，任何一档都不许出现「+」或「约」。 */
    countState: 'idle' as 'idle' | 'loading' | 'ready' | 'failed',
    /** 这份计数属于哪个查询（换会话 / 换过滤时要能看出是不是在答当前这个问题）。 */
    countKey: '',
  }),

  getters: {
    /** 还能不能往上翻：到底了就不该再留按钮（点了什么也不会有）。 */
    canPageBack(): boolean {
      return !this.exhausted
    },
  },

  actions: {
    /**
     * 订阅 + 首拉。
     *
     * 失败要落到 `degraded` 而不是留 `loaded = false`：否则界面永远停在「正在读取」，
     * 用户分不清是没数据还是读不到数据。
     */
    async start() {
      if (this.started) return
      this.started = true
      // 重新进这一页 = 视图从头看起，跟随状态也归位（上次可能停在「人已滚走」）。
      this.following = true
      this.pinned = true
      try {
        await invoke('set_status_log_view', { attached: true })
        stopListener = await listen<unknown>(STATUS_LOG_EVENT, event => {
          if (isPage(event.payload)) this.applyPage(event.payload, 'merge')
        })
        this.applyPage(await invoke<StatusLogPage>('get_status_log', { query: null }), 'replace')
      } catch (error) {
        this.connection = 'degraded'
        this.loaded = true
        console.warn('status log channel unavailable', error)
      }
    },

    /** 卸载：停订阅并告诉后端「没人看了」，让 monitor 停止每轮广播。 */
    stop() {
      stopListener?.()
      stopListener = undefined
      if (this.started) {
        this.started = false
        // 通知后端只是省事优化：失败也不影响界面，吞掉即可（面板已经不再收数据）。
        void invoke('set_status_log_view', { attached: false }).catch(() => {})
      }
    },

    /**
     * 吸收一页数据。
     *
     * - `replace`：整份换掉（首拉、清空、改了过滤条件之后重新拉）。
     * - `merge`：广播来的**增量**前插，按 `id`（盘上坐标）去重（同一轮可能和首拉重叠）。
     */
    applyPage(page: StatusLogPage, mode: 'replace' | 'merge') {
      this.head = page.head
      this.dir = page.dir
      this.usage = page.usage
      this.loaded = true
      this.connection = 'connected'

      if (mode === 'replace') {
        this.entries = page.entries
        this.pending = 0
        // 翻页锚点与「到底没有」只认**分页**结果：实时增量包不带这两个值。
        this.nextBefore = page.next_before
        this.exhausted = page.exhausted
        return
      }
      if (!this.following || !this.pinned) {
        // 暂停跟随、或人已经滚离顶部：新行只累计条数，不动画面（浮层那颗 chip 负责「跟回最新」）。
        this.pending += page.entries.length
        return
      }
      // 实时行与分页行共用盘上坐标做身份 ⇒ 这一轮和首拉重叠的部分自然被摘掉。
      const known = new Set(this.entries.map(entry => entry.id))
      const fresh = page.entries.filter(entry => !known.has(entry.id))
      if (!fresh.length) return
      this.entries = [...fresh, ...this.entries]
    },

    /** 按条件重新拉一页（环节 / 只看被拒 / 换了选中会话都走这里）。 */
    async refresh(query: StatusLogQuery) {
      try {
        this.applyPage(await invoke<StatusLogPage>('get_status_log', { query }), 'replace')
      } catch (error) {
        this.connection = 'degraded'
        console.warn('status log query failed', error)
      }
    },

    /**
     * 按条件精确重算条数（`count_status_log`）。
     *
     * ⚠️ 这条命令必定把该条件涉及的目录整个翻完，成本靠后端「3 秒节流 + 4 条 LRU 缓存」兜；
     * 前端因此只在「选中会话变了」与「head 前进（盘上确实多了行）」两个时机调它，不自建定时器。
     * 失败要说「读不到」而不是留着上一个数 —— 旧数会被当成事实。
     * `identity` 是这次查询归属的键（选中会话 / 过滤条件串），用来丢弃迟到的回包。
     */
    async refreshCount(query: StatusLogQuery, identity: string) {
      this.countKey = identity
      this.countState = 'loading'
      try {
        const count = await invoke<StatusLogCount>('count_status_log', { query })
        if (this.countKey === identity) {
          this.count = count
          this.countState = 'ready'
        }
      } catch (error) {
        if (this.countKey === identity) {
          this.countState = 'failed'
        }
        console.warn('status log count failed', error)
      }
    },

    /** 按 `trace_id` 读一条信号的完整链路（浮层右栏用，不受那边勾选的过滤条件影响）。 */
    async queryTrace(traceId: string): Promise<StatusLogEntry[]> {
      try {
        const page = await invoke<StatusLogPage>('get_status_log', {
          query: { trace_id: traceId, limit: TRACE_WINDOW },
        })
        return page.entries
      } catch (error) {
        console.warn('status log trace query failed', error)
        return []
      }
    },

    /**
     * 重读「记录详情」条目那几行（按选中会话）。
     *
     * 由页面在两个时机调用：换了选中项、以及广播让 `head` 前进（说明盘上有了新行）。
     * 走独立查询而不是从 `entries` 里筛，是因为 `entries` 受浮层那套过滤条件影响 ——
     * 浮层勾了「只看被拒」时，详情条目不该跟着变空。
     */
    async refreshPeek(key: string) {
      if (!key) {
        this.peek = []
        return
      }
      try {
        const page = await invoke<StatusLogPage>('get_status_log', {
          query: { key, limit: PEEK_WINDOW },
        })
        this.peek = page.entries
      } catch (error) {
        this.connection = 'degraded'
        console.warn('status log peek failed', error)
      }
    },

    /** 往上翻更旧的：锚点 = 上一次分页给的 `next_before`（后端语义「严格比它更旧」）。 */
    async loadOlder(query: StatusLogQuery) {
      if (this.loadingMore || this.exhausted) return
      this.loadingMore = true
      try {
        const page = await invoke<StatusLogPage>('get_status_log', {
          query: { ...query, before: this.nextBefore },
        })
        const known = new Set(this.entries.map(entry => entry.id))
        // 旧行接在下面，保持整份新→旧；驻留多少由「用户真翻过几页」决定，不再有前端上限。
        this.entries = [
          ...this.entries,
          ...page.entries.filter(entry => !known.has(entry.id)),
        ]
        this.nextBefore = page.next_before
        this.exhausted = page.exhausted
      } catch (error) {
        console.warn('status log paging failed', error)
      } finally {
        this.loadingMore = false
      }
    },

    /** 恢复跟随，并把暂停期间攒的行一次补上（整份重拉最省事，也和后端一致）。 */
    async resumeFollowing(query: StatusLogQuery) {
      this.following = true
      this.pending = 0
      await this.refresh(query)
    },

    /** 清空日志：后端另起新文件并回一份空页，界面立刻收敛。 */
    async clear() {
      this.applyPage(await invoke<StatusLogPage>('clear_status_log'), 'replace')
    },

    /** 按当前条件导出到用户选好的路径（路径校验在后端）。 */
    async exportTo(path: string, query: StatusLogQuery): Promise<ExportedLog> {
      return await invoke<ExportedLog>('export_status_log', { path, query })
    },
  },
})
