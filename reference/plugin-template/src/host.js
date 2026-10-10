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
