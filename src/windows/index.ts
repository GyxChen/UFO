import type { Component } from 'vue'
import ScriptEditorWindow from '@/components/windows/ScriptEditorWindow.vue'
import StatusLogWindow from '@/modules/agent-status/panels/StatusLogWindow.vue'
import PluginLogWindow from '@/components/plugins/PluginLogWindow.vue'
import { SCRIPT_EDITOR_WINDOW_LABEL } from '@/lib/script-editor-window'
import { STATUS_LOG_WINDOW_LABEL } from '@/modules/agent-status/lib/status-log-window'
import { PLUGIN_LOG_WINDOW_LABEL } from '@/lib/plugin-log-window'

/**
 * 主窗以外那些「按需显示」的独立面板窗：窗口 label → 该渲染哪个组件。
 *
 * 这里是窗口侧的组合根（与 `src/modules/index.ts` 同一地位），**只有这里允许点名模块** ——
 * `App.vue` 只按 label 查这张表，所以新增一个独立窗口是两步：在 `tauri.conf.json` 里声明
 * （`visible: false`）+ 在这里登记一行，外壳文件零改动。
 *
 * ⚠️ label 一律取各窗口契约里的常量，不写字面量：`tauri.conf.json`、`capabilities/*.json`
 * 与前端常量必须同源，对不上的话窗口开不出来或被 capability 挡在门外（护栏见 `panel-windows.spec.ts`）。
 */
export const PANEL_WINDOW_VIEWS: Record<string, Component> = {
  [SCRIPT_EDITOR_WINDOW_LABEL]: ScriptEditorWindow,
  [STATUS_LOG_WINDOW_LABEL]: StatusLogWindow,
  [PLUGIN_LOG_WINDOW_LABEL]: PluginLogWindow,
}
