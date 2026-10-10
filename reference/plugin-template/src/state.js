import { reactive } from 'vue'

/**
 * 这个包自己的共享状态：四个挂载点看的是同一份，所以一次订阅能同时喂给格子、信息卡、
 * 插入块和设置页。
 *
 * ⚠️ 不要往里塞 `host`：它是冻结对象，塞进 reactive 会招来「代理不了冻结对象」那一类
 * 开发期告警。要 host 就在组件里 `useHost()`。
 */
export const state = reactive({
  /** 最近一次转灯事件的灯态（`''` = 还没等到）。 */
  phase: '',
  /** 最近一次事件里的 endpoint。 */
  endpoint: '',
  /** 用户在设置页存的那句话（`host.storage` 回读出来的）。 */
  note: '',
  /** 给界面显示的一句错误（不走 console，用户在悬浮窗里看不到 console）。 */
  error: '',
})
