/**
 * 「完成情况」里一条目标的右键菜单项。
 *
 * ⚠️ 菜单本体是**自绘浮层**（`components/ui/BaseContextMenu.vue`），不是 Tauri 的系统原生菜单：
 * 原生菜单由操作系统画在另一个窗口里，观感与 Matte 面板割裂，而且它接管输入 ——
 * 「点菜单外面只关菜单、不触发底下的 UI」这条做不到（那是自绘那一层自己实现的）。
 *
 * 所以这里只产出**纯数据**：有哪些项、哪一项能不能点。动作由调用方按 id 分派
 * （`GeneralSection.vue` 的 `handleMenuSelect`），本文件不认识 store，也不碰网络。
 */

import type { ContextMenuItem } from '@/lib/context-menu'

/** 「顺延到今天」项 id。 */
export const TASK_MENU_POSTPONE = 'postpone'
/** 「删除目标」项 id。 */
export const TASK_MENU_DELETE = 'delete'

/** 生成菜单需要的输入。 */
export interface TaskMenuOptions {
  /** 这条目标此刻能不能顺延：只有**逾期且未完成**的才有意义 */
  canPostpone: boolean
}

/**
 * 生成一条目标的右键菜单项。
 *
 * @param options 目标此刻的状态
 * @returns 菜单项，顺序即渲染顺序
 */
export function buildTaskMenuItems(options: TaskMenuOptions): ContextMenuItem[] {
  return [
    {
      id: TASK_MENU_POSTPONE,
      label: '顺延到今天',
      // 不逾期就没什么可顺延的：留一个置灰项，用户能看出「是这条不逾期」而不是菜单坏了
      disabled: !options.canPostpone,
    },
    {
      id: TASK_MENU_DELETE,
      label: '删除目标',
      // 删除不可撤销：红字 + 与上面的普通项隔开；二次确认由调用方弹（菜单里塞不下）
      tone: 'danger',
    },
  ]
}
