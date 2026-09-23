import { fileURLToPath, URL } from 'node:url'
import { mergeConfig } from 'vite'
import { defineConfig } from 'vitest/config'
import { version as pkgVersion } from './package.json'
import viteConfig from './vite.config'

process.env.VITE_APP_VERSION = pkgVersion
if (process.env.NODE_ENV === 'production') {
  process.env.VITE_APP_BUILD_EPOCH = new Date().getTime().toString()
}

export default mergeConfig(
  viteConfig,
  defineConfig({
    test: {
      globals: true,
      /*
       * 只放行「CSS + ?raw」这一种组合：默认 vitest 会把所有 .css 请求打桩成空串，
       * 于是 import.meta.glob 带 ?raw 去读模块的 .css 时拿到的是空串，读样式内容的护栏测试
       * （floating-module-scaling.spec.ts、assets/theme-css.spec.ts）就没法工作。
       * 其余 CSS 导入（组件 import './x.css'）行为完全不变，仍是空桩 —— 测试环境没有真实排版。
       */
      css: { include: [/\.css\?raw/] },
      include: ['tests/unit/**/*.test.ts', 'src/**/*.spec.ts'],
      root: fileURLToPath(new URL('./', import.meta.url)),
      setupFiles: ['./tests/setup/testglobals.ts'],
      coverage: {
        provider: 'v8',
        reporter: ['text', 'json', 'json-summary'],
        include: ['!src/main.ts', 'src/**/*.ts', 'src/**/*.vue'],
        thresholds: {
          lines: 10,
          functions: 0,
          branches: 10,
          statements: 10,
        },
      },
    },
  })
)
