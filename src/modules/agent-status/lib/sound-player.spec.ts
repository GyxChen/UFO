// @vitest-environment happy-dom

/**
 * 状态灯音效的播放端：音频不在前端 bundle 里，而是**按需**从安装路径下 `data/bgm` 读字节。
 *
 * 这里锁三件事：
 * - 读的是后端命令，且**名字就是约定的三个文件名**（Rust 侧按白名单校验，名字对不上就哑）；
 * - 同一段音效只读一次（连击时不该反复发 IPC）；
 * - 读不到 / 播放失败都**不能抛**——提示音挂了不能带倒状态灯。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))

/** 每次取一份**全新**的模块实例：播放端内部有「已加载」缓存，跨用例必须隔离。 */
async function loadPlayer() {
  vi.resetModules()
  return await import('./sound-player')
}

/** 造一段假音频字节。 */
function audioBytes() {
  return new Uint8Array([0x49, 0x44, 0x33, 0x03, 0x00]).buffer
}

beforeEach(() => {
  invokeMock.mockReset()
  // happy-dom 不一定实现 createObjectURL；本用例只关心「拿到字节 → 建地址」这一步。
  Object.defineProperty(URL, 'createObjectURL', {
    configurable: true,
    writable: true,
    value: vi.fn(() => 'blob:sound'),
  })
  // 播放本身不测（测试环境没有音频设备），只断言「确实去播了」。
  vi.spyOn(HTMLAudioElement.prototype, 'play').mockImplementation(() => Promise.resolve())
})

// ⚠️ 必须还原原型上的 spy：`vi.spyOn` 对已被 spy 过的方法会**复用同一个 mock**，
// 不还原的话上一个用例的调用次数会带到下一个用例里。
afterEach(() => {
  vi.restoreAllMocks()
})

describe('preloadLightSounds', () => {
  it('reads all three sounds once, with the agreed file names', async () => {
    invokeMock.mockResolvedValue(audioBytes())
    const { preloadLightSounds } = await loadPlayer()

    preloadLightSounds()
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(3))

    const requested = invokeMock.mock.calls.map(call => call[1]?.name).sort()
    expect(requested).toEqual(['goError.mp3', 'goSucc.mp3', 'goWarn.mp3'])
    expect(invokeMock.mock.calls.every(call => call[0] === 'read_light_sound')).toBe(true)

    // 再预热一次：命中缓存，不再发 IPC。
    preloadLightSounds()
    await Promise.resolve()
    expect(invokeMock).toHaveBeenCalledTimes(3)
  })
})

describe('playLightSound', () => {
  it('plays the sound it read from the install directory', async () => {
    invokeMock.mockResolvedValue(audioBytes())
    const { playLightSound } = await loadPlayer()

    playLightSound('warn')
    await vi.waitFor(() => expect(HTMLAudioElement.prototype.play).toHaveBeenCalledTimes(1))

    expect(invokeMock).toHaveBeenCalledWith('read_light_sound', { name: 'goWarn.mp3' })
    expect(URL.createObjectURL).toHaveBeenCalledTimes(1)
  })

  it('stays silent and warns once when the file cannot be read', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    invokeMock.mockRejectedValue(new Error('unreadable'))
    const { playLightSound } = await loadPlayer()

    // 不抛：提示音读不到不能影响状态灯本身。
    playLightSound('error')
    playLightSound('success')
    await vi.waitFor(() => expect(warn).toHaveBeenCalledTimes(1))

    expect(HTMLAudioElement.prototype.play).not.toHaveBeenCalled()
    warn.mockRestore()
  })

  it('ignores an empty payload', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    invokeMock.mockResolvedValue(new ArrayBuffer(0))
    const { playLightSound } = await loadPlayer()

    playLightSound('error')
    await vi.waitFor(() => expect(warn).toHaveBeenCalledTimes(1))

    expect(HTMLAudioElement.prototype.play).not.toHaveBeenCalled()
    warn.mockRestore()
  })
})
