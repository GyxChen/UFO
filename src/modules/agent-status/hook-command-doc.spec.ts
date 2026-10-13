import { describe, expect, it } from 'vitest'
import confRaw from '../../../src-tauri/tauri.conf.json?raw'
import hooksDocRaw from '../../../reference/doc/rule/agent-status-hooks.md?raw'
import onboardRaw from '../../../reference/doc/guide/agent-status-platform-onboarding-guide.md?raw'
import readmeRaw from '../../../README.md?raw'
import wxsRaw from '../../../src-tauri/windows/wix/path-fragment.wxs?raw'
import nshRaw from '../../../src-tauri/windows/nsis/path-hooks.nsh?raw'

/**
 * 「文档里让抄的 hook 命令名」与「装机后真实存在的可执行文件名」必须同源。
 *
 * 这条不变量坏掉的形态很要命：不是编译不过、不是测试先红，而是**用户照文档配完 hook，
 * 灯永远不亮** —— 因为 `ufo` 这个命令既来自 `mainBinaryName`（决定 `tauri build` 把产物
 * 改名成什么），又来自安装器追加进用户 PATH 的那个目录，两处都不在 CI 的常规断言面上。
 * 所以这里同时卡三个环节：命令名 ↔ mainBinaryName、配置 ↔ 两个安装器文件、
 * 以及安装器文件里那几个「默认值是数据丢失」的关键属性。
 */

/**
 * 归一行尾并剥掉 BOM。
 *
 * makensis 要求脚本是带 BOM 的 UTF-8（中文注释在无 BOM 时直接 `Bad text encoding`），
 * 而 `?raw` 会把 BOM 一起读进来，留着它就会破坏锚在行首的匹配。
 */
/** BOM 用码位取，不写字面量：那份文本里的 BOM 看不见，抄进断言只会变成一条静默失配的正则。 */
const BOM = String.fromCharCode(0xfeff)
const norm = (text: string) => text.replace(/\r\n/g, '\n').replace(BOM, '')

/**
 * 剥掉 XML 注释，只留代码。
 *
 * 不剥的话这两个方向都会出错：`$(var.Win64)` 在注释里被解释「为什么不能用」，
 * 负向断言就把自己的说明文本判成违规；反过来，关键属性只写在注释里也能骗过正向断言。
 */
const xmlCode = (text: string) => text.replace(/<!--[\s\S]*?-->/g, '')

/**
 * 剥掉 NSIS 注释：整行 `;` 开头，以及行内 ` ;` 之后的部分。
 *
 * 取「空格 + 分号」而不是「首个分号」，是因为被拼接的 PATH 串里就有分号
 * （`StrCpy $R1 "$R9;$R8"`），从首个分号切会把代码一起切掉。
 */
const nshCode = (text: string) => text.replace(/^[ \t]*;.*$/gm, '').replace(/ ;.*$/gm, '')

const conf = JSON.parse(norm(confRaw)) as {
  mainBinaryName?: string
  bundle?: {
    windows?: {
      wix?: { upgradeCode?: string; fragmentPaths?: string[]; componentRefs?: string[] }
      nsis?: { installerHooks?: string; installMode?: string }
    }
  }
}

const wxs = xmlCode(norm(wxsRaw))
const nsh = nshCode(norm(nshRaw))

/**
 * 抽出文档里所有 `"command": "…"` 的值。
 *
 * 只挑带 `--status-hook` 的那些当作「本 App 的 hook」：文档里还有一条举例用的
 * `curl.exe …`，把它算进来就会永远对不上命令名。解析出 0 条直接抛 ——
 * 正则漂了要响，不能变成「一条都没匹配上所以全都相等」的假绿。
 */
function hookCommands(text: string, label: string): string[] {
  const all = [...text.matchAll(/"command":\s*"([^"]*)"/g)].map((m) => m[1])
  if (all.length === 0) throw new Error(`${label} 里一条 "command" 都没解析出来，格式或正则漂了`)
  return all.filter((command) => command.includes('--status-hook'))
}

/**
 * 抽出**散文式**提及（README 与文档正文里的 `ufo --status-hook …`，不是 JSON 键值）。
 *
 * 前置断言里排掉 `$`：自测脚本写的是 `$exe --status-hook`（开发期绝对路径变量），
 * 那个 token 本来就不该等于安装名，混进来会变成假红。解析出 0 条同样抛。
 */
function proseHookCommands(text: string, label: string): string[] {
  const found = [...text.matchAll(/(?<![$\w.-])([A-Za-z0-9_.-]+)\s+--status-hook/g)].map((m) => m[1])
  if (found.length === 0) throw new Error(`${label} 里一条 --status-hook 都没解析出来，写法漂了`)
  return found
}

/** 命令的第一个 token 就是要被 PATH 解析的可执行文件名。 */
const commandName = (command: string) => command.trim().split(/\s+/)[0].toLowerCase()

/** Windows 按名查找大小写不敏感，所以全比对小写形；写成严格相等会在 `Ufo` 这种改名上假红。 */
const binary = (conf.mainBinaryName ?? '').toLowerCase()

describe('hook 命令名与 mainBinaryName 同源', () => {
  it('三份材料里每条 hook 命令用的都是构建产物那个名字', () => {
    // README 只给散文式的提醒（不给配置样例，免得又多一处会漂的抄本），所以只过散文扫描器。
    const fromJson = [...hookCommands(norm(hooksDocRaw), 'agent-status-hooks.md'),
      ...hookCommands(norm(onboardRaw), 'agent-status-platform-onboarding-guide.md')]
    const fromProse = [...proseHookCommands(norm(hooksDocRaw), 'agent-status-hooks.md'),
      ...proseHookCommands(norm(onboardRaw), 'agent-status-platform-onboarding-guide.md'),
      ...proseHookCommands(norm(readmeRaw), 'README.md')]
    // 命令名一旦与 mainBinaryName 分叉，照抄的人配出来的就是「装了但灯不亮」。
    const names = [...new Set([...fromJson, ...fromProse].map(commandName))].sort()
    expect(names, '文档 / README 里的 hook 命令名集合与 mainBinaryName 不一致').toEqual([binary])
    // 至少各留一条：整节被删光时上面的集合会变成空数组，不该被当成「一致」。
    expect(fromJson.length, '带 --status-hook 的 "command" 少于 3 条，八成是正文被整段删了').toBeGreaterThanOrEqual(3)
    expect(fromProse.length, '散文式的 --status-hook 提及为 0，README 那条提醒可能被改写了').toBeGreaterThanOrEqual(1)
  })

  it('mainBinaryName 存在且不带扩展名（schema 规定由 CLI 自己补 .exe）', () => {
    expect(binary, 'tauri.conf.json 根级必须有 mainBinaryName').not.toBe('')
    expect(binary).not.toContain('.exe')
  })

  it('两条打包通道都挂上了 PATH 那段，且引用的组件 id 与 fragment 声明相等', () => {
    const windows = conf.bundle?.windows
    expect(windows?.nsis?.installerHooks, 'nsis.installerHooks 没指向本仓库那份 .nsh').toBe('windows/nsis/path-hooks.nsh')
    expect(windows?.wix?.fragmentPaths, 'wix.fragmentPaths 没指向本仓库那份 .wxs').toEqual(['windows/wix/path-fragment.wxs'])
    // 两侧各自独立解析：配置里的 componentRefs 改了、或 fragment 里的组件改名了，都会红。
    const declared = [...wxs.matchAll(/<Component Id="([^"]+)"/g)].map((m) => m[1]).sort()
    expect(declared, 'fragment 应当正好声明一个组件').toHaveLength(1)
    expect([...(windows?.wix?.componentRefs ?? [])].sort(), 'componentRefs 与 fragment 声明的组件不一致').toEqual(declared)
  })

  it('WiX fragment 的四个关键属性没被改坏（其中一个是「默认值即数据丢失」）', () => {
    // 目录 id 是 Tauri 模板里的 INSTALLDIR；写成 INSTALLFOLDER 时 candle 不报，
    // 要到 light 的引用完整性校验才炸，所以在这里先卡住。
    expect(wxs, 'DirectoryRef 的 id 漂了').toContain('<DirectoryRef Id="INSTALLDIR">')
    expect(wxs).toMatch(/<Environment[^>]+Name="PATH"/)
    // Part 缺省是 all = 整串替换 = 抹掉用户 PATH，必须显式 last。
    expect(wxs, 'Environment 必须 Part="last"，缺省 all 会覆盖整个用户 PATH').toMatch(/<Environment[^>]+Part="last"/)
    expect(wxs, 'Permanent="no" 才能卸载时撤销').toMatch(/<Environment[^>]+Permanent="no"/)
    expect(wxs, 'System="no"：NSIS 那条通道没提权，只有 HKCU 写得动').toMatch(/<Environment[^>]+System="no"/)
    // <Environment> 当不了 key path，组件必须另有 RegistryValue KeyPath="yes"。
    expect(wxs, '组件必须有 key path，否则 light 阶段报 does not have an explicit key path').toMatch(/KeyPath="yes"/)
    expect(wxs, 'fragment 取不到 main.wxs 的文件级 define，写了 candle 立刻报错').not.toMatch(/\$\(var\.Win64\)/)
  })

  it('NSIS 钩子的时机、写法与禁区', () => {
    const macros = [...nsh.matchAll(/^!macro\s+(NSIS_HOOK_\w+)/gm)].map((m) => m[1]).sort()
    // POSTUNINSTALL 而不是 PREUNINSTALL：后者插在 CheckIfAppIsRunning 之前，
    // 卸载被中止时会出现「PATH 摘了、应用还在」。
    expect(macros, '只应定义 POSTINSTALL 与 POSTUNINSTALL 两个钩子').toEqual(['NSIS_HOOK_POSTINSTALL', 'NSIS_HOOK_POSTUNINSTALL'])
    expect(nsh, '卸载侧必须被 $UpdateMode 守卫，否则静默更新会先把 PATH 摘掉').toMatch(/\$\{If\}\s+\$UpdateMode\s+<>\s+1/)
    expect(nsh, 'PATH 必须按 Expand 写回，否则把 %NVM_HOME% 这类间接引用固化成死路径')
      .toMatch(/WriteRegExpandStr\s+\$\{UFO_ENV_ROOT\}\s+\$\{UFO_ENV_SUBKEY\}\s+"Path"/)
    // 两个 define 是探针能在 scratch 子键上跑同一份算法的前提。
    expect(nsh, '环境变量落点必须可覆盖，否则 PATH 算法只能在真人机器上验').toContain('!ifndef UFO_ENV_SUBKEY')
    expect(nsh.toLowerCase(), 'setx 会把 PATH 截到 1024 字符再写回，是禁区').not.toContain('setx')
  })

  it('upgradeCode 已钉住、installMode 已钉住', () => {
    // 默认 upgradeCode 由 productName 派生：以后有人改 productName 就会静默换码，
    // Windows 把新版本当成另一个应用装第二遍，留下两条 PATH。
    expect(conf.bundle?.windows?.wix?.upgradeCode, 'wix.upgradeCode 必须钉死成字面 UUID').toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
    )
    // 改成 perMachine / both 会让安装器提权，HKCU 落到执行安装那个账户的 hive。
    expect(conf.bundle?.windows?.nsis?.installMode, 'nsis.installMode 必须钉死 currentUser').toBe('currentUser')
  })
})
