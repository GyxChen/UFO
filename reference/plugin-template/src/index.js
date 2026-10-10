import styles from './styles.css?inline'
import Cell from './Cell.vue'
import TooltipCard from './TooltipCard.vue'
import ExtensionBlock from './ExtensionBlock.vue'
import SettingsPanel from './SettingsPanel.vue'
import { has } from './host.js'
import { state } from './state.js'

/**
 * 组件包的入口：宿主 `import()` 的就是这份默认导出。
 *
 * 每一项都对应 `public/plugin.json` 里的一句声明。清单没写的挂载点，宿主不会来取。
 */
export default {
  /** 悬浮窗格子里渲染的组件。 */
  cell: Cell,
  /** 悬停自己的格子时弹的独立信息卡。 */
  tooltip: TooltipCard,
  /** 想要的卡尺寸（物理像素）；宿主会夹进工作区，超了不会出屏。 */
  tooltipSize: { width: 320, height: 160 },
  /** 此刻有没有内容可展示：假 = 整卡不弹，不留一块空糊在屏幕上。 */
  hasContent: () => state.phase !== '',
  /** 插进 `agent-status` 信息卡尾部的那一块。 */
  extension: ExtensionBlock,
  /** 插入块此刻有没有内容：没内容就别插，宿主会把整块省掉。 */
  extensionHasContent: () => state.phase !== '',
  /** 主窗一级页签里自己的设置页分区。 */
  settings: [{ id: 'general', label: '常规', component: SettingsPanel }],

  /**
   * 生命周期：每个挂了这个包的窗口各调一次，返回停用函数。
   *
   * ⚠️ 订阅与读盘要放在这里，不要放进组件的 `onMounted`：提示窗在挂载之前就要问 `hasContent`，
   * 那时组件还没 mounted，答案会永远停在「没内容」。
   */
  activate(host) {
    injectStyles()
    // 日志是需求点名的特例：不需要声明权限，任何时候都能打。
    host.log('info', `${host.plugin.name} 已激活`)

    if (has(host, 'storage')) {
      host.storage
        .get('note')
        .then(value => {
          state.note = typeof value === 'string' ? value : ''
        })
        .catch(error => host.log('warn', `读回 note 失败：${error}`))
    }

    if (!has(host, 'events:agent')) {
      // 没声明就不订阅：`host.event.open()` 会抛，而 activate 抛错等于这个包挂不上。
      return () => host.log('info', `${host.plugin.name} 已停用`)
    }

    host.event.open().catch(error => host.log('warn', `订阅失败：${error}`))
    const off = host.event.subscribe(event => {
      // 一次订阅会收到多个主题；模板只关心灯态变化。
      if (event.topic !== 'light_changed') return
      state.phase = event.phase
      state.endpoint = event.endpoint ?? ''
    })

    return () => {
      off()
      void host.event.close().catch(() => undefined)
    }
  },
}

/**
 * 把样式挂进当前窗口。
 *
 * 为什么手动：清单里只有一个 `entry`（一个 JS 产物），构建额外产出的 `.css` 没人去取。
 * `?inline` 让 vite 把 CSS 作为字符串交给 JS，挂 `<style>` 这一步就在自己手里。
 * 带 id 是因为每个窗口都会各调一次 `activate`（悬浮窗、提示窗、主窗），重复挂只会多一份规则。
 */
const STYLE_ID = 'ufo-plugin-my-plugin-styles'
function injectStyles() {
  if (typeof document === 'undefined' || document.getElementById(STYLE_ID)) return
  const element = document.createElement('style')
  element.id = STYLE_ID
  element.textContent = styles
  document.head.appendChild(element)
}
