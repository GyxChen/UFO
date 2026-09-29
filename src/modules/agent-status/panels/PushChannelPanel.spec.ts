// @vitest-environment happy-dom

/**
 * 「消息推送」面板的组件测试。
 *
 * 只测**面板 → store 的契约**（点哪个控件该调哪个动作、传什么参数），
 * store 本身与后端逻辑各有自己的测试。用假 store 而不是 mock Tauri：
 * 组件不该知道 IPC 的存在，把它 mock 在 store 边界上，断言才稳定。
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import PushChannelPanel from './PushChannelPanel.vue'
import type { ChannelView, ProviderView, PushSnapshot } from '../lib/push'

/** 面板要读的全部 store 表面；动作一律是 vi.fn()。 */
const actions = {
  start: vi.fn(),
  setEnabled: vi.fn(),
  toggleEvent: vi.fn(),
  toggleContent: vi.fn(),
  setProviderEnabled: vi.fn(),
  setProviderServer: vi.fn(),
  addTopic: vi.fn(),
  updateTopic: vi.fn(),
  removeTopic: vi.fn(),
  testProvider: vi.fn(),
  refresh: vi.fn(),
}

/** 可变的面板状态（每个用例重建一份）。 */
const state = vi.hoisted(() => ({ current: null as unknown }))

vi.mock('../stores/pushStatus', () => ({
  usePushStore: () => state.current,
}))

/** 一份两个服务、三行主题的快照。 */
function snapshot(patch: Partial<PushSnapshot> = {}): PushSnapshot {
  const ntfy: ProviderView = {
    id: 'ntfy',
    label: 'ntfy',
    enabled: true,
    server: 'https://ntfy.sh',
    spec: { label: '服务器', default: 'https://ntfy.sh', help: '官方公共服务器留空即可' },
  }
  const bark: ProviderView = {
    id: 'bark',
    label: 'Bark',
    enabled: false,
    server: '',
    spec: { label: '服务器', default: 'https://api.day.app', help: '自建地址在 App 里查' },
  }
  const channels: ChannelView[] = [
    { id: 'c1', provider: 'ntfy', name: '家里电脑', topic: 'ufo-home' },
    { id: 'c2', provider: 'ntfy', name: '公司群', topic: '' },
    { id: 'c3', provider: 'bark', name: 'iPhone', topic: 'k9abc' },
  ]
  return {
    config: {
      enabled: false,
      events: ['turn_finished', 'waiting_for_user', 'failed'],
      content: [],
      providers: {
        ntfy: { enabled: true, server: 'https://ntfy.sh' },
        bark: { enabled: false, server: '' },
      },
      channels: channels.map(channel => ({ ...channel })),
    },
    providers: [ntfy, bark],
    channels,
    ...patch,
  }
}

/** 装好假 store 并挂载面板。 */
async function mountPanel(patch: Partial<PushSnapshot> = {}) {
  const data = snapshot(patch)
  // 写动作必须真的落回快照：否则组件因为反馈文案重渲染时，会把用户刚输入的值顶回旧值，
  // happy-dom 会因此再发一次 change，第二次调用带着旧值把反馈清空（测出来的反馈是空的）。
  actions.updateTopic.mockImplementation((id: string, next: Partial<ChannelView>) => {
    const topic = data.channels.find(item => item.id === id)
    if (topic) Object.assign(topic, next)
    return Promise.resolve()
  })
  state.current = {
    ...data,
    loaded: true,
    connection: 'connected',
    pendingProvider: null,
    isBusy: () => false,
    ...actions,
  }
  const wrapper = mount(PushChannelPanel)
  await flushPromises()
  return wrapper
}

/** 输入框的最小表面（DOMWrapper 与它结构兼容，省得在这里铺 Vue 的类型）。 */
interface InputLike {
  setValue(value: unknown): Promise<unknown>
  trigger(event: string): Promise<unknown>
}

/** 改一个输入框并提交（组件监听的是 change，`setValue` 只触发 input）。 */
async function commit(field: InputLike, value: string) {
  await field.setValue(value)
  await field.trigger('change')
  await flushPromises()
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('PushChannelPanel', () => {
  it('挂载时自己起订阅（不依赖 App.vue 的启动顺序）', async () => {
    await mountPanel()
    expect(actions.start).toHaveBeenCalledTimes(1)
  })

  it('总开关改动直接落到 store', async () => {
    const wrapper = await mountPanel()
    await wrapper.get('.push-master input[type="checkbox"]').setValue(true)
    await flushPromises()
    expect(actions.setEnabled).toHaveBeenCalledWith(true)
  })

  it('服务行的开关只改启用状态，不会顺手切换选中项', async () => {
    const wrapper = await mountPanel()
    // 行不是 button：开关与选择按钮是兄弟节点，点开关不该同时改变选中服务。
    const row = wrapper.findAll('.push-list__row')[1]
    await row.get('.push-list__pick').trigger('click')
    expect(actions.setProviderEnabled).not.toHaveBeenCalled()
    await row.get('input[type="checkbox"]').setValue(true)
    await flushPromises()
    expect(actions.setProviderEnabled).toHaveBeenCalledWith('bark', true)
  })

  it('主题行的名字与主题串各自落进 updateTopic', async () => {
    const wrapper = await mountPanel()
    const first = wrapper.findAll('.push-topic')[0]
    await commit(first.get('.push-topic__name'), '书房')
    expect(actions.updateTopic).toHaveBeenLastCalledWith('c1', { name: '书房' })
    await commit(first.get('.push-topic__value'), 'ufo-study')
    expect(actions.updateTopic).toHaveBeenLastCalledWith('c1', { topic: 'ufo-study' })
  })

  it('主题留空照样保存，但要提醒这一行发不出去', async () => {
    const wrapper = await mountPanel()
    await commit(wrapper.findAll('.push-topic')[0].get('.push-topic__value'), '   ')
    await flushPromises()
    expect(actions.updateTopic).toHaveBeenCalledWith('c1', { topic: '   ' })
    expect(wrapper.get('.push-status').text()).toContain('不能为空')
  })

  it('服务器地址缺 scheme 时不写配置，错误走行内提示', async () => {
    const wrapper = await mountPanel()
    const server = wrapper.get('input[aria-label="ntfy 服务器"]')
    await commit(server, 'ntfy.example.com')
    expect(actions.setProviderServer).not.toHaveBeenCalled()
    expect(wrapper.get('.push-status--err').text()).toContain('http')
  })

  it('服务器地址合法就直接保存（改完即存，没有保存按钮）', async () => {
    const wrapper = await mountPanel()
    await commit(wrapper.get('input[aria-label="ntfy 服务器"]'), 'https://ntfy.example.com')
    expect(actions.setProviderServer).toHaveBeenCalledWith('ntfy', 'https://ntfy.example.com')
  })

  it('删除主题要两步确认', async () => {
    const wrapper = await mountPanel()
    const remove = wrapper.findAll('.push-topic')[0].get('.ep-list__remove')
    await remove.trigger('click')
    expect(actions.removeTopic).not.toHaveBeenCalled()
    // 第一下之后按钮进入 armed 态并显示「确认」，再点才真删。
    const armed = wrapper.findAll('.push-topic')[0].get('.ep-list__remove--armed')
    expect(armed.text()).toBe('确认')
    await armed.trigger('click')
    await flushPromises()
    expect(actions.removeTopic).toHaveBeenCalledWith('c1')
  })

  it('事件勾选直接反映到 store，且带当前取反值', async () => {
    const wrapper = await mountPanel()
    const chips = wrapper.findAll('[aria-label="推送事件"] .push-chip')
    // 四个事件里默认勾了三个。
    expect(chips.filter(chip => chip.attributes('aria-pressed') === 'true')).toHaveLength(3)
    await chips[3].trigger('click')
    await flushPromises()
    expect(actions.toggleEvent).toHaveBeenCalledWith('session_finished', true)
  })

  it('提醒内容勾选走 toggleContent（与事件两组，互不串台）', async () => {
    const wrapper = await mountPanel()
    const chips = wrapper.findAll('[aria-label="提醒内容"] .push-chip')
    expect(chips).toHaveLength(4)
    await chips[1].trigger('click')
    await flushPromises()
    expect(actions.toggleContent).toHaveBeenCalledWith('prompt', true)
    expect(actions.toggleEvent).not.toHaveBeenCalled()
  })

  it('右上角的测试一次给当前服务的每个主题各发一条', async () => {
    actions.testProvider.mockResolvedValue([
      { name: '家里电脑', ok: true, status: 200, error: null },
      { name: '公司群', ok: false, status: 0, error: 'ntfy 返回 404：主题不存在' },
    ])
    const wrapper = await mountPanel()
    const button = wrapper
      .findAll('button')
      .find(item => item.text().startsWith('发一条测试'))
    expect(button, '卡片右上角应有统一测试按钮').toBeTruthy()
    await button!.trigger('click')
    await flushPromises()
    // 只调一次命令，由后端按主题逐条发（面板不自己循环）。
    expect(actions.testProvider).toHaveBeenCalledTimes(1)
    expect(actions.testProvider).toHaveBeenCalledWith('ntfy')
    const status = wrapper.get('.push-status').text()
    expect(status).toContain('家里电脑 ✓')
    expect(status).toContain('公司群 ✗ ntfy 返回 404')
    expect(wrapper.get('.push-status--err').exists(), '有一条失败就该标成警示色').toBe(true)
  })

  it('一行主题都没有时测试按钮禁用', async () => {
    const only = snapshot().channels?.filter(channel => channel.provider === 'bark') ?? []
    const wrapper = await mountPanel({ channels: only })
    const button = wrapper
      .findAll('button')
      .find(item => item.text().startsWith('发一条测试'))
    expect(button!.attributes().disabled).toBeDefined()
  })

  it('后端送来的未知服务照样能渲染（降级显示，不整块空白）', async () => {
    const wrapper = await mountPanel()
    // 面板按 providers 数组渲染，不认具体 id：这里断言没有出现任何 provider 专属分支的痕迹。
    expect(wrapper.findAll('.push-list__row')).toHaveLength(2)
    // 选中服务的主题行渲染出来（名字与主题串都在 input 的 value 里，不在 textContent 里）。
    const rows = wrapper.findAll('.push-topic')
    expect(rows).toHaveLength(2)
    expect(rows[0].get('.push-topic__name').element.value).toBe('家里电脑')
    expect(rows[1].get('.push-topic__value').element.value).toBe('')
  })

  it('不再渲染任何密钥输入框与投递记录列表', async () => {
    const wrapper = await mountPanel()
    // 主题按用户要求明文显示：密码框与「最近发送」那套已经整体删掉，别悄悄加回来。
    expect(wrapper.find('input[type="password"]').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('凭据管理器')
    expect(wrapper.text()).not.toContain('最近发送')
  })
})
