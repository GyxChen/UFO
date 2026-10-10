import { computed } from 'vue'
import type { FloatingModuleDefinition } from '@/lib/floating-modules'
import { pluginDefinitions } from '@/lib/plugin-loader'
import { agentStatusModule } from './agent-status'
import { todoModule } from './todo'
import { weatherModule } from './weather'

/**
 * 内置模块的定义：这里只是内置那半边的登记处。
 *
 * ⚠️ 顺序 = 主窗口列表的展示顺序；悬浮窗里的先后按配置里的 `order`（外壳负责排）。
 */
export const BUILTIN_MODULE_DEFINITIONS: FloatingModuleDefinition[] = [
  agentStatusModule,
  todoModule,
  weatherModule,
]

/**
 * 悬浮窗模块注册表：内置在前、组件包在后。
 *
 * 悬浮窗宿主、主窗口组件列表、提示窗分派、模块生命周期都读这一份 ——
 * 组件包（`plugin-loader` 从台账装载出来的那些）进的是同一张表，
 * 所以「装一个插件」不需要外壳多出任何一条分支。
 *
 * ⚠️ 它是 `computed`：装载完成后整份换掉，读到它的界面要跟着重算；
 * 消费点一律写 `FLOATING_MODULE_DEFINITIONS.value`（三窗与主窗都算）。
 */
export const FLOATING_MODULE_DEFINITIONS = computed<FloatingModuleDefinition[]>(() => [
  ...BUILTIN_MODULE_DEFINITIONS,
  ...pluginDefinitions.value,
])
