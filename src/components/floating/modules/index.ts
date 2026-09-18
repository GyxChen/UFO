import type { FloatingModuleDefinition } from '@/lib/floating-modules'
import { agentStatusModule } from './agent-status'

/**
 * 悬浮窗模块注册表：新增组件就是在 `modules/<id>/` 下建目录，
 * 然后在这里加一条；悬浮窗宿主与主窗口组件选择列表都读它。
 *
 * 数组顺序 = 主窗口列表的展示顺序；悬浮窗内的先后顺序按配置里的 `order`。
 */
export const FLOATING_MODULE_DEFINITIONS: FloatingModuleDefinition[] = [agentStatusModule]
