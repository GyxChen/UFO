<script setup lang="ts">
import GreetComponent from './GreetComponent.vue'

const store = useStore()
store.initApp()
</script>

<template>
  <main class="main-window">
    <h1>Welcome to Tauri 2 + Vue</h1>

    <div class="flex flex-row">
      <a href="https://vitejs.dev" target="_blank">
        <img src="/vite.svg" class="logo vite" alt="Vite logo" />
      </a>
      <a href="https://tauri.app" target="_blank">
        <img src="/tauri.svg" class="logo tauri" alt="Tauri logo" />
      </a>
      <a href="https://vuejs.org/" target="_blank">
        <img src="../assets/vue.svg" class="logo vue" alt="Vue logo" />
      </a>
    </div>
    <p>Click the floating launcher or tray icon to open this window.</p>

    <GreetComponent />
  </main>
</template>
