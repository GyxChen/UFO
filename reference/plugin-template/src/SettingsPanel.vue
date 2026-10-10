<script setup>
import { ref } from 'vue'
import { has, useHost } from './host.js'
import { state } from './state.js'

/**
 * 主窗一级页签里自己的设置页。
 *
 * 两条界面规矩（宿主自己的设置页也守这两条，组件包没有例外）：
 * - 不用 `window.alert` / `confirm`（被宿主的防默认插件拦掉了），反馈一律走行内 `role="status"`；
 * - 「当前是什么状态」的读数要常驻，不许藏进 hover。
 */
const host = useHost()
const draft = ref(state.note)
const status = ref('')
const picked = ref('')

/** 存一句话到自己的存储分区（按包 id 隔离，卸载即删）。 */
async function save() {
  if (!has(host, 'storage')) {
    status.value = '清单没声明 storage，这一项用不了'
    return
  }
  try {
    await host.storage.set('note', draft.value)
    state.note = draft.value
    status.value = '已存进本组件的存储分区'
  } catch (error) {
    status.value = `存不下：${error}`
  }
}

/** 让用户亲手选一个文件（句柄只在这次启动里有效，重启要重选）。 */
async function pick() {
  if (!has(host, 'fs')) {
    status.value = '清单没声明 fs，选不了文件'
    return
  }
  try {
    const file = await host.fs.pick('挑一个文件给模板看看')
    picked.value = `${file.name} · ${file.size} 字节${file.archive ? ' · 能读里面的每一项' : ''}`
    status.value = ''
  } catch (error) {
    // 取消对话框也走到这里：这不是错误，别显示成一句失败。
    picked.value = ''
    status.value = String(error)
  }
}
</script>

<template>
  <section class="tpl-set">
    <h2 class="tpl-set__sr">常规</h2>

    <label class="tpl-set__field">
      <span class="tpl-set__label">存一句话</span>
      <input v-model="draft" class="tpl-set__input" type="text" placeholder="会写进本组件的存储分区" />
    </label>

    <div class="tpl-set__actions">
      <button type="button" class="tpl-set__btn" @click="save">保存</button>
      <button type="button" class="tpl-set__btn" @click="pick">选一个文件…</button>
    </div>

    <p class="tpl-set__picked">{{ picked }}</p>
    <!-- 行内反馈：读数常驻，不藏进 hover，也不用 alert。 -->
    <p class="tpl-set__status" role="status" aria-live="polite">{{ status }}</p>

    <p class="tpl-set__hint">这个包声明的能力：{{ host.plugin.permissions.join(' / ') || '（只有界面挂载点）' }}</p>
  </section>
</template>
