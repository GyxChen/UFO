// @vitest-environment happy-dom

/**
 * 插件管理区的行为护栏。
 *
 * 锁的是「流程形状」而不是像素：装与卸都必须两步、启用只有一个写入入口、
 * 徽标只渲染注册表里真有其名的条目。这几条错的表现都是「用户以为自己做对了」。
 */
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import PluginManagerCard from './PluginManagerCard.vue'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import { usePluginsStore } from '@/stores/plugins'
import type { CapabilityFactJson, PluginRecordJson } from '@/lib/plugin-types'

const { openMock, invokeMock, openLogMock } = vi.hoisted(() => ({
  openMock: vi.fn(),
  invokeMock: vi.fn(),
  openLogMock: vi.fn(async () => true),
}))

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: openMock }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}), emitTo: vi.fn(async () => {}) }))
// 问号角标的悬浮框会自己挂事件，测行为不需要它；留个空壳。
vi.mock('@/lib/plugin-log-window', () => ({ openPluginLogWindow: openLogMock }))
vi.mock('@/components/ui/SettingsHint.vue', () => ({
  default: { name: 'SettingsHint', template: '<i class="hint-stub" />' },
}))

function fact(id: string, tone = 'local'): CapabilityFactJson {
  return { id, kind: 'permission', label: `徽标 ${id}`, icon: 'grid', tone, disclosure: `${id} 会碰到什么` }
}

function record(patch: Partial<PluginRecordJson> = {}): PluginRecordJson {
  return {
    id: 'balance-board',
    name: '余额看板',
    version: '1.2.0',
    manifest: {
      id: 'balance-board',
      name: '余额看板',
      version: '1.2.0',
      description: '把余额画进一格',
      entry: 'main.js',
      surfaces: { cell: 2, settings: true },
      permissions: ['http'],
      hosts: ['api.example.com'],
    },
    dir: 'C:/work/balance-board',
    source: 'local',
    devReload: false,
    warnings: [],
    files: 2,
    bytes: 2048,
    loadMs: 42,
    error: null,
    ...patch,
  }
}

/** 装好两个 store 的初始状态（管理区只读它们，不自己碰 IPC）。 */
function seed(records: PluginRecordJson[]) {
  const plugins = usePluginsStore()
  plugins.applyBroadcast({ revision: 1, records })
  plugins.registry = [fact('cell'), fact('settings'), fact('http', 'caution')]
  const modules = useFloatingModulesStore()
  modules.applyConfig({ schema_version: 1, revision: 1, modules: [], module_settings: {} })
  return { plugins, modules }
}

/** 当前用例的 pinia：mount 必须显式装上它，否则组件里的 `usePluginsStore()` 会拿到另一份实例。 */
let pinia: Pinia

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  openMock.mockReset()
  invokeMock.mockReset()
  openLogMock.mockReset()
  openLogMock.mockResolvedValue(true)
  invokeMock.mockResolvedValue({ schema_version: 1, revision: 2, modules: [], module_settings: {} })
})

/** 统一的挂载入口（带上本用例的 pinia）。 */
function mountCard() {
  return mount(PluginManagerCard, { global: { plugins: [pinia] } })
}

describe('已装列表', () => {
  it('一行给名字、版本与格数，权限徽标只在右栏出现', async () => {
    const { plugins } = seed([record()])
    plugins.select('balance-board')
    const wrapper = mountCard()
    const row = wrapper.find('.module-row')
    expect(row.text()).toContain('余额看板')
    expect(row.text()).toContain('1.2.0')
    expect(row.text()).toContain('2 格')
    // 徽标只在右栏：左栏出现徽标就说明窄列会被挤成省略号。
    expect(wrapper.find('.plugin-manager__column').find('.plugin-manager__perms').exists()).toBe(false)
    expect(wrapper.findAll('.plugin-manager__perms li')).toHaveLength(3)
  })

  it('校验失败的那行直接说原因，不说一句看不懂的空态', () => {
    const { plugins } = seed([record({ manifest: null, error: '清单缺少 entry' })])
    plugins.select('balance-board')
    const wrapper = mountCard()
    expect(wrapper.find('.module-row__hint').text()).toContain('清单缺少 entry')
    expect(wrapper.find('.plugin-manager__reason').text()).toContain('清单缺少 entry')
  })
})

describe('安装的两步确认', () => {
  it('选完目录只 preview，确认之后才 install', async () => {
    const { plugins } = seed([])
    openMock.mockResolvedValue('C:/work/balance-board')
    invokeMock.mockImplementation((command: string) => {
      if (command === 'preview_plugin') return Promise.resolve(record())
      return Promise.resolve({ revision: 2, records: [record()] })
    })
    const wrapper = mountCard()
    await wrapper.get('.plugin-manager__drop').trigger('click')
    await flushPromises()

    const commands = invokeMock.mock.calls.map(call => call[0])
    expect(commands).toContain('preview_plugin')
    expect(commands).not.toContain('install_plugin')
    expect(wrapper.find('[role="dialog"]').exists()).toBe(true)

    // 披露里的域名与逐条文案都要在场，否则「同意」是对看不见的东西同意。
    const dialog = wrapper.get('[role="dialog"]').text()
    expect(dialog).toContain('api.example.com')
    expect(dialog).toContain('http 会碰到什么')

    await wrapper.findAll('[role="dialog"] button').at(1)!.trigger('click')
    await flushPromises()
    expect(invokeMock.mock.calls.map(call => call[0])).toContain('install_plugin')
    expect(plugins.records).toHaveLength(1)
  })

  it('取消就什么都不写', async () => {
    seed([])
    openMock.mockResolvedValue('C:/work/balance-board')
    invokeMock.mockImplementation((command: string) =>
      command === 'preview_plugin' ? Promise.resolve(record()) : Promise.resolve({ revision: 1, records: [] })
    )
    const wrapper = mountCard()
    await wrapper.get('.plugin-manager__drop').trigger('click')
    await flushPromises()
    await wrapper.findAll('[role="dialog"] button').at(0)!.trigger('click')
    expect(wrapper.find('[role="dialog"]').exists()).toBe(false)
    expect(invokeMock.mock.calls.map(call => call[0])).not.toContain('install_plugin')
  })

  it('披露里有一条 danger 时，确认钮先点不动', async () => {
    const plugins = usePluginsStore()
    plugins.registry = [fact('fs', 'danger')]
    openMock.mockResolvedValue('C:/work/x')
    invokeMock.mockImplementation((command: string) =>
      command === 'preview_plugin'
        ? Promise.resolve(record({ manifest: { ...record().manifest!, permissions: ['fs'] } }))
        : Promise.resolve({ revision: 1, records: [] })
    )
    const wrapper = mountCard()
    await wrapper.get('.plugin-manager__drop').trigger('click')
    await flushPromises()
    const confirm = wrapper.findAll('[role="dialog"] button').at(1)!
    expect(confirm.attributes('disabled')).toBeDefined()
  })
})

describe('卸载的两步确认', () => {
  it('第一下只武装，第二下才发命令', async () => {
    const { plugins } = seed([record()])
    invokeMock.mockResolvedValue({ revision: 2, records: [] })
    plugins.select('balance-board')
    const wrapper = mountCard()
    await wrapper.findAll('.plugin-manager__actions button').at(1)!.trigger('click')
    expect(invokeMock.mock.calls.map(call => call[0])).not.toContain('uninstall_plugin')
    expect(wrapper.find('.plugin-manager__actions').text()).toContain('确认卸载？')

    await wrapper.findAll('.plugin-manager__actions button').at(1)!.trigger('click')
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('uninstall_plugin', { id: 'balance-board' })
  })
})

describe('启用与热重载', () => {
  it('启用状态在这里只读不写：唯一的入口是上方组件列表那枚开关', () => {
    seed([record()])
    const wrapper = mountCard()
    // 组件包与内置模块共用同一张注册表，两处各放一枚开关就会互相看不见对方的变化。
    expect(wrapper.find('.module-row .base-switch').exists()).toBe(false)
    // 状态本身仍然看得出来（● 开着 / – 关着），只是这一行点不动。
    expect(wrapper.get('.plugin-manager__order').text()).toBe('●')
  })

  it('非本地源的包，热重载开关是禁用的', () => {
    const { plugins } = seed([record({ source: 'copied', devReload: false })])
    plugins.select('balance-board')
    const wrapper = mountCard()
    // 列表行没有开关了，卡里只剩调试卡这一枚热重载开关。
    const switches = wrapper.findAll('.base-switch input')
    expect(switches).toHaveLength(1)
    expect(switches[0]!.attributes('disabled')).toBeDefined()
  })

  it('本地源的开关一拨就写入安装记录', async () => {
    const { plugins } = seed([record()])
    invokeMock.mockResolvedValue({ revision: 2, records: [record({ devReload: true })] })
    plugins.select('balance-board')
    const wrapper = mountCard()
    await wrapper.findAll('.base-switch input').at(-1)!.setValue(true)
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('set_plugin_dev_reload', { id: 'balance-board', devReload: true })
  })
})

/**
 * 需求 5 的那一下：详情卡上的「拓展」要真的把独立日志窗开起来。
 *
 * 锁三件事：按**当前选中的包**开、没开热重载时这颗钮不该能点（关着就不留流水）、
 * 窗口没声明时把原因写进行内提示而不是静默。这些都是「用户以为自己点中了」的形状。
 */
describe('日志展开', () => {
  /** 按文案找那颗钮：不依赖 BaseButton 把属性挂在哪一层。 */
  function expandButton(wrapper: ReturnType<typeof mountCard>) {
    return wrapper.findAll('button').find(item => item.text().trim() === '拓展')
  }

  it('本地源 + 开着热重载时，点「拓展」按选中的包开日志窗', async () => {
    const { plugins } = seed([record({ devReload: true })])
    plugins.select('balance-board')
    const wrapper = mountCard()
    await flushPromises()
    const button = expandButton(wrapper)
    expect(button, '调试卡里没有「拓展」那颗钮').toBeTruthy()
    expect(button?.attributes('title')).toBe('打开完整流水')
    await button?.trigger('click')
    await flushPromises()
    expect(openLogMock).toHaveBeenCalledWith({ pluginId: 'balance-board', pluginName: '余额看板' })
    expect(plugins.actionError).toBe('')
    wrapper.unmount()
  })

  it('没开热重载时这颗钮是禁用的（关着就没有流水可看）', async () => {
    seed([record({ devReload: false })])
    const wrapper = mountCard()
    await flushPromises()
    const button = expandButton(wrapper)
    expect(button?.attributes('disabled'), '关着调试时这颗钮该禁用').toBeDefined()
    await button?.trigger('click')
    await flushPromises()
    expect(openLogMock).not.toHaveBeenCalled()
    wrapper.unmount()
  })

  it('窗口没声明时把原因写在行内，不是静默什么都不发生', async () => {
    openLogMock.mockResolvedValueOnce(false)
    const { plugins } = seed([record({ devReload: true })])
    const wrapper = mountCard()
    await flushPromises()
    await expandButton(wrapper)?.trigger('click')
    await flushPromises()
    expect(plugins.actionError, '开不出来要在行内说清').toContain('没在 tauri.conf.json 里声明')
    wrapper.unmount()
  })
})
