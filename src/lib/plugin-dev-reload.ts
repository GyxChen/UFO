/**
 * 开发期热重载：问一次「指纹」，变了就重挂。
 *
 * 三个决定各有一个理由：
 * 1. **轮询而不是文件监听**：只有开了热重载的包参与，量是一句 `mtime` 聚合；
 *    作者的包目录常在 junction / WSL 挂载 / 网盘里，那些位置的原生文件事件本就不可靠，
 *    而「问一次指纹」在哪儿都成立。宿主因此不需要 `notify` 这一条依赖。
 * 2. **走读命令**（`get_plugin_dev_digest`）：所以每个窗口都能问 —— 悬浮窗、提示窗里
 *    正在跑的组件包也要跟着换，而写命令（`reload_plugins`）只授设置侧的窗口。
 * 3. **指纹变了只重跑装载**，不重扫盘：台账记录没变，要绕开的只是 ESM 的模块缓存 ——
 *    `plugin-loader` 会给开了热重载的包换一个新的 `?v=`，于是拿到的是作者刚写出的那一份。
 *
 * ⚠️ 这条只在开发期成立（正式包的 `devReload` 被台账强制夹成 false），
 * 界面上那枚开关也只在「本地源」时可点。
 */

import { invoke } from '@tauri-apps/api/core'

/** 问一次指纹的间隔（毫秒）。没有包开着热重载时不会真的发问。 */
const POLL_INTERVAL_MS = 2_000

/** 连续问到这么多次失败就停：IPC 都不通的时候不该一秒敲一次门。 */
const MAX_FAILURES = 3

/** 起一个热重载轮询，返回停止函数。 */
export function startPluginDevReload(input: {
  /** 此刻有没有「已启用且开了热重载」的包；没有就不问（一次 IPC 都不发）。 */
  hasDevPackages: () => boolean
  /** 指纹变了：重挂一次组件包定义。 */
  reload: () => Promise<void> | void
  /** 注入指纹读取（单测用；默认打真实命令）。 */
  fetchDigest?: () => Promise<number>
}): () => void {
  /** 上一次看到的指纹；`undefined` = 还没问过（第一次只记基线，不重挂）。 */
  let last: number | undefined
  let failures = 0
  let stopped = false

  /** 停掉定时器并记住已停（防止在途的那一次回来接着干）。 */
  function stop() {
    stopped = true
    clearInterval(timer)
  }

  const timer = setInterval(() => {
    if (stopped || !input.hasDevPackages()) return
    const fetch = input.fetchDigest ?? (() => invoke<number>('get_plugin_dev_digest'))
    void fetch()
      .then(digest => {
        if (stopped) return
        failures = 0
        if (last === undefined) {
          last = digest
          return
        }
        if (digest === last) return
        last = digest
        return input.reload()
      })
      .catch(error => {
        failures += 1
        if (failures >= MAX_FAILURES) {
          stop()
          console.warn('插件热重载的指纹问不到了，已停止这一窗的轮询', error)
        }
      })
  }, POLL_INTERVAL_MS)

  return stop
}
