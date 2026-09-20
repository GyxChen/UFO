// @vitest-environment happy-dom

/**
 * 信息卡的回归护栏：**它就是设置页「完成情况」里那组未完成任务，但少了两个东西**。
 *
 * 这一块最容易在改版时被「顺手补全」——把设置页卡头的按钮、右键菜单一并搬过来看着更完整，
 * 代价却是把「看一眼 + 勾一下」的地方变成了又一个写操作入口（信息卡还是悬停才出现的，
 * 批量顺延 / 删除放在这里既容易误触，也没地方做二次确认）。所以这里从组件这一层钉住：
 *
 * 1. 渲染的是**同一套行组件与树**（`.todo-status__row` + `--todo-depth` 缩进）；
 * 2. **没有任何卡头按钮**（刷新 / 全部顺延）；
 * 3. **右键不弹菜单**（设置页那边会弹「顺延 / 删除」）；
 * 4. 点勾选框完成，且**连带完成父目标**（与设置页同一条 `cascadeTargets` 路径）；
 * 5. 失败只写进概览的「完成失败」行，不弹系统对话框。
 */
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import TodoTooltip from './TodoTooltip.vue'
import { useTodoStore, type TodoSnapshot, type TodoTaskItem } from './stores/todo'

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
// store 在 setup 里读窗口 label（只在悬浮窗起轮询）；测试环境没有 Tauri 运行时。
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ label: 'tooltip' }) }))

/**
 * 造一条任务：只写测试关心的字段，其余给中性值。
 *
 * @param overrides 本次要改的字段
 * @returns 一条未完成的普通任务
 */
function makeTask(overrides: Partial<TodoTaskItem> = {}): TodoTaskItem {
  return {
    id: 't1',
    title: '写周报',
    project_id: 'inbox',
    project_name: '收件箱',
    priority: 3,
    due_at_ms: null,
    has_time: false,
    items_total: 0,
    items_done: 0,
    completed_at_ms: null,
    parent_id: null,
    subtasks: [],
    ...overrides,
  }
}

/**
 * 造一份快照。
 *
 * @param list 未完成任务
 * @param completed 近 7 天已完成任务（信息卡要拿它算子目标进度 `N/M`）
 * @returns 快照
 */
function snapshotOf(list: TodoTaskItem[], completed: TodoTaskItem[] = []): TodoSnapshot {
  return { list, total_incomplete: list.length, completed, updated_at_ms: 1_700_000_000_000 }
}

/**
 * 挂载信息卡。
 *
 * @param snapshot 要灌进 store 的快照；null = 还没拉到数据
 * @returns 组件包装器与 store
 */
function mountTooltip(snapshot: TodoSnapshot | null) {
  const pinia = createPinia()
  setActivePinia(pinia)
  const todoStore = useTodoStore()
  todoStore.snapshot = snapshot
  const wrapper = mount(TodoTooltip, {
    props: { target: { moduleId: 'todo', key: 'todo-list' } },
    global: { plugins: [pinia] },
  })
  return { wrapper, todoStore }
}

beforeEach(() => {
  invokeMock.mockReset()
})

describe('TodoTooltip · 照搬《完成情况》的未完成组', () => {
  it('renders the unfinished tree with the same rows and indentation as the settings card', () => {
    const parent = makeTask({ id: 'p1', title: '发布新版本' })
    const child = makeTask({ id: 'c1', title: '写发布说明', parent_id: 'p1' })
    const { wrapper } = mountTooltip(snapshotOf([parent, child]))

    // 与设置页同一个分组标题（带计数），说明这一块是「照搬」而不是另画的
    expect(wrapper.get('.todo-status__group-title').text().replace(/\s+/g, '')).toBe('未完成2')
    // 目标一行、子目标一行，缩进由 flattenTree 给的深度变量决定
    const rows = wrapper.findAll('.todo-status__row')
    expect(rows).toHaveLength(2)
    expect(rows[0].attributes('style')).toContain('--todo-depth: 0')
    expect(rows[1].attributes('style')).toContain('--todo-depth: 1')
    // 概览四行仍在（范围 / 未完成 / 已逾期 / 更新时间）
    expect(wrapper.text()).toContain('范围')
    expect(wrapper.text()).toContain('已逾期')
    expect(wrapper.text()).toContain('更新时间')
  })

  it('carries no header buttons and no context menu', async () => {
    const { wrapper } = mountTooltip(snapshotOf([makeTask()]))

    // 卡头按钮留在设置页：信息卡只放任务树
    const labels = wrapper.findAll('button').map(button => button.text())
    expect(labels.some(text => text.includes('刷新'))).toBe(false)
    expect(labels.some(text => text.includes('顺延'))).toBe(false)
    expect(wrapper.text()).not.toContain('全部顺延')

    // 右键一行：设置页那边会弹出「顺延 / 删除」，信息卡不弹任何菜单
    await wrapper.get('.todo-status__row').trigger('contextmenu')
    await flushPromises()
    expect(document.querySelector('[role="menu"]')).toBeNull()
  })

  it('completes the task from the checkbox and cascades to the finished parent', async () => {
    const parent = makeTask({ id: 'p1', title: '发布新版本' })
    const child = makeTask({ id: 'c1', title: '写发布说明', parent_id: 'p1' })
    invokeMock.mockResolvedValue(snapshotOf([]))
    const { wrapper } = mountTooltip(snapshotOf([parent, child]))

    const boxes = wrapper.findAll('.todo-status__check-btn')
    // 父目标（第一行）有未完成子目标：它的勾选框不可点（完成由子目标驱动），只有子目标能点
    expect(boxes[0].attributes('disabled')).toBeDefined()
    expect(boxes[1].attributes('disabled')).toBeUndefined()

    await boxes[1].trigger('click')
    await flushPromises()

    // 子目标在前、被连带完成的父目标在后，一次提交（与设置页同一条链路）
    expect(invokeMock).toHaveBeenCalledWith('complete_todo_task', {
      targets: [
        { projectId: 'inbox', taskId: 'c1' },
        { projectId: 'inbox', taskId: 'p1' },
      ],
    })
  })

  it('reports a failed completion in the overview instead of opening a dialog', async () => {
    invokeMock.mockRejectedValue('网络超时')
    const { wrapper } = mountTooltip(snapshotOf([makeTask()]))

    await wrapper.get('.todo-status__check-btn').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('完成失败')
    expect(wrapper.text()).toContain('网络超时')
  })

  it('guides to the settings page while there is nothing to show', () => {
    const { wrapper } = mountTooltip(null)

    expect(wrapper.find('.todo-status__row').exists()).toBe(false)
    expect(wrapper.text()).toContain('填入 API Token')
  })
})
