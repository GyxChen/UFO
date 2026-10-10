import { inject } from 'vue'

/**
 * 宿主能力面的取法：`useHost()`。
 *
 * ⚠️ 一定要在 `setup()` 里调，不要在模块顶层调：顶层只有 `activate(host)` 拿到的那一份是准的，
 * 而组件实例可能挂在别的窗口里。用 `Symbol.for` 的同一枚键，所以产物与宿主各自打包也对得上。
 */
export const PLUGIN_HOST_KEY = Symbol.for('ufo.plugin.host')

/** 取这个包的 host；没取到说明组件被挂在没有绑定的位置。 */
export function useHost() {
  const host = inject(PLUGIN_HOST_KEY, null)
  if (!host) {
    throw new Error('组件被挂在没绑定 host 的位置：请检查它是否由 UFO 装载（plugin-loader 的 withPluginHost）')
  }
  return host
}

/** 这个包声明过某项能力吗（用来在界面上显示「没授权」而不是让调用抛错）。 */
export function has(host, permission) {
  return host.plugin.permissions.includes(permission)
}

/**
 * 外壳给「这一格」的上下文键（也是 `Symbol.for`，产物才 inject 得到）。
 *
 * ⚠️ 悬停上报走的是这一条，不是 `host`：信息卡弹不弹由外壳决定，而外壳只认「哪个模块的哪个 key
 * 正被悬停」。不报就没有任何悬停事件 —— 清单里声明的 `surfaces.tooltip.own` 永远不会出现。
 */
export const FLOATING_MODULE_KEY = Symbol.for('ufo.floating.module')

/**
 * 取外壳注入给这一格的上下文（`allocatedCells` 与 `reportHover`）。
 *
 * 与 `useHost()` 不同，这一份**允许拿不到**：同一个组件被挂在设置页或测试里没有槽位上下文，
 * 少一次悬停不是错，所以这里返回 null 而不抛。
 */
export function useCell() {
  return inject(FLOATING_MODULE_KEY, null)
}
