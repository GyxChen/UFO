// @vitest-environment happy-dom

/**
 * 《完成情况》一行的交互护栏：**只有勾选框能切换完成态**。
 *
 * 这条口径容易在改版式时被顺手改回去（把勾选框与标题重新合成一个按钮看着更「整」），
 * 而代价是误勾：想选个字、想再看看标题，任务就完成了。所以这里从组件这一层钉住：
 * 点标题不发 `toggle`，点勾选框才发。
 */
import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { leafNode, type TodoTreeNode } from '../lib/todo-tree'
import type { TodoTaskItem } from '../stores/todo'
import TodoTaskRow from './TodoTaskRow.vue'

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

/** 挂一行。 */
function mountRow(node: TodoTreeNode) {
  return mount(TodoTaskRow, { props: { node } })
}

describe('TodoTaskRow · 只有勾选框能切换完成态', () => {
  it('ignores clicks on the title and toggles from the checkbox', async () => {
    const wrapper = mountRow(leafNode(makeTask()))

    await wrapper.get('.todo-status__title').trigger('click')
    expect(wrapper.emitted('toggle')).toBeUndefined()

    await wrapper.get('.todo-status__check-btn').trigger('click')
    expect(wrapper.emitted('toggle')).toHaveLength(1)
  })

  it('puts the action on the checkbox itself so it is reachable by keyboard', () => {
    const wrapper = mountRow(leafNode(makeTask()))

    // 标题不再是控件；开关只有一个，且它带着「点了会发生什么」的名字
    expect(wrapper.get('.todo-status__title').element.tagName).toBe('SPAN')
    expect(wrapper.get('.todo-status__check-btn').attributes('aria-label')).toBe('完成：写周报')
  })

  it('flips the checkbox label and state for a completed task', () => {
    const wrapper = mountRow(leafNode(makeTask({ completed_at_ms: 1_700_000_000_000 })))

    expect(wrapper.get('.todo-status__check-btn').attributes('aria-label')).toBe('取消完成：写周报')
    expect(wrapper.get('.todo-status__check').classes()).toContain('is-done')
    expect(wrapper.get('.todo-status__check').text()).toBe('✓')
  })

  it('disables the checkbox while the children drive completion', () => {
    const child = leafNode(makeTask({ id: 'c1', parent_id: 't1' }))
    const wrapper = mountRow({ task: makeTask(), children: [child] })

    // 有未完成子目标：这一条不由用户点，勾选框转虚线且不可点
    expect(wrapper.get('.todo-status__check-btn').attributes('disabled')).toBeDefined()
    expect(wrapper.get('.todo-status__check').classes()).toContain('is-derived')
    expect(wrapper.get('.todo-status__check-btn').attributes('aria-label')).toContain('由子目标驱动')
  })

  it('keeps the collapse twist only for rows with open children', async () => {
    const child = leafNode(makeTask({ id: 'c1', parent_id: 't1' }))
    const wrapper = mountRow({ task: makeTask(), children: [child] })

    const twist = wrapper.get('.todo-status__twist-btn')
    expect(twist.attributes('aria-expanded')).toBe('true')

    await twist.trigger('click')
    expect(wrapper.emitted('toggle-collapse')).toHaveLength(1)

    // 叶子行没有折叠按钮，但留同宽占位 —— 勾选框才能上下对齐成一列
    const leaf = mountRow(leafNode(makeTask()))
    expect(leaf.find('.todo-status__twist-btn').exists()).toBe(false)
    expect(leaf.find('.todo-status__twist').exists()).toBe(true)
  })

  it('marks a pending row as busy so the checkbox cannot be clicked twice', () => {
    const wrapper = mount(TodoTaskRow, { props: { node: leafNode(makeTask()), pending: true } })

    expect(wrapper.get('.todo-status__check-btn').attributes('aria-busy')).toBe('true')
    expect(wrapper.get('.todo-status__check-btn').attributes('disabled')).toBeDefined()
  })
})
