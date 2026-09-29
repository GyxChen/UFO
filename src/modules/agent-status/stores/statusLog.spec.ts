/**
 * 转灯日志 store 的回归测试：合并、翻页、暂停跟随、以及「本会话 N 条」的计数口径。
 *
 * 这里锁的都是**看不出来但会错**的地方：
 * - 广播是增量，必须按 `seq` 去重前插，整份覆盖会把用户正在看的那一段顶掉；
 * - `page.total` 是整个环形缓冲的条数，拿来当「本会话多少条」就是错报；
 * - 暂停跟随时收到的行只涨计数不动画面，恢复时要能补回来。
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { StatusLogEntry, StatusLogPage } from '../lib/status-log'
import { emptyLogUsage, logCountLabel } from '../lib/status-log'
import { PEEK_WINDOW, useStatusLogStore } from './statusLog'

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
vi.mock('@tauri-apps/api/event', () => ({
  listen: listenMock,
  // 类型导出（UnlistenFn）在运行时不存在，补一个空对象免得命名空间导入报错。
  default: {},
}))

/**
 * 造一条记录：只需要游标号、盘上身份与环节，其它字段对这几条断言无意义。
 *
 * ⚠️ `id` 必须给且必须互不相同：store 的合并去重按 `id` 走，缺了它所有行都会撞成同一个
 * `undefined`，测试会以「新行全被当成重复」的形态红掉。
 */
function entry(seq: number, stage = 'kernel_verdict'): StatusLogEntry {
  return {
    id: `2026-09-28.0.${seq}`,
    ts_ms: 1_790_000_000_000 + seq,
    writer: 'monitor',
    stage,
    key: 'claude:s1',
  } as StatusLogEntry
}

/** 行身份（盘上坐标）—— 测试里按它断言顺序，与前端 `:key` 同一个口径。 */
function idOf(...seqs: number[]): string[] {
  return seqs.map(seq => `2026-09-28.0.${seq}`)
}

/** 造一页（后端语义：`entries` 新→旧；`usage` 是设置页占用读数的唯一来源）。 */
function page(entries: StatusLogEntry[], extra: Partial<StatusLogPage> = {}): StatusLogPage {
  return {
    head: entries.length,
    total: entries.length,
    dropped: 0,
    dir: 'X:\\log',
    usage: emptyLogUsage(),
    entries,
    ...extra,
  }
}

/**
 * 取出某条命令**最后一次**调用的 query。
 *
 * ⚠️ 不能用 `find` 拿第一条：`start()` 也发一次 `get_status_log`（query 为 null），
 * 之后动作的入参就永远被它挡住 —— 我第一版就是这么误判成 store 有 bug 的。
 */
function invokedQuery(command: string): unknown {
  const calls = invokeMock.mock.calls.filter(([name]) => name === command)
  return calls[calls.length - 1]?.[1]?.query
}

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  listenMock.mockReset()
  listenMock.mockResolvedValue(() => {})
})

describe('statusLog store', () => {
  it('attaches the viewer on start and detaches on stop', async () => {
    invokeMock.mockImplementation(async (command: string) => (command === 'get_status_log' ? page([]) : undefined))
    const store = useStatusLogStore()
    await store.start()
    expect(invokeMock).toHaveBeenCalledWith('set_status_log_view', { attached: true })
    store.stop()
    expect(invokeMock).toHaveBeenCalledWith('set_status_log_view', { attached: false })
    // 重复 stop 不该再发一次「没人看了」。
    invokeMock.mockClear()
    store.stop()
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('merges broadcast rows without dropping what the user is looking at', async () => {
    invokeMock.mockImplementation(async (command: string) =>
      command === 'get_status_log' ? page([entry(5), entry(4)]) : undefined
    )
    const store = useStatusLogStore()
    await store.start()
    expect(store.entries.map(item => item.id)).toEqual(idOf(5, 4))

    // 广播来的是一批增量，且可能与已有游标重叠（同一轮被首拉与广播各带一次）。
    store.applyPage(page([entry(8), entry(7), entry(5)]), 'merge')
    expect(store.entries.map(item => item.id)).toEqual(idOf(8, 7, 5, 4))
  })

  it('holds rows while following is paused and catches up on resume', async () => {
    invokeMock.mockImplementation(async (command: string) =>
      command === 'get_status_log' ? page([entry(1)]) : undefined
    )
    const store = useStatusLogStore()
    await store.start()
    store.following = false
    store.applyPage(page([entry(3), entry(2)]), 'merge')
    expect(store.entries.map(item => item.id)).toEqual(idOf(1))
    expect(store.pending).toBe(2)

    await store.resumeFollowing({ key: 'claude:s1' })
    expect(store.following).toBe(true)
    expect(store.pending).toBe(0)
    expect(invokedQuery('get_status_log')).toEqual({ key: 'claude:s1' })
  })

  it('reports the peek count as the session rows it actually fetched, never the ring total', async () => {
    const store = useStatusLogStore()
    invokeMock.mockResolvedValue(page([entry(3), entry(2)]))
    await store.refreshPeek('claude:s1')
    expect(invokedQuery('get_status_log')).toEqual({ key: 'claude:s1', limit: PEEK_WINDOW })
    expect(store.peek).toHaveLength(2)

    // 条数只能来自 `count_status_log`：已载入多少行、环形装过多少条都不是「这个会话有多少条」。
    invokeMock.mockImplementation(async (command: string) =>
      command === 'count_status_log'
        ? { total: 9123, as_of_ms: 1_790_000_123_000, scanned_bytes: 4096 }
        : page([])
    )
    await store.refreshCount({ key: 'claude:s1' }, 'claude:s1')
    expect(store.countState).toBe('ready')
    const label = logCountLabel('本会话', store.count, store.countState)
    expect(label).toContain('9123 条')
    // ⚠️ 精确值 + 「截至」时刻，所以任何一档都不该再出现「+」+ 或「约」。
    expect(label).not.toContain('+')
    expect(label).toContain('截至')

    // 换会话时迟到的回包不许覆盖新问题（否则会显示成上一条会话的条数）。
    invokeMock.mockImplementation(async () => {
      await new Promise(resolve => setTimeout(resolve, 0))
      return { total: 1, as_of_ms: 1, scanned_bytes: 1 }
    })
    void store.refreshCount({ key: 'slow' }, 'slow')
    await store.refreshCount({ key: 'fast' }, 'fast')
    expect(store.countKey).toBe('fast')

    // 计数失败要说「读不到」，不许留着上一个数当事实。
    invokeMock.mockRejectedValue(new Error('scan failed'))
    await store.refreshCount({ key: 'claude:s1' }, 'claude:s1')
    expect(store.countState).toBe('failed')
    expect(logCountLabel('本会话', store.count, 'failed')).toBe('本会话计数暂时读不到')
  })

  /**
   * 「跟随最新」改成只在钉顶时前插：人翻到中间看某一段时，前插会把那一段挤走。
   * 不前插就不必做滚动锚定补偿（等高列表最容易错的一块），代价是「跟随」的语义变成
   * 「钉在顶上才跟」—— 攒着的条数由浮层那颗 chip 兜住。
   */
  it('holds incoming rows when the list is scrolled away from the top', async () => {
    invokeMock.mockImplementation(async (command: string) =>
      command === 'get_status_log' ? page([entry(1)]) : undefined
    )
    const store = useStatusLogStore()
    await store.start()
    expect(store.pinned).toBe(true)

    // 滚离顶部后广播增量：画面一行都不许多出来。
    store.pinned = false
    store.applyPage(page([entry(3), entry(2)]), 'merge')
    expect(store.entries.map(item => item.id)).toEqual(idOf(1))
    expect(store.pending).toBe(2)

    // 回到顶部并「跟回最新」：攒下的行一次补上。
    store.pinned = true
    await store.resumeFollowing({ key: 'claude:s1' })
    expect(store.pending).toBe(0)

    // 重新进这一页时跟随状态归位（上次可能停在「人已滚走」）。
    store.started = false
    await store.start()
    expect(store.following).toBe(true)
    expect(store.pinned).toBe(true)
  })

  it('pages back with the disk anchor, stops at the head and never repeats a row', async () => {
    const anchor = { day: '2026-09-28', file_seq: 0, offset: 800 }
    invokeMock.mockResolvedValue(page([entry(9), entry(8)], { next_before: anchor, exhausted: false }))
    const store = useStatusLogStore()
    await store.refresh({ key: 'claude:s1', limit: 2 })
    expect(store.nextBefore).toEqual(anchor)
    expect(store.exhausted).toBe(false)

    // 下一页的锚点就是上一次分页给的 `next_before`（后端语义：严格比它更旧）。
    invokeMock.mockResolvedValue(
      page([entry(8), entry(7), entry(6)], { next_before: null, exhausted: true })
    )
    await store.loadOlder({ key: 'claude:s1', limit: 2 })
    expect(invokedQuery('get_status_log')).toEqual({ key: 'claude:s1', limit: 2, before: anchor })
    // 更旧的接在下面，重叠的那条不许出现两次；顺序仍是新→旧。
    expect(store.entries.map(item => item.id)).toEqual(idOf(9, 8, 7, 6))
    expect(store.exhausted).toBe(true)

    // 到底了就别再发请求：那只会让「看更早的」变成一个一直点了没反应的按钮。
    invokeMock.mockClear()
    await store.loadOlder({ key: 'claude:s1', limit: 2 })
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('degrades instead of spinning when the channel is unavailable', async () => {
    invokeMock.mockRejectedValue(new Error('ipc down'))
    const store = useStatusLogStore()
    await store.start()
    // 永远停在「正在读取」会让人以为数据还在路上；这里必须落到「读不到」。
    expect(store.connection).toBe('degraded')
    expect(store.loaded).toBe(true)
  })
})
