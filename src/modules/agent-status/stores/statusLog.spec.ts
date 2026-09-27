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

/** 造一条记录：只需要游标号与环节，其它字段对这几条断言无意义。 */
function entry(seq: number, stage = 'kernel_verdict'): StatusLogEntry {
  return { seq, ts_ms: 1_790_000_000_000 + seq, writer: 'monitor', stage } as StatusLogEntry
}

/** 造一页（后端语义：`entries` 新→旧）。 */
function page(entries: StatusLogEntry[], extra: Partial<StatusLogPage> = {}): StatusLogPage {
  return { head: entries[0]?.seq ?? 0, total: entries.length, dropped: 0, dir: 'X:\\log', entries, ...extra }
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
    expect(store.entries.map(item => item.seq)).toEqual([5, 4])

    // 广播来的是一批增量，且可能与已有游标重叠（同一轮被首拉与广播各带一次）。
    store.applyPage(page([entry(8), entry(7), entry(5)]), 'merge')
    expect(store.entries.map(item => item.seq)).toEqual([8, 7, 5, 4])
  })

  it('holds rows while following is paused and catches up on resume', async () => {
    invokeMock.mockImplementation(async (command: string) =>
      command === 'get_status_log' ? page([entry(1)]) : undefined
    )
    const store = useStatusLogStore()
    await store.start()
    store.following = false
    store.applyPage(page([entry(3), entry(2)]), 'merge')
    expect(store.entries.map(item => item.seq)).toEqual([1], '暂停跟随时画面不许动')
    expect(store.pending).toBe(2)

    await store.resumeFollowing({ key: 'claude:s1' })
    expect(store.following).toBe(true)
    expect(store.pending).toBe(0)
    expect(invokedQuery('get_status_log')).toEqual({ key: 'claude:s1' })
  })

  it('reports the peek count as the session rows it actually fetched, never the ring total', async () => {
    const store = useStatusLogStore()
    invokeMock.mockResolvedValue(page([entry(3), entry(2)], { total: 5000, dropped: 400 }))
    await store.refreshPeek('claude:s1')
    // ⚠️ `total` 是整个环形缓冲的条数：把它写成「本会话 5000 条」就是当面向用户报错数。
    expect(store.peekLabel).toBe('本会话 2 条')
    expect(invokedQuery('get_status_log')).toEqual({ key: 'claude:s1', limit: PEEK_WINDOW })

    // 捞满一窗就说不准了，只能报「N+」。
    invokeMock.mockResolvedValue(page(Array.from({ length: PEEK_WINDOW }, (_, index) => entry(index))))
    await store.refreshPeek('claude:s1')
    expect(store.peekLabel).toBe(`本会话 ${PEEK_WINDOW}+ 条`)

    // 没选中会话时不发请求，直接把行清空。
    invokeMock.mockClear()
    await store.refreshPeek('')
    expect(store.peek).toEqual([])
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('pages back with the oldest visible cursor and keeps the order new-to-old', async () => {
    invokeMock.mockResolvedValue(page([entry(9), entry(8)]))
    const store = useStatusLogStore()
    await store.start()
    invokeMock.mockResolvedValue(page([entry(8), entry(7), entry(6)]))
    await store.loadOlder({ key: 'claude:s1', limit: 2 })
    expect(invokedQuery('get_status_log')).toMatchObject({ before_seq: 8 })
    // 更旧的接在下面，重叠的那条不许出现两次。
    expect(store.entries.map(item => item.seq)).toEqual([9, 8, 7, 6])
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
