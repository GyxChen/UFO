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

/**
 * CSP 必须放行 `blob:` 媒体。
 *
 * 提示音的地址是 `URL.createObjectURL(...)` 造的 `blob:`，而 CSP 没写 `media-src` 时会
 * 回落到 `default-src` —— 那里只有 `'self' customprotocol: asset:`，**不含 `blob:`**，
 * 于是打包态（CSP 由 `tauri://` 协议加响应头，dev 走 Vite 所以不受管）媒体加载被拦，
 * 表现就是「灯在跳、就是没声」，而且 `play()` 的失败被静默吞掉，界面上毫无痕迹。
 *
 * ⚠️ 这条断言是**单向**的护栏（它查配置，不查浏览器行为）：删掉 `media-src` 必须变红。
 */
describe('CSP 与 blob: 音效同源', () => {
  it('media-src 允许 blob:（否则装机版提示音会被拦）', async () => {
    const confRaw = (await import('../../../../src-tauri/tauri.conf.json?raw')).default
    const conf = JSON.parse(confRaw.replace(/\r\n/g, '\n').replace(/^/, '')) as {
      app: { security: { csp?: Record<string, string> } }
    }
    const csp = conf.app.security.csp
    expect(csp, 'tauri.conf.json 里必须有 app.security.csp').toBeTruthy()

    const mediaSrc = csp['media-src']
    expect(mediaSrc, '缺 media-src 时会回落到 default-src，而它不含 blob:').toBeTruthy()
    expect(mediaSrc.split(/\s+/), `media-src 里没有 blob:：${mediaSrc}`).toContain('blob:')

    // default-src 单独看一眼：它确实不带 blob:，所以上面那条不是冗余断言。
    expect(csp['default-src'].split(/\s+/)).not.toContain('blob:')
  })
})
