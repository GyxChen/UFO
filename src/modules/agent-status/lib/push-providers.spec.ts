/**
 * 推送服务注册表的同源性护栏。
 *
 * 锁三件事：
 * 1. **后端注册的每个 provider 都在前端有展示信息** —— 漏一行时面板会退化成「原始 id 当名字」，
 *    功能还在但文案糙，正是那种没人报 bug、只是越用越别扭的漂移。
 * 2. **官方默认地址只有一处真相**（后端 `ServerSpec::default`）—— 前端只填用户改过的值，
 *    所以绝不该在 TS 里再抄一份 `https://ntfy.sh` 之类的地址。
 * 3. **没有密钥这条路**：主题按用户要求明文存配置，provider 里再出现 `FieldSpec` / `secrets`
 *    就说明有人把凭据管理器那套悄悄加回来了。
 *
 * 手法照 `provider-registry.spec.ts`：`?raw` 读 Rust 源，正则抠出符号名再比对。
 */
import { describe, expect, it } from 'vitest'
import ntfyRaw from '../../../../src-tauri/src/modules/agent_status/push/providers/ntfy.rs?raw'
import barkRaw from '../../../../src-tauri/src/modules/agent_status/push/providers/bark.rs?raw'
import registryRaw from '../../../../src-tauri/src/modules/agent_status/push/mod.rs?raw'
import { KNOWN_PUSH_PROVIDERS, pushProviderMeta } from './push-providers'

/** 从 provider 源文件里抠 `fn id()` 返回的字符串。 */
function rustProviderId(raw: string): string {
  const matched = /fn id\(&self\) -> &'static str \{\s*"([^"]+)"/.exec(raw)
  expect(matched, '没解析出 provider id，多半是签名写法变了').not.toBeNull()
  return matched![1]
}

/** 抠 `fn server()` 里声明的默认地址与说明（地址通常写成 `DEFAULT_SERVER` 常量，要跟着解引用）。 */
function rustServerSpec(raw: string): { default: string; help: string } {
  const block = /fn server\(&self\) -> ServerSpec \{([\s\S]*?)\n    \}/.exec(raw)?.[1] ?? ''
  expect(block, '没解析出 ServerSpec，多半是签名写法变了').not.toBe('')
  const read = (field: string): string => {
    const literal = new RegExp(`${field}:\\s*"([^"]+)"`).exec(block)?.[1]
    if (literal !== undefined) return literal
    // 常量写法：`default: DEFAULT_SERVER` → 回源文件里读那条 `const`。
    const name = new RegExp(`${field}:\\s*(\\w+)`).exec(block)?.[1]
    return (name && new RegExp(`const ${name}: &str = "([^"]+)"`).exec(raw)?.[1]) ?? ''
  }
  return { default: read('default'), help: read('help') }
}

/** `build_providers()` 里注册的结构体名 → 源文件里的真实 id。 */
function registeredIds(): string[] {
  const block = /pub fn build_providers\(\)[\s\S]*?\n\}/.exec(registryRaw)?.[0] ?? ''
  const structs = [...block.matchAll(/Arc::new\((\w+Provider)\)/g)].map(match => match[1])
  expect(structs.length, '没解析出任何 provider，多半是函数写法变了').toBeGreaterThan(0)
  const sources: Record<string, string> = { NtfyProvider: ntfyRaw, BarkProvider: barkRaw }
  return structs.map(name => {
    const source = sources[name]
    expect(source, `${name} 的源文件没被本测试读到`).toBeTruthy()
    return rustProviderId(source!)
  })
}

describe('推送 provider 注册表', () => {
  it('与 Rust build_providers() 逐个对应', () => {
    const rust = registeredIds()
    expect([...KNOWN_PUSH_PROVIDERS].sort()).toEqual([...rust].sort())
  })

  it('每个已登记服务都有可读名称、一句说明与一个主题量词', () => {
    for (const id of KNOWN_PUSH_PROVIDERS) {
      const meta = pushProviderMeta(id)
      expect(meta.label, id).toBeTruthy()
      expect(meta.blurb.length, `${id} 应当有一句说明`).toBeGreaterThan(0)
      // 量词决定「N 个主题 / N 个设备密钥」与添加按钮的文案，缺了会露出英文 id。
      expect(meta.topicNoun, `${id} 应当有量词（主题 / 设备密钥）`).toBeTruthy()
    }
  })

  it('未登记服务降级显示而不是报错 —— 后端加服务、前端漏一行时面板仍可用', () => {
    const meta = pushProviderMeta('serverchan')
    expect(meta.label).toBe('serverchan')
    expect(meta.topicNoun).toBeTruthy()
    expect(() => pushProviderMeta('')).not.toThrow()
  })

  it('官方默认地址写在后端，前端不得再抄一份', () => {
    // 面板的地址栏靠 placeholder 显示默认值；这里若出现前端硬编码就等于有两处真相。
    expect(rustServerSpec(ntfyRaw).default).toBe('https://ntfy.sh')
    expect(rustServerSpec(barkRaw).default).toBe('https://api.day.app')
    expect(rustServerSpec(barkRaw).help).not.toBe('')
    const registryTs = JSON.stringify(KNOWN_PUSH_PROVIDERS)
    expect(registryTs).not.toContain('https://')
  })

  it('provider 里不再出现字段规格表与密钥路径', () => {
    // 主题 = 明文列；把 `FieldSpec` / `secrets` / Bearer 头加回来就等于把删掉的抽象复活了。
    // （ntfy 自己的测试里允许出现「不该带 Authorization」这条断言，所以这里查的是真正发出去的东西。）
    for (const raw of [ntfyRaw, barkRaw]) {
      expect(raw).not.toMatch(/FieldSpec/)
      expect(raw).not.toMatch(/secrets/)
      expect(raw).not.toMatch(/Bearer/)
    }
  })
})
