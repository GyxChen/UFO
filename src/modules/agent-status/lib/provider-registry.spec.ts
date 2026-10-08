/**
 * provider 注册表护栏。
 *
 * 锁三件事，都对应「接入新平台」真实踩过的坑：
 * 1. **注册表与 Rust `Provider` 枚举同源** —— 前端漏登记会让新平台只有首字符徽标；
 *    Rust 加了变体而前端不知道，正是早先「灯冻结在首帧」那类事故的源头。
 * 2. **未登记平台必须降级而不是被拒** —— 名称回落原始串、图标为 null，绝不冒充已登记平台。
 *    已登记平台则**深 / 浅两档各要有一份图标资源**：图标不再靠 CSS 反相，少一份就是浅色下隐形。
 * 3. 首字符徽标对任何输入都取得出一个可见字符。
 */
import { describe, expect, it } from 'vitest'
// ?raw 读 Rust 源文件：测试环境把 node:fs polyfill 掉了，走 vite 的原始文本导入最稳（见 floating-layout.spec.ts）。
import providerModelRaw from '../../../../src-tauri/src/modules/agent_status/status/model.rs?raw'
import { KNOWN_PROVIDERS, providerFallbackMark, providerIcon, providerMeta } from './provider-registry'

/** 从 Rust 源里取出 `pub enum Provider` 的变体名（小写 = 线协议里的 provider 串）。 */
function rustProviderVariants(): string[] {
  const block = /pub enum Provider\s*\{([\s\S]*?)\}/.exec(providerModelRaw)?.[1] ?? ''
  return [...block.matchAll(/^\s*([A-Z][A-Za-z0-9_]*)\s*,?\s*$/gm)].map(match => match[1].toLowerCase())
}

describe('provider 注册表', () => {
  it('与 Rust 侧 Provider 枚举逐个对应', () => {
    const rust = rustProviderVariants()
    expect(rust.length, '没解析出 Rust 的 Provider 变体，多半是枚举写法变了').toBeGreaterThan(0)
    expect([...KNOWN_PROVIDERS].sort()).toEqual([...rust].sort())
  })

  it('每个已登记平台都有可读名称，且深 / 浅两档都有图标资源', () => {
    for (const provider of KNOWN_PROVIDERS) {
      const meta = providerMeta(provider)
      expect(meta.label).toBeTruthy()
      // 缺失会退化成首字符徽标；浅色档缺资源则等于「白图标落在白底上」，灯直接看不见。
      expect(meta.icon, `${provider} 应当有 svg`).toBeTruthy()
      expect(meta.iconLight, `${provider} 应当有浅色档 svg`).toBeTruthy()
      expect(providerIcon(meta, true)).toBe(meta.icon)
      expect(providerIcon(meta, false)).toBe(meta.iconLight)
    }
  })

  it('未登记平台降级显示，且不冒充任何已登记平台', () => {
    const meta = providerMeta('gpt')
    expect(meta.label).toBe('gpt')
    expect(meta.icon).toBeNull()
    expect(providerIcon(meta, true)).toBeNull()
    expect(providerIcon(meta, false)).toBeNull()
    // 查不到是降级，不是抛错：一个都认不出的名字也要能渲染出一行。
    expect(() => providerMeta('🤖')).not.toThrow()
  })
})

describe('providerFallbackMark', () => {
  it('取首字符并大写', () => {
    expect(providerFallbackMark('gemini')).toBe('G')
    expect(providerFallbackMark('Claude Code')).toBe('C')
  })

  it('空串与纯空白回落问号，不留空徽标', () => {
    expect(providerFallbackMark('')).toBe('?')
    expect(providerFallbackMark('   ')).toBe('?')
  })

  it('按码点取字符，代理对不会被切成半个', () => {
    // `slice(0, 1)` 会返回半个代理对（渲染成乱码），展开成码点数组才拿得到整个字形。
    expect(providerFallbackMark('🤖cli')).toBe('🤖')
  })
})
