/**
 * 权限徽标的图标：图标键 → 24×24 的单色描边路径。
 *
 * 全部走 `stroke`（不用 fill）：同一批徽标在 11.5px 下粗细观感才一致，
 * 而且不必为每个图标纠结填充方向。颜色由徽标自己的 tone 类给（`currentColor`）。
 *
 * ⚠️ 键名与 Rust `capability.rs` 的 `icon` 字段逐条对应，由
 *    `plugin-capabilities.spec.ts` 断言「表里出现的每个键都有路径」。
 */
export const PLUGIN_CAPABILITY_ICONS: Record<string, string> = {
  // 挂载点：悬浮窗格子。
  grid: 'M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z',
  // 挂载点：插件自己的独立信息卡（气泡）。
  card: 'M4 4h16v12H9l-5 4zM8 8h8M8 11h6',
  // 挂载点：插进别人信息卡的一块（面板 + 追加的行）。
  insert: 'M4 4h16v16H4zM7 8h10M7 11h6M7 15h8',
  // 挂载点：主窗设置页（齿轮）。
  gear: 'M12 8a4 4 0 100 8 4 4 0 000-8zM12 2v3M12 19v3M2 12h3M19 12h3',
  // 能力：出网。
  globe: 'M12 21a9 9 0 100-18 9 9 0 000 18zM3.6 9h16.8M3.6 15h16.8M12 3a14 14 0 000 18M12 3a14 14 0 010 18',
  // 能力：按包隔离的持久化存储。
  database: 'M12 3c4.4 0 8 1.3 8 3s-3.6 3-8 3-8-1.3-8-3 3.6-3 8-3zM4 6v6c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6',
  // 能力：订阅转灯事件。
  bell: 'M12 3a5 5 0 00-5 5v3.5L5.5 17h13L17 11.5V8a5 5 0 00-5-5zM10 19.5a2 2 0 004 0',
  // 能力：文件读写。
  folder: 'M3 6h6l2 2h10v11H3zM3 6v13',
  // 能力：密钥。
  key: 'M9 4a4 4 0 110 8 4 4 0 010-8zM9 8a0.5 0.5 0 100 1 0.5 0.5 0 000-1zM12.5 8H21M18 8v3.5M20.5 8v2.5',
  // 能力：蓝牙。
  bluetooth: 'M8 8l8 8-4 3.5V4.5L16 8l-8 8',
  // 能力：串口。
  plug: 'M9 3v5M15 3v5M6 8h12v3a6 6 0 01-12 0zM12 17v4',
  // 能力：内嵌网页（框架窗口）。
  frame: 'M3 5h18v14H3zM3 9.5h18M6 7.2h1.5',
}

/** 取一个图标的路径；未知键返回空串（渲染成无路径的空框，绝不崩）。 */
export function pluginIconPath(icon: string): string {
  return PLUGIN_CAPABILITY_ICONS[icon] ?? ''
}
