import { fileURLToPath, URL } from 'node:url'
import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vite'

/**
 * 组件包的构建：产出 `dist/my-plugin/`（一个 ESM 文件 + 一份清单），装它就能跑。
 *
 * 三条与宿主绑死的规矩，改之前先读 `reference/doc/guide/plugin-guide.md`：
 *
 * 1. **Vue / Pinia 不打进包里**，用宿主注入的那一份（`globalThis.__UFO_VUE__` / `__UFO_PINIA__`）。
 *    两个 Vue 实例的坏法是 `provide/inject` 与响应式静默错乱：界面照常渲染，数据不通。
 * 2. **只出一个文件**（`codeSplitting: false`）。宿主按清单的 `entry` 取入口；带 hash 的多 chunk
 *    会让「包目录里有什么」由构建决定，作者没法预先声明，也容易把文件数帽（256）撑爆。
 * 3. **产物文件名固定 `main.js`**，与 `public/plugin.json` 的 `entry` 一字不差。
 *
 * ⚠️ 为什么这里是一个「生成导出名单」的插件而不是一份手写的 `shims/vue.js`：
 * `.vue` 的编译产物会 `import { unref, toDisplayString, createElementVNode, openBlock, ... } from 'vue'`
 * —— 那是编译器的内部助手名，手写名单一定漏（第一版就漏了，`vite build` 直接 23 个 MISSING_EXPORT）。
 * 名单在建包时从模板自己依赖的 vue / pinia 现取，宿主注入的是同一份库，名字对得上。
 */

/** 拦下 `from 'vue'` / `from 'pinia'`，换成「从宿主注入的全局里取」的生成模块。 */
function ufoHostGlobals() {
  const modules = {
    vue: '__UFO_VUE__',
    pinia: '__UFO_PINIA__',
  }
  /** 虚拟模块 id（`\0` 前缀是 rollup/vite 约定的「不要碰文件系统」）。 */
  const virtualOf = name => `\0ufo-${name}`
  /**
   * 名单缓存必须放在闭包里。
   *
   * ⚠️ 挂在 `this` 上是拿不到的：rolldown-vite 给每个钩子一个独立的上下文对象，
   * `buildStart` 里写的 `this.__ufoShim` 到 `load` 时是 undefined（表现是构建直接崩在
   * 「Cannot read properties of undefined」，而不是一个能看懂的提示）。
   */
  const generated = {}
  return {
    name: 'ufo-host-globals',
    // ⚠️ 必须 `enforce: 'pre'`：vite 的核心解析器排在普通插件之前，
    // 不加它就会把 node_modules 里的 vue 真的打进包里（第一版就是这么静默坏了 ——
    // 产物里一个 `__UFO_VUE__` 都没有，两个 Vue 实例会在运行时才暴露）。
    enforce: 'pre',
    // 配置阶段就把名单算好：`load` 是同步钩子，拿不到 await 的结果。
    async buildStart() {
      for (const [name, globalKey] of Object.entries(modules)) {
        let namespace
        try {
          namespace = await import(name)
        } catch (error) {
          // ⚠️ 这一档必须能缺席：名单是**现取**的，而这个包未必装了它。模板坐在宿主仓库里
          // 时 `import('pinia')` 会向上摸到宿主的 node_modules 而侥幸通过，拷到独立目录
          // 起包就整个构建炸在「Cannot find package」—— 那不是配置错，是没装依赖。
          // 缺席就从 shim 表里摘掉：作者真 `import 'pinia'` 时会得到一句「module not found」，
          // 比在这里崩一个插件栈更可查。
          delete modules[name]
          console.warn(`[ufo-host-globals] 没装 ${name}，这一档不转发到宿主注入的 ${globalKey}：要用就 npm i -D ${name}`)
          continue
        }
        const names = Object.keys(namespace).filter(key => key !== 'default' && isIdentifier(key))
        const body = names.map(key => `export const ${key} = host.${key}`).join('\n')
        generated[name] = `const host = globalThis.${globalKey}
if (!host) throw new Error('这个组件包只能在 UFO 里运行：没找到宿主注入的 ${globalKey}')
${body}
export default host
`
      }
    },
    resolveId(source) {
      return modules[source] ? virtualOf(source) : null
    },
    load(id) {
      for (const name of Object.keys(modules)) {
        if (id === virtualOf(name)) return generated[name]
      }
      return null
    },
  }
}

/** 只把合法标识符导出去：库里偶尔会有奇怪的键名，跳过它们比生成一份坏代码好。 */
function isIdentifier(name) {
  return /^[A-Za-z_$][A-Za-z0-9_$]*$/.test(name)
}

export default defineConfig({
  plugins: [vue(), ufoHostGlobals()],
  // 清单从这里整份拷进产物目录：它是给人读、也给宿主读的同一份文件。
  publicDir: 'public',
  build: {
    lib: {
      entry: fileURLToPath(new URL('./src/index.js', import.meta.url)),
      formats: ['es'],
      fileName: () => 'main.js',
    },
    outDir: 'dist/my-plugin',
    // 每次构建清干净：宿主的包目录文件数有上限（256），留着历史 chunk 会把包撑到装不上。
    emptyOutDir: true,
    target: 'es2022',
    // 不压缩：宿主把装载失败原文显示在管理区，栈里的行号对作者有用。
    minify: false,
    rollupOptions: {
      // 单文件产出。⚠️ `codeSplitting` 要挂在 `output` 上 —— 放在 `rollupOptions` 上只会
      // 得一句「Invalid key」的警告，然后被静默忽略（产物看着正常，其实还是可能分片）。
      output: { codeSplitting: false },
    },
  },
})
