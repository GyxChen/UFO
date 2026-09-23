import { createPinia } from 'pinia'
import { createApp } from 'vue'
import App from './App.vue'
import './assets/main.css'
import { useThemeStore } from './stores/theme'

if (import.meta.env.DEV && import.meta.env.VITE_DEVTOOLS === 'true') {
  const { devtools } = await import('@vue/devtools')
  devtools.connect('http://localhost', 8098)
}

const app = createApp(App)
const pinia = createPinia()

app.use(pinia)

// ⚠️ 主题要在挂载前同步落地：store 的 `start()` 得等 Tauri 事件通道就绪（异步），
// 等它来应用的话，浅色用户每次启动都会先闪一帧深色。
useThemeStore(pinia).hydrate()

app.mount('#app')
