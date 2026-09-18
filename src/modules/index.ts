import type { FloatingModuleDefinition } from '@/lib/floating-modules'
import { agentStatusModule } from './agent-status'

/**
 * 悬浮窗模块注册表：新增组件就是在 `src/modules/<id>/` 下建目录（视图 / lib / stores / 样式都收在里面），
 * 然后在这里加一条；悬浮窗宿主、主窗口组件列表、提示窗分派、模块生命周期都读它。
 *
 * 数组顺序 = 主窗口列表的展示顺序；悬浮窗内的先后顺序按配置里的 `order`。
 */
export const FLOATING_MODULE_DEFINITIONS: FloatingModuleDefinition[] = [agentStatusModule]
