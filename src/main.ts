import { createPinia } from 'pinia'
import { createApp } from 'vue'
import App from './App.vue'
import './assets/main.css'

if (import.meta.env.DEV && import.meta.env.VITE_DEVTOOLS === 'true') {
  const { devtools } = await import('@vue/devtools')
  devtools.connect('http://localhost', 8098)
}

const app = createApp(App)
const pinia = createPinia()

app.use(pinia)
app.mount('#app')
