/**
 * 作者侧模板与宿主契约的同源护栏。
 *
 * 模板是「照着做就能装上」的那份样本，它一旦与契约漂移，坏法是**作者照文档做完却装不上**
 * （或装上后一片空白），而报错发生在宿主的校验里，作者先怀疑自己。四条都钉住：
 * - API / 清单版本：宿主 `paths.rs` 是唯一来源，模板抄错版本 → 直接拒装；
 * - 入口文件名：模板的 `plugin.json` 与 `vite.config.mjs` 的产出必须同名；
 * - 挂载点与能力名：写一个没登记的名字 = 拒装，写一个还没实装的名字 = 一用就抛；
 * - id 与相对路径：过宿主同一套判据。
 */

import { describe, expect, it } from 'vitest'
import pathsRaw from '../../src-tauri/src/plugin/paths.rs?raw'
import viteConfigRaw from '../../reference/plugin-template/vite.config.mjs?raw'
import indexRaw from '../../reference/plugin-template/src/index.js?raw'
import settingsRaw from '../../reference/plugin-template/src/SettingsPanel.vue?raw'
import manifestRaw from '../../reference/plugin-template/public/plugin.json?raw'
import { PLUGIN_LIMITS, PLUGIN_ID_PATTERN, isSafePluginRelPath } from './plugin-limits'
import { PLUGIN_PERMISSIONS, PLUGIN_SURFACES } from './plugin-capabilities'
import { declaredSurfaceIds, type PluginManifestJson } from './plugin-types'

/** 模板清单（结构由 Rust 校验，这里只读字段做同源比对）。 */
const manifest = JSON.parse(manifestRaw.replace(/\r\n/g, '\n')) as {
  manifestVersion: number
  apiVersion: number
  id: string
  entry: string
  surfaces: Record<string, unknown>
  permissions: string[]
  hosts: string[]
}

/** 宿主当前的 API 版本（从 Rust 源里取，不从前端那份表反推）。 */
function rustApiVersion(): number {
  const matched = pathsRaw
    .replace(/\r\n/g, '\n')
    .match(/^pub const PLUGIN_API_VERSION: u32 = (\d+);$/m)
  if (!matched?.[1]) throw new Error('paths.rs 里找不到 PLUGIN_API_VERSION')
  return Number(matched[1])
}

/**
 * 只留下代码部分。
 *
 * ⚠️ 这些断言查的是「配置里有没有这句开关」，而 vite.config 的注释里把每条开关都原名写过一遍
 * （那是这份文件的教学价值）。直接对原文 `toContain("enforce: 'pre'")` 会把注释当证据 ——
 * 实测把 `enforce: 'pre'` 改成 `'post'`（正是会静默把 Vue 打进包里的那个错法）护栏全绿。
 */
function codeOf(raw: string): string {
  return raw
    .replace(/\r\n/g, '\n')
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/\/\/[^\n]*/g, '')
}

describe('模板与宿主契约同源', () => {
  it('版本号抄的是宿主当前那两个数', () => {
    expect(manifest.apiVersion, '模板要求的宿主 API 版本与 paths.rs 不一致').toBe(rustApiVersion())
    expect(manifest.apiVersion).toBe(PLUGIN_LIMITS.PLUGIN_API_VERSION)
    expect(manifest.manifestVersion).toBe(PLUGIN_LIMITS.MANIFEST_VERSION)
  })

  /** 入口对不上就是「装上了但 import 404」，而那句话会被作者当成路径写错。 */
  it('清单的 entry 与 vite 产出的文件名一字不差', () => {
    const config = codeOf(viteConfigRaw)
    expect(manifest.entry).toBe('main.js')
    expect(config, "vite 的产出文件名要写死 main.js，与清单的 entry 对齐").toMatch(/fileName:\s*\(\)\s*=>\s*'main\.js'/)
    expect(isSafePluginRelPath(manifest.entry), 'entry 必须是合法的包内相对路径').toBe(true)
    // 单文件产物：多 chunk 的 hash 名没法预先声明（读边界因此只能按目录判）。
    // ⚠️ 这个开关必须挂在 `output` 上：放在 `rollupOptions` 上只会得一句「Invalid key」警告，
    // 然后被静默忽略 —— 产物看着正常，其实还会分片。
    expect(config, '要开单文件开关，且挂在 output 上').toMatch(/output:\s*\{\s*codeSplitting:\s*false/)
    /**
     * Vue 由宿主注入，不是打进包里。
     *
     * 拦 `from 'vue'` 的插件必须跑在 vite 的核心解析器之前（`enforce: 'pre'`），
     * 否则它会静默把 node_modules 里的 vue 打进来 —— 两个 Vue 实例的坏法
     * （provide/inject 与响应式错乱）只在运行时暴露，构建与类型检查全绿。
     */
    expect(config, '拦 vue 的插件要跑在核心解析器之前').toContain("enforce: 'pre'")
    expect(config).toContain('__UFO_VUE__')
    expect(config).toContain('__UFO_PINIA__')
  })

  /** 清单没有 styles 字段：一个包被装载的就是那一个 JS 产物，样式必须由 JS 自己带进来。 */
  it('样式走 JS（宿主没有 styles 这个口子）', () => {
    expect(manifest, '宿主不认 styles 字段').not.toHaveProperty('styles')
    expect(codeOf(indexRaw), '样式要以 ?inline 取成字符串，再由这份 JS 挂 <style>').toContain('?inline')
  })

  it('id 过宿主那套字符规则', () => {
    expect(PLUGIN_ID_PATTERN.test(manifest.id), `id「${manifest.id}」不合法`).toBe(true)
  })

  /**
   * 挂载点与能力名都必须是登记过的名字（未登记 = 拒装）。
   *
   * 挂载点走前端同一个 `declaredSurfaceIds`：登记名是 `tooltip-own` / `tooltip-extension`
   * 这种「挂载点」而不是 JSON 的键名，所以必须按宿主的推导规则来查 —— 模板写成
   * `"tooltip": {}`（漏了 `own: true`）也算没声明，那种包装上后悬停什么都不弹。
   */
  it('四个挂载点与所有能力名都是登记过的名字', () => {
    const surfaceIds = new Set(PLUGIN_SURFACES.map(item => item.id))
    const declared = declaredSurfaceIds(manifest as unknown as PluginManifestJson)
    expect(declared.sort(), '模板要示范全部四种挂载点').toEqual([
      'cell',
      'settings',
      'tooltip-extension',
      'tooltip-own',
    ])
    for (const id of declared) {
      expect(surfaceIds.has(id), `推导出的挂载点「${id}」不在表里`).toBe(true)
    }
    const permissionIds = new Set(PLUGIN_PERMISSIONS.map(item => item.id))
    for (const name of manifest.permissions) {
      expect(permissionIds.has(name), `permissions 里的「${name}」宿主不认识，会直接拒装`).toBe(true)
    }
  })

  /**
   * 模板声明的每一项能力都要在它自己的代码里真的被用到。
   *
   * 方向与「开关必须真的被消费」同一条：样本包多声明一项，用户就多给一份权限，
   * 而那份权限在样本里一行代码都没碰 —— 抄模板的人会一路把无用权限带到真包里。
   */
  it('模板声明的能力都在它自己的代码里被用到', () => {
    const sources = [indexRaw, settingsRaw].join('\n')
    for (const name of manifest.permissions) {
      expect(sources, `模板声明了 ${name}，但代码里没用到它`).toContain(`'${name}'`)
    }
    // 反向也要成立：代码里用到的能力名必须都写进清单（否则运行时一用就抛）。
    for (const name of ['storage', 'events:agent', 'fs']) {
      if (sources.includes(`'${name}'`)) {
        expect(manifest.permissions, `${name} 用到了却没声明`).toContain(name)
      }
    }
  })
})
