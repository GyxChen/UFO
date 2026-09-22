import type { FloatingModuleDefinition } from '@/lib/floating-modules'
import { overflowListFor } from './lib/agent-status-layout'
import { useAgentStatusStore } from './stores/agentStatus'
import { useFlowResultsStore } from './stores/flowResults'
import AgentStatusModule from './AgentStatusModule.vue'
import AgentStatusTooltip from './AgentStatusTooltip.vue'
import ExtensionsSection from './sections/ExtensionsSection.vue'
import GeneralSection from './sections/GeneralSection.vue'
import RecordsSection from './sections/RecordsSection.vue'
import { agentStatusDemand } from './demand'
// 模块自己的视图 / 信息卡 / 设置面板样式（从 src/assets/main.css 切出来），随模块一起加载。
import './agent-status.css'

/**
 * Agent 状态灯模块：一个 CLI 会话一盏灯。
 *
 * 格子需求（`demand`）、悬停信息卡（`tooltip`）、设置页分区（`settings.sections`）、
 * 生命周期（`activate`）都挂在这里：宿主与主窗口只认这份定义，不认识组件内部；
 * 模块要订阅什么、要起哪个 store，也只写在这里（外壳按注册表统一调用）。
 */
export const agentStatusModule: FloatingModuleDefinition = {
  id: 'agent-status',
  label: 'Agent 状态灯',
  description: '每个 CLI 会话一盏灯：绿=执行中 / 黄=等待操作 / 红=出错；放不下的折成 +N 环，悬停看折叠的那几盏。',
  version: 1,
  defaultEnabled: true,
  supportedLayouts: ['square', 'corner', 'horizontal', 'vertical'],
  component: AgentStatusModule,
  demand: agentStatusDemand,
  /**
   * 设置页分三个分区，主窗口会把它渲染成组件页顶部的二级 tabbar。
   * 顺序 = tab 顺序；第一个（常规）是点齿轮 / 点悬浮窗进来时落到的默认分区。
   *
   * 切法不是「一个配置项一个 tab」，而是按**版面**切：短表单全收在「常规」一页，
   * 需要占满高度、左右各自滚动的两个面板各占一个 tab（`fill: true`）。
   */
  settings: {
    sections: [
      { id: 'general', label: '常规', component: GeneralSection },
      { id: 'extensions', label: '信息卡拓展', component: ExtensionsSection, fill: true },
      { id: 'records', label: '会话记录', component: RecordsSection, fill: true },
    ],
  },
  /**
   * 生命周期：三个窗口都要订阅会话快照与流程脚本结果 ——
   * 提示窗的数据晚一步到就弹不出信息卡（`hasContent` 在内容组件挂载前求值），
   * 所以在挂载时就把订阅拉起来，而不是等组件自己 `onMounted`。
   */
  activate: () => {
    const agentStatus = useAgentStatusStore()
    const flowResults = useFlowResultsStore()
    void agentStatus.start()
    void flowResults.start()
    return () => {
      agentStatus.stop()
      flowResults.stop()
    }
  },
  tooltip: {
    component: AgentStatusTooltip,
    // 一屏预算：单会话 = head + Endpoint + token + 4 字段 + 刷新标注 + 提问 1 行 ≈ 236px；
    // 折叠清单 = head + 7 行 + 一行兜底 ≈ 213px —— 两种内容都在 260 内（见 AGENTS.md）。
    size: { width: 300, height: 260 },
    /**
     * 会话数据要先到达提示窗才弹（提示窗是独立 webview，事件异步），
     * 否则会先弹一个空框、再填内容（且位置可能还在上一个目标那里）。
     *
     * 溢出格的合成 key（`@overflow:<layout>:<cells>`）走另一条判定：**有折叠才弹** ——
     * 折叠在悬停期间都结束了就给 false，与单会话「目标已消失」同款处理。
     */
    hasContent: target => {
      const sessions = useAgentStatusStore().sessions
      const overflow = overflowListFor(target.key, sessions)
      return overflow ? overflow.length > 0 : sessions.some(session => session.key === target.key)
    },
  },
}
