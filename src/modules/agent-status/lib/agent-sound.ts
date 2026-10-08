/**
 * 状态灯音效的**纯逻辑**：把「上一帧的灯」与「这一帧的灯」比出该播哪几段音。
 *
 * 播放端（Audio 元素、文件地址、缺文件的兜底）在同目录的 `sound-player.ts` ——
 * 逻辑与副作用分开，前者才能直接跑单测（测试环境没有真正的音频实现）。
 *
 * 三段音效与灯的对应（见 `reference/doc/rule/agent-status-hooks.md`「指示灯状态映射」）：
 * - 🔴 红灯 → `goErr`；🟡 黄灯 → `goWarn`；🟢 闪烁（完成态）→ `goSucc`。
 * - 🟢 常亮（正在执行）不发声：它是出现得最多的一态，每次都响会变成噪音。
 */
import type { SessionSnapshot } from './agent-status'

/** 三段音效：红灯 / 黄灯 / 完成绿闪。 */
export type LightSound = 'error' | 'warn' | 'success'

/**
 * 音效文件名 → 存放位置是 `src/assets/bgm/`（用户自备，缺文件时静默跳过）。
 * 文件名是这里与 `sound-player.ts` 之间的契约，改名要一起改。
 * ⚠️ 红灯那段实际落盘叫 `goError.mp3`（不是口述时的 `goErr`），改名要同步这里与 `agent-sound.spec.ts`。
 */
export const LIGHT_SOUND_FILES: Readonly<Record<LightSound, string>> = {
  error: 'goError.mp3',
  warn: 'goWarn.mp3',
  success: 'goSucc.mp3',
}

/**
 * 一盏灯在**音效意义上**的类别。
 *
 * 比对外契约 `SessionPhase` 多分出「绿闪」一档：完成态与「等 prompt」对外都是 `waiting`，
 * 但完成态要先绿闪（`is_flashing`），两者的提示音不同。绿常亮与未知态合并为 `quiet`（不发声）。
 */
export type LightClass = 'error' | 'flash' | 'warn' | 'quiet'

/**
 * 取一盏灯的音效类别（只看 `phase` 与 `is_flashing` 两个字段）。
 *
 * @param session 一帧快照里的会话
 * @returns 该会话当前的音效类别
 */
export function lightClassOf(session: SessionSnapshot): LightClass {
  if (session.phase === 'error') return 'error'
  if (session.is_flashing) return 'flash'
  if (session.phase === 'waiting') return 'warn'
  return 'quiet'
}

/**
 * 一帧快照里每盏会话的音效类别。
 *
 * @param sessions 快照里的会话列表
 * @returns `会话 key → 音效类别`（key 与快照里的 `key` 同源，灯消失即从表里消失）
 */
export function lightClassesOf(sessions: readonly SessionSnapshot[]): Map<string, LightClass> {
  return new Map(sessions.map(session => [session.key, lightClassOf(session)]))
}

/**
 * 一次「上一帧 → 这一帧」的类别变化该播哪段音；不该响的转移返回 null。
 *
 * 规则：
 * - **本帧新出现的灯**（上一帧没有这个 key）不响：那是建档 / 重新发现，不是「跳色」；
 * - 转红、转绿闪都响；
 * - 转黄只在「从绿常亮 / 红灯过来」时响 —— **从绿闪落到黄常亮不响**：
 *   `Stop` 已经在绿闪那一刻响过 `goSucc`，retention 到点再响一次 `goWarn` 只会像回声。
 *
 * @param previous 上一帧该会话的类别；`undefined` = 上一帧还没这盏灯
 * @param next 这一帧该会话的类别
 * @returns 该播的音效，不响时为 null
 */
export function soundForTransition(
  previous: LightClass | undefined,
  next: LightClass
): LightSound | null {
  if (previous === undefined || previous === next) return null
  switch (next) {
    case 'error':
      return 'error'
    case 'flash':
      return 'success'
    case 'warn':
      return previous === 'flash' ? null : 'warn'
    default:
      return null
  }
}

/**
 * 整帧对比：返回这一帧**该播的音效**（去重，按固定优先级红 → 绿闪 → 黄）。
 *
 * 一盏灯一次变化只响一声；同一帧里多盏灯同时跳色也只各响一次 ——
 * 叠着播同一段音既听不清，也没有更多信息。
 *
 * @param previous 上一帧的 `key → 类别`
 * @param next 这一帧的 `key → 类别`
 * @returns 按优先级排列的音效列表（最多三段，可能为空）
 */
export function transitionsBetween(
  previous: ReadonlyMap<string, LightClass>,
  next: ReadonlyMap<string, LightClass>
): LightSound[] {
  const fired = new Set<LightSound>()
  for (const [key, klass] of next) {
    const sound = soundForTransition(previous.get(key), klass)
    if (sound) fired.add(sound)
  }
  // 固定顺序 = 注意力优先级（红 > 完成 > 等待），与 Map 的遍历顺序无关。
  return (['error', 'success', 'warn'] as const).filter(sound => fired.has(sound))
}
