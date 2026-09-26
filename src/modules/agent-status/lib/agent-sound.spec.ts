import { describe, expect, it } from 'vitest'
import type { SessionSnapshot } from './agent-status'
import {
  LIGHT_SOUND_FILES,
  lightClassOf,
  lightClassesOf,
  soundForTransition,
  transitionsBetween,
  type LightClass,
} from './agent-sound'

/** 造一盏灯；只填音效判定关心的两个字段（phase / is_flashing），其余走默认值。 */
const session = (
  key: string,
  phase: SessionSnapshot['phase'],
  isFlashing = false
): SessionSnapshot => ({
  key,
  provider: 'claude',
  session_id: key,
  phase,
  source: 'hook',
  confidence: 100,
  started_at_ms: 0,
  last_seen_at_ms: 0,
  completed_at_ms: null,
  remove_at_ms: null,
  is_flashing: isFlashing,
  pid: null,
  error_code: null,
  base_url: null,
  upstream_url: null,
  prompt: null,
  waiting_reason: null,
  tokens: null,
  last_sequence: null,
  termination_confirmed: false,
})

/** 一份「上一帧 → 这一帧」的类别表，供整帧对比用。 */
const classes = (entries: Record<string, LightClass>) => new Map(Object.entries(entries))

describe('lightClassOf', () => {
  it('maps the light colors onto sound classes', () => {
    expect(lightClassOf(session('a', 'error'))).toBe('error')
    expect(lightClassOf(session('a', 'waiting'))).toBe('warn')
    expect(lightClassOf(session('a', 'running'))).toBe('quiet')
    expect(lightClassOf(session('a', 'unknown'))).toBe('quiet')
  })

  it('treats a flashing light as its own class', () => {
    // Stop（waiting + 绿闪）与 SessionEnd（completed + 绿闪）都算「完成绿闪」。
    expect(lightClassOf(session('a', 'waiting', true))).toBe('flash')
    expect(lightClassOf(session('a', 'completed', true))).toBe('flash')
  })

  it('treats a settled completed light as quiet', () => {
    // 保留时长为 0 时 Completed 不闪：它就是绿常亮，不该出声。
    expect(lightClassOf(session('a', 'completed', false))).toBe('quiet')
  })
})

describe('lightClassesOf', () => {
  it('indexes classes by session key', () => {
    const map = lightClassesOf([
      session('claude:a', 'running'),
      session('codex:b', 'error'),
    ])
    expect(map.get('claude:a')).toBe('quiet')
    expect(map.get('codex:b')).toBe('error')
    expect(map.size).toBe(2)
  })
})

describe('soundForTransition', () => {
  it('stays silent for a light seen for the first time', () => {
    // 建档 / 重新发现不是「跳色」：新灯无论当前什么颜色都不补音效。
    for (const next of ['error', 'flash', 'warn', 'quiet'] as const) {
      expect(soundForTransition(undefined, next)).toBeNull()
    }
  })

  it('rings on red and on the completion flash', () => {
    expect(soundForTransition('quiet', 'error')).toBe('error')
    expect(soundForTransition('warn', 'error')).toBe('error')
    expect(soundForTransition('flash', 'error')).toBe('error')
    expect(soundForTransition('quiet', 'flash')).toBe('success')
    expect(soundForTransition('warn', 'flash')).toBe('success')
  })

  it('rings when the light turns yellow from green or red', () => {
    expect(soundForTransition('quiet', 'warn')).toBe('warn')
    expect(soundForTransition('error', 'warn')).toBe('warn')
  })

  it('stays silent when the completion flash settles into solid yellow', () => {
    // Stop 已在绿闪那一刻响过 goSucc；retention 到点再响 goWarn 只会像回声。
    expect(soundForTransition('flash', 'warn')).toBeNull()
  })

  it('stays silent for green and for unchanged classes', () => {
    expect(soundForTransition('error', 'quiet')).toBeNull()
    expect(soundForTransition('warn', 'quiet')).toBeNull()
    expect(soundForTransition('quiet', 'quiet')).toBeNull()
    expect(soundForTransition('warn', 'warn')).toBeNull()
    expect(soundForTransition('flash', 'flash')).toBeNull()
  })
})

describe('transitionsBetween', () => {
  it('rings once per changed light and never for new lights', () => {
    const previous = classes({ 'claude:a': 'quiet', 'codex:b': 'quiet' })
    const next = classes({ 'claude:a': 'flash', 'codex:b': 'quiet', 'codex:c': 'error' })
    // 只有 a 真的变了；c 是本帧新出现的灯。
    expect(transitionsBetween(previous, next)).toEqual(['success'])
  })

  it('dedupes simultaneous transitions and orders them red first', () => {
    const previous = classes({ a: 'quiet', b: 'quiet', c: 'quiet' })
    const next = classes({ a: 'error', b: 'flash', c: 'warn' })
    expect(transitionsBetween(previous, next)).toEqual(['error', 'success', 'warn'])
  })

  it('returns nothing when every light kept its class', () => {
    const previous = classes({ a: 'warn', b: 'quiet' })
    const next = classes({ a: 'warn', b: 'quiet' })
    expect(transitionsBetween(previous, next)).toEqual([])
  })

  it('ignores lights that disappeared', () => {
    const previous = classes({ a: 'error', b: 'quiet' })
    expect(transitionsBetween(previous, classes({ b: 'quiet' }))).toEqual([])
  })
})

describe('LIGHT_SOUND_FILES', () => {
  it('keeps the file names the user drops into src/assets/bgm', () => {
    // 跟着 `src/assets/bgm/` 里实际的文件名走：改名必须同时改这里，否则播放会静默跳过。
    expect(LIGHT_SOUND_FILES).toEqual({
      error: 'goError.mp3',
      warn: 'goWarn.mp3',
      success: 'goSucc.mp3',
    })
  })
})
