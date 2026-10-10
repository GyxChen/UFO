/**
 * 开发期热重载轮询的护栏。
 *
 * 这一条链的坏法全是「看起来在动但其实没动」：
 * - 第一次问就重挂 → 每次启动都白装一遍（并且会抖掉正在编辑的界面）；
 * - 失败不停 → IPC 断了的时候一秒敲一次门，日志被刷死；
 * - 命令名写错 → 表现是「开了热重载但改了文件没反应」，最难归因的一类。
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
// ?raw 读 Rust 源：命令名两边各写一遍，这里断言的正是「文件里写着什么」。
import commandsRaw from '../../src-tauri/src/plugin/commands.rs?raw'
import libRaw from '../../src-tauri/src/lib.rs?raw'
import { startPluginDevReload } from './plugin-dev-reload'

/** 一轮测试的可控输入：按 tick 顺序吐出这些指纹（越界后重复最后一个）。 */
function harness(digests: number[], hasDevPackages = () => true) {
  let index = 0
  const reload = vi.fn()
  const fetchDigest = vi.fn(async () => {
    const value = digests[Math.min(index, digests.length - 1)] ?? 0
    index += 1
    return value
  })
  const stop = startPluginDevReload({ hasDevPackages, reload, fetchDigest })
  return { reload, fetchDigest, stop }
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('什么时候问、什么时候重挂', () => {
  it('没有包开着热重载时一次都不问', async () => {
    const { fetchDigest, stop } = harness([1], () => false)
    await vi.advanceTimersByTimeAsync(10_000)
    expect(fetchDigest).not.toHaveBeenCalled()
    stop()
  })

  it('第一次只记基线，指纹变了才重挂一次，没变就不动', async () => {
    const { reload, stop } = harness([7, 7, 8, 8, 9])
    await vi.advanceTimersByTimeAsync(2_000)
    expect(reload).not.toHaveBeenCalled() // 基线那一轮
    await vi.advanceTimersByTimeAsync(2_000)
    expect(reload).not.toHaveBeenCalled() // 指纹没变
    await vi.advanceTimersByTimeAsync(2_000)
    expect(reload).toHaveBeenCalledTimes(1) // 7 → 8
    await vi.advanceTimersByTimeAsync(2_000)
    expect(reload).toHaveBeenCalledTimes(1) // 8 → 8
    await vi.advanceTimersByTimeAsync(2_000)
    expect(reload).toHaveBeenCalledTimes(2) // 8 → 9
    stop()
  })

  it('stop() 之后不再问，也不会在途的那一次回来后重挂', async () => {
    const { reload, fetchDigest, stop } = harness([1, 2])
    await vi.advanceTimersByTimeAsync(2_000)
    stop()
    const asked = fetchDigest.mock.calls.length
    await vi.advanceTimersByTimeAsync(10_000)
    expect(fetchDigest).toHaveBeenCalledTimes(asked)
    expect(reload).not.toHaveBeenCalled()
  })
})

describe('问不到就收手', () => {
  it('连续三次失败就停掉轮询，并留下一句原因', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    const reload = vi.fn()
    const fetchDigest = vi.fn(async (): Promise<number> => {
      throw new Error('ipc down')
    })
    const stop = startPluginDevReload({ hasDevPackages: () => true, reload, fetchDigest })
    // 多走几轮：停在第三次，之后一次都不该再问。
    for (let round = 0; round < 6; round += 1) {
      await vi.advanceTimersByTimeAsync(2_000)
    }
    expect(fetchDigest).toHaveBeenCalledTimes(3)
    expect(reload).not.toHaveBeenCalled()
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('问不到了'), expect.anything())
    warn.mockRestore()
    stop()
  })
})

describe('命令名与 Rust 同源', () => {
  it('指纹命令在 Rust 里有定义并挂进了 handler', () => {
    const commands = commandsRaw.replace(/\r\n/g, '\n')
    const lib = libRaw.replace(/\r\n/g, '\n')
    expect(commands).toMatch(/pub async fn get_plugin_dev_digest\(/)
    expect(lib).toMatch(/get_plugin_dev_digest,/)
  })
})
