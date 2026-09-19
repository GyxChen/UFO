import { describe, expect, it, vi } from 'vitest'
import { retryOnStartupRace } from './startup-retry'

describe('retryOnStartupRace', () => {
  it('首次就成功时不重试', async () => {
    const task = vi.fn(async () => 'ok')

    await expect(retryOnStartupRace(task, 3, 1)).resolves.toBe('ok')
    expect(task).toHaveBeenCalledTimes(1)
  })

  it('前几次失败会继续重试，直到某次成功', async () => {
    let calls = 0
    const task = vi.fn(async () => {
      calls += 1
      if (calls < 3) throw new Error('state not managed for field `state`')
      return calls
    })

    await expect(retryOnStartupRace(task, 5, 1)).resolves.toBe(3)
    expect(task).toHaveBeenCalledTimes(3)
  })

  it('重试用尽后抛出最后一次错误（调用方据此退化）', async () => {
    let calls = 0
    const task = vi.fn(async () => {
      calls += 1
      throw new Error('boom ' + calls)
    })

    await expect(retryOnStartupRace(task, 3, 1)).rejects.toThrow('boom 3')
    expect(task).toHaveBeenCalledTimes(3)
  })
})
