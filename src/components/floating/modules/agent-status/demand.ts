import type { FloatingLayout } from '@/lib/floating-layout'
import { windowCellsForSessions } from '@/lib/agent-status-layout'
import { useAgentStatusStore } from '@/stores/agentStatus'
import { useFloatingModulesStore } from '@/stores/floatingModules'

/**
 * Agent 状态灯的格子需求：一个会话一盏灯，上限由主窗口按形态配置。
 *
 * 写在定义里（`agentStatusModule.demand`）而不是由组件上报，宿主因此不用挂载它
 * 就能算任意形态的窗口大小；组件自己也调它来跟分配到的格子数取小。
 */
export function agentStatusDemand(layout: FloatingLayout): number {
  const sessions = useAgentStatusStore().sessions.length
  return windowCellsForSessions(sessions, useFloatingModulesStore().maxCellsFor(layout))
}