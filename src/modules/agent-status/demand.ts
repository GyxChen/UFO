import type { FloatingLayout } from '@/lib/floating-layout'
import { agentStatusCapacity } from './lib/agent-status-layout'
import { agentStatusSettings } from './settings'
import { useAgentStatusStore } from './stores/agentStatus'
import { useFloatingModulesStore } from '@/stores/floatingModules'

/**
 * Agent 状态灯的格子需求：按形态把会话数归一到阶梯档位，上限取本组件自己的 `max_cells`。
 *
 * 写在定义里（`agentStatusModule.demand`）而不是由组件上报，宿主因此不用挂载它
 * 就能算任意形态的窗口大小；组件自己也调它来跟分配到的格子数取小。
 * `max_cells` 只限制本模块的需求，不影响同级组件（窗口仍按各模块需求之和伸缩）。
 */
export function agentStatusDemand(layout: FloatingLayout): number {
  const sessions = useAgentStatusStore().sessions.length
  const maxCells = agentStatusSettings(useFloatingModulesStore().config).max_cells
  return agentStatusCapacity(layout, sessions, maxCells)
}
