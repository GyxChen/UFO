/**
 * 插件命令的三段同名护栏：前端调用名 ↔ Rust 注册 ↔ permissions/capability。
 *
 * 为什么值得单独一条：这条链上任何一段对不上，**构建、类型检查、其余单测全绿**，
 * 坏法发生在运行时 —— 少了 `commands.allow` 那一行就是「命令存在但这道窗没权限」，
 * 界面表现为某个能力一调就失败（本项目已经因为 identifier 撞名踩过同一类静默失权）。
 *
 * 三段判据：
 * 1. Rust `generate_handler!` 里登记过的插件命令，必须出现在某个 `permissions/*.toml` 的授权表里；
 * 2. 反过来也要成立：授权表里的插件命令必须在 Rust 注册过（写了个不存在的命令，tauri 构建期就炸，
 *    但这条断言能在纯前端单测里提前抓到，不必等打包）；
 * 3. 前端 `invoke('…')` 用到的每个插件命令名必须既注册又授权（拼错一个字母 = 运行时没这条命令）。
 * 外加：带着插件命令的 permission identifier 必须被某个 capability 引用 ——
 * 没被任何窗口引用的 permission 等于不存在。
 *
 * ⚠️ 这条护栏**能**证的是四段名字集合一致（Rust 注册 / toml 授权 / capability 引用 / 前端调用），
 * 加上「写命令没漏给含悬浮窗与提示窗的那份 capability」。
 * 它**不能**证：命令本身做得对不对，也不能证某份 capability 的 `windows` 列的是不是真那几个窗口
 * —— 窗口名与声明的同源由 `src/windows/panel-windows.spec.ts` 那一档负责。
 */

import { describe, expect, it } from 'vitest'
import libRaw from '../../src-tauri/src/lib.rs?raw'

const tomlFiles = import.meta.glob('../../src-tauri/permissions/*.toml', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>

const capabilityFiles = import.meta.glob('../../src-tauri/capabilities/*.json', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>

const frontendFiles = {
  ...import.meta.glob('../lib/**/*.ts', { query: '?raw', import: 'default', eager: true }),
  ...import.meta.glob('../stores/*.ts', { query: '?raw', import: 'default', eager: true }),
  ...import.meta.glob('../components/**/*.vue', { query: '?raw', import: 'default', eager: true }),
  ...import.meta.glob('../windows/*.ts', { query: '?raw', import: 'default', eager: true }),
} as Record<string, string>

/** `?raw` 在 Windows 上拿到的可能是 CRLF，先归一（否则逐行判据会整段失配）。 */
function textOf(raw: string): string {
  return raw.replace(/\r\n/g, '\n')
}

/** 凡名字里带 plugin 的标识符就算插件命令（这套命令的命名一直是这个形状）。 */
function isPluginCommand(name: string): boolean {
  return name.includes('plugin')
}

/** Rust 的 `generate_handler![…]` 里登记的命令名。 */
function registeredCommands(): Set<string> {
  const source = textOf(libRaw)
  const start = source.indexOf('generate_handler![')
  expect(start, 'lib.rs 里找不到 generate_handler!').toBeGreaterThan(-1)
  const block = source.slice(start).split('];')[0]
  const names = new Set<string>()
  for (const matched of block.matchAll(/([a-z][a-z0-9_]*)\s*,/g)) {
    const name = matched[1]
    if (name && isPluginCommand(name)) names.add(name)
  }
  return names
}

/** `permissions/*.toml` 里所有 `commands.allow` 授权过的命令名。 */
function allowedCommands(): Set<string> {
  const names = new Set<string>()
  for (const raw of Object.values(tomlFiles)) {
    for (const matched of textOf(raw).matchAll(/commands\.allow\s*=\s*\[([\s\S]*?)\]/g)) {
      for (const quoted of (matched[1] ?? '').matchAll(/"([a-z0-9_]+)"/g)) {
        const name = quoted[1]
        if (name && isPluginCommand(name)) names.add(name)
      }
    }
  }
  return names
}

/** 每条 permission 的 identifier → 它授权的插件命令。 */
function pluginPermissions(): { identifier: string; commands: string[] }[] {
  const out: { identifier: string; commands: string[] }[] = []
  for (const raw of Object.values(tomlFiles)) {
    for (const matched of textOf(raw).matchAll(/\[\[permission\]\]([\s\S]*?)(?=\[\[permission\]\]|$)/g)) {
      const body = matched[1] ?? ''
      const identifier = /identifier\s*=\s*"([^"]+)"/.exec(body)?.[1]
      const allow = /commands\.allow\s*=\s*\[([\s\S]*?)\]/.exec(body)?.[1] ?? ''
      const commands = [...allow.matchAll(/"([a-z0-9_]+)"/g)]
        .map(item => item[1])
        .filter((name): name is string => !!name && isPluginCommand(name))
      if (identifier && commands.length > 0) out.push({ identifier, commands })
    }
  }
  return out
}

/** capability 里引用过的 permission identifier。 */
function grantedIdentifiers(): Set<string> {
  const ids = new Set<string>()
  for (const raw of Object.values(capabilityFiles)) {
    for (const matched of textOf(raw).matchAll(/"([a-z0-9_-]+)"/g)) {
      const value = matched[1]
      if (value?.startsWith('allow-')) ids.add(value)
    }
  }
  return ids
}

/**
 * 前端实际调用到的插件命令名。
 *
 * 只认两种写法的第一个实参：`invoke('name'` / `invoke<T>('name'`，以及 store 里那层
 * `write('name', …)` 的公共尾巴 —— 写命令全走它，直接找 `invoke` 会漏掉安装与卸载。
 * 测试夹具（`*.spec.ts`）不参与：它们里的假命令名不是产品事实。
 */
function invokedCommands(): Set<string> {
  const names = new Set<string>()
  for (const [path, raw] of Object.entries(frontendFiles)) {
    if (path.includes('.spec.')) continue
    const source = textOf(raw)
    for (const matched of source.matchAll(/invoke(?:<[^>]*>)?\(\s*'([a-z0-9_]+)'/g)) {
      const name = matched[1]
      if (name && isPluginCommand(name)) names.add(name)
    }
    for (const matched of source.matchAll(/\bwrite\(\s*'([a-z0-9_]+)'/g)) {
      const name = matched[1]
      if (name && isPluginCommand(name)) names.add(name)
    }
  }
  return names
}

const sorted = (values: Iterable<string>) => [...values].sort()

describe('插件命令的授权链同源', () => {
  const registered = registeredCommands()
  const allowed = allowedCommands()
  const invoked = invokedCommands()

  it('Rust 注册的插件命令全部被某条 permission 授权', () => {
    const missing = sorted([...registered].filter(name => !allowed.has(name)))
    expect(missing, '这些命令没写进 permissions 的 commands.allow：运行时「没权限」而构建全绿').toEqual([])
  })

  it('permissions 授权的插件命令都在 Rust 注册过', () => {
    const unknown = sorted([...allowed].filter(name => !registered.has(name)))
    expect(unknown, 'permissions 里点了不存在的命令名：ACL 与实现漂移').toEqual([])
  })

  it('前端调用到的每个插件命令既注册又授权', () => {
    const notRegistered = sorted([...invoked].filter(name => !registered.has(name)))
    expect(notRegistered, 'invoke 的名字在 Rust 没有对应命令（拼错一个字母就是运行时失败）').toEqual([])
    const notAllowed = sorted([...invoked].filter(name => !allowed.has(name)))
    expect(notAllowed, 'invoke 的名字没有任何 permission 授权：那道窗调不动它').toEqual([])
  })

  it('带插件命令的 permission 都被某个 capability 引用', () => {
    const granted = grantedIdentifiers()
    const orphan = pluginPermissions()
      .filter(item => !granted.has(item.identifier))
      .map(item => item.identifier)
    expect(sorted(orphan), '没被任何窗口引用的 permission 等于不存在').toEqual([])
  })

  /**
   * 写命令只许待在设置侧那一份 capability 里（AGENTS.md 的「读给所有窗口、写只给设置侧」）。
   *
   * 写侧命令集合不从名字猜：它就是前端 `write('…')` 那条公共尾巴用到的命令名。
   * 「所有窗口」那份 = windows 里含 `floating` 或 `tooltip` 的 capability —— 组件包的 UI
   * 就跑在这两扇窗里，它只要能调自己那套能力，不能装包、不能改安装记录。
   */
  it('写命令没有漏给含悬浮窗 / 提示窗的那份 capability', () => {
    const writes = new Set<string>()
    for (const [path, raw] of Object.entries(frontendFiles)) {
      if (path.includes('.spec.')) continue
      for (const matched of textOf(raw).matchAll(/\bwrite\(\s*'([a-z0-9_]+)'/g)) {
        const name = matched[1]
        if (name && isPluginCommand(name)) writes.add(name)
      }
    }
    expect(writes.size, '一条写命令都没抓到 = 判据空转').toBeGreaterThan(3)

    const commandsByIdentifier = new Map<string, string[]>()
    for (const item of pluginPermissions()) commandsByIdentifier.set(item.identifier, item.commands)

    for (const [file, raw] of Object.entries(capabilityFiles)) {
      const source = textOf(raw)
      const windows = /"windows"\s*:\s*\[([\s\S]*?)\]/.exec(source)?.[1] ?? ''
      const mentionsAllWindows = windows.includes('"floating"') || windows.includes('"tooltip"')
      if (!mentionsAllWindows) continue
      const granted: string[] = []
      for (const matched of source.matchAll(/"(allow-[a-z0-9_-]+)"/g)) {
        const identifier = matched[1]
        if (!identifier) continue
        granted.push(...(commandsByIdentifier.get(identifier) ?? []))
      }
      const leaked = sorted(granted.filter(name => writes.has(name)))
      expect(
        leaked,
        `${file} 覆盖悬浮窗 / 提示窗，却把设置侧的写命令授了出去（组件包能从那儿装包、改安装记录）`
      ).toEqual([])
    }
  })

  /** 反向钉住调用面：别让「插件命令一个都没被抓到」变成这条护栏的空转。 */
  it('护栏真的抓到了调用面（不是空集合自证）', () => {
    expect(invoked.size, '一个插件命令都没扫到 —— glob 或调用写法变了').toBeGreaterThan(20)
    expect(registered.size).toBeGreaterThan(20)
    // 安装记录读写与两条外设命令都该在里面，缺一个就是判据退化。
    for (const name of ['get_plugins', 'install_plugin', 'plugin_ble_scan', 'plugin_serial_open']) {
      expect(registered.has(name), `Rust 注册表里少了 ${name}`).toBe(true)
    }
  })
})
