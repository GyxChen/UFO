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
 * - 恒定占用 2 格（`todoDemand`）：徽标 + 最近一条任务
 * - 悬停时通过 `reportHover('todo-list')` 唤起「完整未完成列表」信息卡
 * - 主窗口提供 GeneralSection 单一常规分区：API Token（入凭据管理器）+ 清单筛选 + 刷新周期 + 预览
 */
export const todoModule: FloatingModuleDefinition = {
  id: 'todo',
  label: '滴答待办',
  description: '滴答清单：未完成任务数与最近一项，悬停查看完整未完成列表。',
  version: 1,
  // 新组件默认关闭：用户升级后不会凭空多出一块弹窗，要自己在「悬浮窗设置」里开。
  defaultEnabled: false,
  supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
  component: TodoModule,
  demand: todoDemand,
  settings: {
    // 单一常规分区，主窗口会自动隐藏二级 Tab
    sections: [{ id: 'general', label: '常规设置', component: GeneralSection }],
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
