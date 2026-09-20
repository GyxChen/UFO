import type { FloatingModuleDefinition } from '@/lib/floating-modules'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import TodoModule from './TodoModule.vue'
import TodoTooltip from './TodoTooltip.vue'
import GeneralSection from './sections/GeneralSection.vue'
import { todoDemand } from './demand'
import { todoSettings } from './settings'
import { useTodoStore } from './stores/todo'

// 导入待办模块专用样式
import './todo.css'

/**
 * 滴答清单待办悬浮窗模块定义。
 *
 * - 恒定占用 2 格（`todoDemand`）：三面小旗 = 高 / 中 / 低三档的未完成数（旗子字形取自
 *   `src/assets/flag.svg`，红 / 黄 / 蓝由 CSS 上色），不显示任务明细
 *   （方形 / 四角 / 横条横排三面，竖条竖排三面，见 `TodoModule.vue` 与 `todo.css`）
 * - 悬停时通过 `reportHover('todo-list')` 唤起信息卡：就是设置页「完成情况」里的未完成组
 *   （同一套行组件与树逻辑，可滚动、点勾选框即完成、父目标连带完成），**不带右键菜单**
 * - 主窗口提供 GeneralSection 单一常规分区：新增任务 + API Token（入凭据管理器）+ 清单筛选
 *   + 刷新周期 + 完成情况
 */
export const todoModule: FloatingModuleDefinition = {
  id: 'todo',
  label: '滴答待办',
  description: '滴答清单：三面小旗显示高 / 中 / 低三档的未完成数，悬停查看未完成任务并可直接完成。',
  version: 1,
  // 新组件默认关闭：用户升级后不会凭空多出一块弹窗，要自己在「悬浮窗设置」里开。
  defaultEnabled: false,
  supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
  component: TodoModule,
  demand: todoDemand,
  settings: {
    // 单一常规分区，主窗口会自动隐藏二级 Tab。fill: 占满主窗内容高度、只有列表内部滚动
    // （见「组件开发规范」的高度链），否则设置页三块卡会超出主窗、出现整页 scrollbar。
    sections: [{ id: 'general', label: '常规设置', component: GeneralSection, fill: true }],
  },
  activate: () => {
    const store = useTodoStore()
    void store.start()
    return () => {
      store.stop()
    }
  },
  tooltip: {
    component: TodoTooltip,
    size: { width: 340, height: 400 },
    // 没配 Token 时不弹卡（`hasContent` 在内容组件挂载前求值，判据只能来自配置）
    hasContent: () => todoSettings(useFloatingModulesStore().config).has_token,
  },
}
