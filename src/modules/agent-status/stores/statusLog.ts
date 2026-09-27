import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  STATUS_LOG_EVENT,
  type ExportedLog,
  type StatusLogEntry,
  type StatusLogPage,
  type StatusLogQuery,
} from '../lib/status-log'

let stopListener: UnlistenFn | undefined

/** 客户端最多留多少条在内存里（后端环形 5000，面板再截一层免得把主窗拖慢）。 */
const CLIENT_CAP = 1200

/** 缺省页大小：与 Rust `StatusLogQuery` 的 `default_limit` 对齐，别两边各写一个数。 */
const DEFAULT_LIMIT = 200

/** 详情条目一次最多读多少行：够数出「本会话多少条」，又不至于把整段日志搬进主窗。 */
export const PEEK_WINDOW = 50

/** 导出时一次带多少条：按后端环形容量取，导出的语义是「把当前看得见的都带走」。 */
export const EXPORT_LIMIT = 5000

/** 空页：后端还没答话时也要有个结构完整的对象可渲染（面板不许判 undefined）。 */
function emptyPage(): StatusLogPage {
  return { head: 0, total: 0, dropped: 0, dir: '', entries: [] }
}

/**
 * 收到的载荷必须是页形状：事件可能被旧二进制或别的窗口发歪，
 * 结构不对就整份丢掉（比按半个对象更新状态安全）。
 */
function isPage(value: unknown): value is StatusLogPage {
  const page = value as StatusLogPage | null
  return (
    !!page &&
    typeof page.head === 'number' &&
    Array.isArray(page.entries) &&
    typeof page.dir === 'string'
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
    /** 后端环形缓冲的游标水位：判断有没有新数据、翻页锚点都靠它。 */
    head: 0,
    /** 当前环形缓冲里总共多少条。 */
    total: 0,
    /** 被环形容量挤掉多少条（面板要说明「更早的只能去文件里翻」）。 */
    dropped: 0,
    /** 日志目录：常驻显示，用户才知道去哪翻历史。 */
    dir: '',
    /** 是否已经 `start()` 过（幂等，重复调不重新订阅）。 */
    started: false,
    /** 首次拉取是否完成：区分「真的没有日志」与「还没读到」。 */
    loaded: false,
    /** IPC 通道状态。 */
    connection: 'starting' as 'starting' | 'connected' | 'degraded',
    /** 跟随最新：关掉后新行不再挤进视图，攒着的条数走 `pending`。 */
    following: true,
    /** 暂停跟随期间收到的条数。 */
    pending: 0,
    /** 是否还能往上翻（后端返回的页满了才可能还有更旧的）。 */
    canPageBack: false,
    /** 翻页在途标记，避免连点重复请求。 */
    loadingMore: false,
    /**
     * 「记录详情」里那个条目用的行（只按当前选中会话查，最多 [`PEEK_WINDOW`] 条）。
     *
     * 与 `entries`（浮层的过滤视图）分开存：两处查询条件不同，共用一份会互相顶掉。
     */
    peek: [] as StatusLogEntry[],
  }),

  getters: {
    /**
     * 条目的计数文案。
     *
     * ⚠️ 后端 `total` 是**整个环形缓冲**的条数，不是这个会话的，直接拿来写「本会话 N 条」就是错报。
     * 所以计数按 peek 实际捞到的条数说：捞满了就报「N+」—— 宁可少说，不含糊其辞。
     */
    peekLabel(): string {
      if (!this.peek.length) return '本会话 0 条'
      if (this.peek.length >= PEEK_WINDOW) return `本会话 ${PEEK_WINDOW}+ 条`
      return `本会话 ${this.peek.length} 条`
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
     * - `merge`：广播来的**增量**前插，按 `seq` 去重（同一轮可能和首拉重叠）。
     */
    applyPage(page: StatusLogPage, mode: 'replace' | 'merge') {
      this.head = page.head
      this.total = page.total
      this.dropped = page.dropped
      this.dir = page.dir
      this.loaded = true
      this.connection = 'connected'

      if (mode === 'replace') {
        this.entries = page.entries.slice(0, CLIENT_CAP)
        this.pending = 0
        this.canPageBack = page.entries.length >= DEFAULT_LIMIT
        return
      }
      if (!this.following) {
        // 暂停跟随时新行照单收下条数，但不改画面：用户可能正盯着中间某一行。
        this.pending += page.entries.length
        return
      }
      const known = new Set(this.entries.map(entry => entry.seq))
      const fresh = page.entries.filter(entry => !known.has(entry.seq))
      if (!fresh.length) return
      this.entries = [...fresh, ...this.entries].slice(0, CLIENT_CAP)
      this.canPageBack = this.entries.length >= CLIENT_CAP || this.canPageBack
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

    /** 往上翻更旧的：`before_seq` 取当前**最后一条**的游标（后端语义是「比它更旧」）。 */
    async loadOlder(query: StatusLogQuery) {
      const last = this.entries[this.entries.length - 1]
      if (!last || this.loadingMore) return
      this.loadingMore = true
      try {
        const page = await invoke<StatusLogPage>('get_status_log', {
          query: { ...query, before_seq: last.seq },
        })
        const known = new Set(this.entries.map(entry => entry.seq))
        // 旧行接在下面，保持整份新→旧。
        this.entries = [...this.entries, ...page.entries.filter(entry => !known.has(entry.seq))].slice(
          0,
          CLIENT_CAP
        )
        this.canPageBack = page.entries.length >= (query.limit ?? DEFAULT_LIMIT)
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
