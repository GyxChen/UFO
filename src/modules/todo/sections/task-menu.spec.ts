// @vitest-environment node

/**
 * 目标右键菜单项（纯数据）的单测。菜单本体是通用组件，这里只锁这一层产出的内容：
 * 1. 「顺延到今天」按 `canPostpone` 置灰（不逾期的目标不该给点，但也不能整项消失）；
 * 2. 「删除目标」永远可点，且标成破坏性色调（红字 + 与上面的项分隔）；
 * 3. id 与调用方分派时用的常量一致 —— 对不上就是「点了没反应」。
 */
import { describe, expect, it } from 'vitest'
import { TASK_MENU_DELETE, TASK_MENU_POSTPONE, buildTaskMenuItems } from './task-menu'

/** 取某一项。 */
function item(id: string, canPostpone: boolean) {
  const found = buildTaskMenuItems({ canPostpone }).find(entry => entry.id === id)
  expect(found, `菜单里没有 ${id} 这一项`).toBeTruthy()
  return found!
}

describe('buildTaskMenuItems', () => {
  it('greys out the postpone item when the task cannot be postponed', () => {
    const postpone = item(TASK_MENU_POSTPONE, false)

    expect(postpone.label).toBe('顺延到今天')
    expect(postpone.disabled).toBe(true)
  })

  it('enables the postpone item when the task can be postponed', () => {
    expect(item(TASK_MENU_POSTPONE, true).disabled).toBe(false)
  })

  it('always offers the destructive delete item, marked as danger', () => {
    const del = item(TASK_MENU_DELETE, false)

    expect(del.label).toBe('删除目标')
    expect(del.disabled).toBeUndefined()
    expect(del.tone).toBe('danger')
  })

  it('keeps the postpone item above the destructive one', () => {
    // 破坏性操作排在最后，菜单的视觉分组（普通项在上、分隔线之下是删除）才成立
    expect(buildTaskMenuItems({ canPostpone: true }).map(entry => entry.id)).toEqual([
      TASK_MENU_POSTPONE,
      TASK_MENU_DELETE,
    ])
  })
})
