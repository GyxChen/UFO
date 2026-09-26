/**
 * 推送纯逻辑的单元测试：配置归一化、地址校验、快照守卫。
 *
 * 锁的都是「后端还没回话」或「配置被手改坏」时的表现 —— 这两种情况下界面也必须能正常用，
 * 且绝不能把一个坏字段扩散成整块设置失效。
 */
import { describe, expect, it } from 'vitest'
import {
  DEFAULT_PUSH_CONFIG,
  isPushSnapshot,
  newChannelId,
  parsePush,
  pushPatch,
  pushSettings,
  validateServer,
  validateTopic,
} from './push'

describe('parsePush', () => {
  it('缺块与类型不对都回落默认，且默认是关的', () => {
    for (const raw of [undefined, null, 'nope', 42, []]) {
      const config = parsePush(raw)
      expect(config.enabled).toBe(false)
      expect(config.events).toEqual(DEFAULT_PUSH_CONFIG.events)
      expect(config.channels).toEqual([])
      expect(config.providers).toEqual({})
    }
  })

  it('一个坏字段只回落它自己', () => {
    const config = parsePush({
      enabled: true,
      events: 'failed',
      content: ['prompt'],
      providers: 'ntfy',
    })
    expect(config.enabled).toBe(true)
    expect(config.events).toEqual(DEFAULT_PUSH_CONFIG.events)
    expect(config.content).toEqual(['prompt'])
    expect(config.providers).toEqual({})
  })

  it('空事件数组是用户的明确意图，不回落默认', () => {
    expect(parsePush({ events: [] }).events).toEqual([])
    expect(parsePush({ content: [] }).content).toEqual([])
  })

  it('未登记的事件 / 内容项被丢掉（后端加了枚举而前端注册表还没跟上时不该崩）', () => {
    expect(parsePush({ events: ['nope', 'failed', 7] }).events).toEqual(['failed'])
    expect(parsePush({ content: ['endpoint', 'nope', 'session_id'] }).content).toEqual([
      'endpoint',
      'session_id',
    ])
  })

  it('提醒内容默认全不勾', () => {
    // 出网内容必须用户显式选：默认一个都不带（ntfy 主题公开可订阅）。
    expect(parsePush({}).content).toEqual([])
  })

  it('被删掉的功能不再被读出来（节流 / 静默 / 标题前缀 / 收件方开关）', () => {
    const config = parsePush({
      throttle_sec: 30,
      quiet: { enabled: true, start: '23:00', end: '07:30' },
      title_prefix: 'X',
      channels: [{ id: 'c1', provider: 'ntfy', enabled: false, topic: 'ufo' }],
    }) as Record<string, unknown>
    expect(config).not.toHaveProperty('throttle_sec')
    expect(config).not.toHaveProperty('quiet')
    expect(config).not.toHaveProperty('title_prefix')
    expect(config.channels).not.toHaveProperty('0.enabled')
  })

  it('缺 id / 缺 provider 的主题行整行丢掉', () => {
    const config = parsePush({
      channels: [{ id: 'c1', provider: 'bark' }, { provider: 'bark' }, { id: 'c3' }, null, 'nope'],
    })
    expect(config.channels).toHaveLength(1)
    expect(config.channels[0].id).toBe('c1')
  })

  it('没起名字的行回落服务名，列表左侧不能是空的', () => {
    const config = parsePush({
      channels: [
        { id: 'c1', provider: 'ntfy', name: '   ', topic: 'ufo' },
        { id: 'c2', provider: 'bark', name: 'iPhone', topic: 'k9' },
      ],
    })
    expect(config.channels[0].name).toBe('ntfy')
    expect(config.channels[1].name).toBe('iPhone')
  })

  it('旧配置里的 fields.server / fields.topic 仍然读得出来', () => {
    // 地址与主题当年存在通用 `fields` map 里；漏了这条回落会**静默丢光**用户已有的 ntfy 主题。
    const config = parsePush({
      providers: { ntfy: { enabled: true, fields: { server: 'http://192.168.1.20:8080' } } },
      channels: [{ id: 'c1', provider: 'ntfy', name: '家里', fields: { topic: 'ufo' } }],
    })
    expect(config.providers.ntfy.server).toBe('http://192.168.1.20:8080')
    expect(config.channels[0].topic).toBe('ufo')
    // 新键优先于旧键（用户改过就不该被历史值盖回去）。
    const newer = parsePush({
      channels: [{ id: 'c1', provider: 'ntfy', topic: 'new', fields: { topic: 'old' } }],
    })
    expect(newer.channels[0].topic).toBe('new')
  })

  it('主题串与地址都被按字符裁剪', () => {
    const config = parsePush({
      providers: { ntfy: { enabled: true, server: `  https://x.example/${'h'.repeat(5000)}  ` } },
      channels: [{ id: 'c1', provider: 'ntfy', name: '汉'.repeat(60), topic: 't'.repeat(500) }],
    })
    expect([...config.providers.ntfy.server].length).toBeLessThanOrEqual(4096)
    expect(config.providers.ntfy.server.startsWith('https://')).toBe(true)
    expect([...config.channels[0].name].length).toBe(48)
    expect(config.channels[0].topic).toHaveLength(256)
  })
})

describe('pushSettings', () => {
  it('从外壳配置块里取 push 子块', () => {
    const config = pushSettings({
      module_settings: {
        'agent-status': { push: { enabled: true, providers: { ntfy: { enabled: true, server: 'https://ntfy.example.com' } } } },
      },
    } as never)
    expect(config.enabled).toBe(true)
    expect(config.providers.ntfy.server).toBe('https://ntfy.example.com')
  })

  it('完全没有配置时也返回一份可用默认值', () => {
    expect(pushSettings(null).events).toEqual(DEFAULT_PUSH_CONFIG.events)
  })
})

describe('validateServer', () => {
  it('留空算合法（含义是「用官方默认地址」）', () => {
    expect(validateServer('')).toBeNull()
    expect(validateServer('   ')).toBeNull()
  })

  it('必须带 http(s) scheme，且不能夹空白', () => {
    expect(validateServer('ntfy.example.com')).toContain('http')
    expect(validateServer('https://a b.example.com')).toContain('非法字符')
    expect(validateServer('https://ntfy.example.com')).toBeNull()
  })

  it('超长要拦（按字符数，中文不算三个）', () => {
    // `https://` 占 8 个字符，所以 4088 个汉字刚好是上限 4096：按字符算而不是按字节，
    // 按字节会把中文当成三倍长而误拦。
    expect(validateServer(`https://${'汉'.repeat(4088)}`)).toBeNull()
    expect(validateServer(`https://${'汉'.repeat(4090)}`)).toContain('超过')
  })
})

describe('validateTopic', () => {
  it('唯一的硬性要求是非空', () => {
    expect(validateTopic('')).toMatch(/不能为空/)
    expect(validateTopic('   ')).toMatch(/不能为空/)
    expect(validateTopic('ufo-gyx-home')).toBeNull()
  })
})

describe('newChannelId', () => {
  it('形状稳定且每次不同', () => {
    const first = newChannelId()
    expect(first).toMatch(/^ch_[0-9a-f]{8}$/)
    expect(new Set([first, newChannelId(), newChannelId()]).size).toBe(3)
  })
})

describe('isPushSnapshot', () => {
  /** 一份形状正确的快照。 */
  function snapshot(patch: Record<string, unknown> = {}) {
    return {
      config: {
        enabled: false,
        events: [],
        content: [],
        providers: {},
        channels: [],
      },
      providers: [],
      channels: [],
      ...patch,
    }
  }

  it('接受形状正确的载荷，包括后端新增的未知 provider', () => {
    // ⚠️ 不按 provider 白名单收窄：那样会让新平台的整帧被丢弃，面板停在首帧。
    expect(
      isPushSnapshot(
        snapshot({
          providers: [{ id: 'serverchan', label: 'Server酱', enabled: false, server: '', spec: { label: '服务器', default: 'https://sctapi.ftqq.com', help: '' } }],
        })
      )
    ).toBe(true)
  })

  it('拒绝缺字段与类型不对的帧', () => {
    expect(isPushSnapshot(null)).toBe(false)
    expect(isPushSnapshot('nope')).toBe(false)
    expect(isPushSnapshot(snapshot({ channels: undefined }))).toBe(false)
    expect(isPushSnapshot(snapshot({ providers: {} }))).toBe(false)
    expect(isPushSnapshot(snapshot({ config: {} }))).toBe(false)
  })
})

describe('pushPatch', () => {
  it('写回前裁掉名字与主题的多余字符，并给空名称兜一个可读值', () => {
    const patch = pushPatch({
      ...DEFAULT_PUSH_CONFIG,
      providers: { ntfy: { enabled: true, server: '  https://ntfy.example.com  ' } },
      channels: [
        { id: 'c1', provider: 'ntfy', name: '   ', topic: ' ufo ' },
        { id: 'c2', provider: 'bark', name: '汉'.repeat(60), topic: 'k'.repeat(500) },
      ],
    })
    expect(patch.push.providers.ntfy.server).toBe('https://ntfy.example.com')
    expect(patch.push.channels[0].name).toBe('ntfy')
    expect(patch.push.channels[0].topic).toBe('ufo')
    expect([...patch.push.channels[1].name].length).toBe(48)
    expect(patch.push.channels[1].topic).toHaveLength(256)
  })

  it('整块写回：被删掉的旧字段不会残留在配置里', () => {
    // 外壳按浅合并整块替换 `push`，所以旧键（节流 / 静默 / 前缀）在第一次保存时自然消失。
    expect(Object.keys(pushPatch(DEFAULT_PUSH_CONFIG).push).sort()).toEqual([
      'channels',
      'content',
      'enabled',
      'events',
      'providers',
    ])
  })
})
