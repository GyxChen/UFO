/**
 * 状态灯音效的播放端：向后端要三次「安装路径里那份 mp3 的字节」，转成 `blob:` 地址播放。
 *
 * ⚠️ 音频**不打包进前端**：开发目录 ≠ 安装目录，编进 bundle 的音频用户改不了。
 * 真正的文件在 `<安装路径>/data/bgm/`（Rust 侧 `sound.rs` 负责创建目录、补齐随包默认音效、
 * 并按白名单读字节），用户拿同名 mp3 覆盖即可换音（**重启后生效**）。
 * ⚠️ 目录路径**不从后端要**：设置页那句提示是固定文案（见 `GeneralSection.vue`），
 * 免得把 `target/debug/data/bgm` 这种开发期路径摆给用户看。
 */
import { invoke } from '@tauri-apps/api/core'
import { LIGHT_SOUND_FILES, type LightSound } from './agent-sound'

/**
 * 每段音效的加载结果：`Promise<blob 地址 | null>`。
 *
 * 缓存的是 **Promise 而不是结果**：同一段音效在第一次加载完成前被连击时，只会发一次 IPC。
 * （失败也缓存：没必要每次跳色都去撞一次读不到的文件。）
 */
const loaded = new Map<LightSound, Promise<string | null>>()

/** 每段音效复用一个 Audio：反复触发时不必重建。 */
const players = new Map<LightSound, HTMLAudioElement>()

/** 「读不到音效」只在控制台提醒一次，别把日志刷满。 */
let missingWarned = false

/**
 * 读一段音效的字节并做成 `blob:` 地址。
 *
 * @param sound 音效类别
 * @returns 可播放的地址；读不到（文件缺失 / IPC 不可用）时为 null
 */
async function loadLightSoundUrl(sound: LightSound): Promise<string | null> {
  try {
    const bytes = await invoke<ArrayBuffer>('read_light_sound', {
      name: LIGHT_SOUND_FILES[sound],
    })
    if (!bytes || bytes.byteLength === 0) {
      throw new Error('empty sound payload')
    }
    return URL.createObjectURL(new Blob([bytes], { type: 'audio/mpeg' }))
  } catch (error) {
    if (!missingWarned) {
      missingWarned = true
      console.warn(
        `[agent-status] 读不到状态灯音效（应在安装目录的 bgm/ 下放 ${LIGHT_SOUND_FILES[sound]}），已跳过播放`,
        error
      )
    }
    return null
  }
}

/** 取某段音效的地址（带缓存）。 */
function soundUrl(sound: LightSound): Promise<string | null> {
  let pending = loaded.get(sound)
  if (!pending) {
    pending = loadLightSoundUrl(sound)
    loaded.set(sound, pending)
  }
  return pending
}

/**
 * 预热三段音效：模块挂载时调一次。
 *
 * 不预热也能用，但第一盏灯跳色时要等一次 IPC + Blob 构造，声音会晚一瞬；
 * 预热把这点延迟挪到启动时（用户还看不见灯的那几百毫秒里）。
 */
export function preloadLightSounds(): void {
  for (const sound of Object.keys(LIGHT_SOUND_FILES) as LightSound[]) {
    void soundUrl(sound)
  }
}

/**
 * 播一段状态灯音效。
 *
 * 取舍：
 * - **等地址就绪再播**（而不是丢掉）：提示音的价值就在「听到了」，晚几十毫秒也不要漏；
 * - **从头重播**：同一段音效连击（多盏灯同时跳色）时重头播，而不是排队叠加；
 * - **播放失败不抛**：自动播放被系统策略拦住、音频设备忙，都只吞掉，不冒泡到状态灯逻辑。
 *
 * @param sound 要播的音效类别
 */
export function playLightSound(sound: LightSound): void {
  void soundUrl(sound).then(url => {
    if (!url) return
    let player = players.get(sound)
    if (!player) {
      player = new Audio(url)
      player.preload = 'auto'
      players.set(sound, player)
    }
    try {
      // 元数据还没到位时写 currentTime 会抛 InvalidStateError；未就绪时直接从 0 播即可。
      if (player.readyState > 0) player.currentTime = 0
    } catch {
      /* 定位失败不影响这次播放 */
    }
    void player.play().catch(() => {
      /* autoplay 被挡 / 设备忙：静默忽略 */
    })
  })
}
