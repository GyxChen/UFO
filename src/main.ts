import { createPinia } from 'pinia'
import { createApp } from 'vue'
import App from './App.vue'
import './assets/main.css'
import { installPluginGlobals } from '@/lib/plugin-runtime'
import { useThemeStore } from './stores/theme'

if (import.meta.env.DEV && import.meta.env.VITE_DEVTOOLS === 'true') {
  const { devtools } = await import('@vue/devtools')
  devtools.connect('http://localhost', 8098)
}

// ⚠️ 必须早于任何组件包产物的 `import()`：插件产物 external 掉 Vue / Pinia，运行时从这两支全局取。
// 晚一步的表现是「冷启动插件报 undefined，HMR 之后又好了」—— 全场只有一个 Vue 实例这条不变量的前提。
installPluginGlobals()

const app = createApp(App)
const pinia = createPinia()

app.use(pinia)

// ⚠️ 主题要在挂载前同步落地：store 的 `start()` 得等 Tauri 事件通道就绪（异步），
// 等它来应用的话，浅色用户每次启动都会先闪一帧深色。
useThemeStore(pinia).hydrate()

app.mount('#app')
