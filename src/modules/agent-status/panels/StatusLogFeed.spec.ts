// @vitest-environment happy-dom

/**
 * 「记录详情 · 转灯日志条目」的渲染回归：关闭态、折叠态、展开成浮层。
 *
 * 这一层之所以值得单独测：条目同时受三份状态影响（配置里的开关、store 的 peek 行、
 * store 的加载/降级态），任何一个读错都会显示成「没日志」，而用户没法分辨
 * 「没记」「没到」「读不到」—— 这三种必须各有自己的文案。
 */
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import StatusLogFeed from './StatusLogFeed.vue'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import { AGENT_STATUS_MODULE_ID } from '../settings'
import type { FloatingModuleConfig } from '@/lib/floating-modules'
import type { StatusLogEntry, StatusLogPage } from '../lib/status-log'
import { emptyLogUsage } from '../lib/status-log'

const { invokeMock, listenMock, saveMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
  saveMock: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock, default: {} }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: saveMock }))

/** 造一条记录（字段按线协议命名；`id` 是盘上身份，store 的合并去重靠它）。 */
function entry(seq: number, patch: Partial<StatusLogEntry> = {}): StatusLogEntry {
  return {
    seq,
    id: `2026-09-28.0.${seq}`,
    ts_ms: 1_790_000_000_000 + seq * 1000,
    writer: 'monitor',
    stage: 'kernel_verdict',
    reason: 'duplicate_in_seen_window',
    trace_id: 'c4-1',
    key: 'codex:s1',
    ...patch,
  } as StatusLogEntry
}

function page(entries: StatusLogEntry[]): StatusLogPage {
  return {
    head: entries[0]?.seq ?? 0,
    total: entries.length,
    dropped: 0,
    dir: 'C:\\log',
    usage: emptyLogUsage(),
    entries,
  }
}

/**
 * 浮层宿主：主窗把内容区标成 `#main-content`（`MainWindow.vue`），浮层 `Teleport` 到那里。
 * 测试环境没有主窗外壳，所以要自己摆一块 —— 目标不存在时 Teleport 会直接抛错。
 */
function ensureOverlayHost(): HTMLElement {
  const existing = document.getElementById('main-content')
  if (existing) return existing
  const host = document.createElement('div')
  host.id = 'main-content'
  host.className = 'main-content'
  document.body.append(host)
  return host
}

/**
 * 挂上组件并把配置灌进外壳 store。
 *
 * ⚠️ 两点都是这个仓库里踩过的（写法照 `sections/GeneralSection.spec.ts`）：
 * ① pinia 必须显式注入 `mount` —— 否则组件与测试各拿一份 store，配置怎么改界面都不动；
 * ② invoke 的假实现必须在 `mount` 之前装好：组件挂载即发首帧 peek（watch immediate）。
 */
async function mountFeed(on: boolean, entries = [entry(3), entry(2), entry(1)]) {
  ensureOverlayHost()
  invokeMock.mockImplementation(async (command: string) => {
    if (command === 'get_status_log') return page(entries)
    // 条数另有一条命令按盘数出来（精确值 + 截至时刻）；折叠条目那行读的是它，不是行数。
    if (command === 'count_status_log') {
      return { total: entries.length, as_of_ms: 1_790_000_000_000, scanned_bytes: 1024 }
    }
    return undefined
  })
  const pinia = createPinia()
  setActivePinia(pinia)
  useFloatingModulesStore().applyConfig({
    schema_version: 1,
    revision: 1,
    modules: [{ id: AGENT_STATUS_MODULE_ID, enabled: true, order: 0 }],
    module_settings: {
      [AGENT_STATUS_MODULE_ID]: on
        ? { log: { level: 'verbose', hook_enabled: true } }
        : { log: { level: 'off', hook_enabled: false } },
    },
  } as FloatingModuleConfig)
  const wrapper = mount(StatusLogFeed, { props: { sessionKey: 'codex:s1' }, attachTo: document.body, global: { plugins: [pinia] } })
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  listenMock.mockReset()
  listenMock.mockResolvedValue(() => {})
  saveMock.mockReset()
})

describe('StatusLogFeed', () => {
  it('says it is switched off, and offers no export or expand control', async () => {
    const wrapper = await mountFeed(false)
    expect(wrapper.text()).toContain('已关闭')
    expect(wrapper.text()).toContain('什么都不记')
    expect(wrapper.find('.lg-inline__expand').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('导出本会话')
    // 关闭态也要说清存档不受影响 —— 这是用户检验本次改动的第一句问话。
    expect(wrapper.text()).toContain('存档')
  })

  it('collapses to one headline plus two preview rows', async () => {
    const wrapper = await mountFeed(true)
    const rows = wrapper.findAll('.lg-inline__rows li')
    expect(rows).toHaveLength(2, '折叠态只给两行摘要，第三条是结论本身')
    expect(wrapper.text()).toContain('去重窗口内已见过同一个 event_id')
    expect(wrapper.text()).toContain('本会话 3 条')
    // 「截至」时刻必须一起给：它把一个会过期的精确数变成一句永远为真的历史事实。
    expect(wrapper.text()).toContain('截至')
    expect(wrapper.find('.lg-inline__expand').exists()).toBe(true)
    expect(wrapper.text()).toContain('导出本会话')
  })

  it('colors only the lamp characters, never the quoted prompt text', async () => {
    // 灯态那两个字必须染色（用户明确要过）；而「提问原文」里出现的同名字不许被染。
    const wrapper = await mountFeed(true, [
      entry(3, { stage: 'light_transition', reason: 'phase_changed', phase_from: 'running', phase_to: 'waiting' }),
    ])
    const colored = wrapper.findAll('.lg-light--green, .lg-light--yellow').map(node => node.text())
    expect(colored).toEqual(['绿', '黄'])
  })

  it('expands into the two-pane overlay and shows every field of the picked row', async () => {
    const wrapper = await mountFeed(true)
    expect(document.querySelector('.lg-modal')).toBeNull()
    await wrapper.find('.lg-inline__expand').trigger('click')
    await flushPromises()
    const modal = document.querySelector('.lg-modal')
    // ⚠️ 宿主必须是内容区 `#main-content`，不是 `body`：挂在 body 上时浮层按视口铺满，
    // 实测会盖住一级 / 二级标签页，方角还伸出 `.main-shell` 的圆角之外（比窗口还大）。
    expect(modal, '浮层要 Teleport 进主窗内容区宿主').not.toBeNull()
    expect(modal?.parentElement?.id, '浮层的父节点必须是 #main-content').toBe('main-content')
    expect(modal?.closest('.main-window'), '浮层不许落在滚动区里面').toBeNull()
    expect(modal.querySelectorAll('.lg-split > *').length).toBe(2, '左右两栏')
    expect(modal.textContent).toContain('c4-1')
    // 行尾与右栏标的都是**盘上坐标**（`文件序号@字节偏移`），不再有进程内游标那种假身份。
    expect(modal.textContent).toMatch(/\d+@\d+/)
    expect(modal.textContent).toContain('盘上的字节坐标')

    // 「完成」收回浮层，条目本身还在。
    const done = Array.from(modal.querySelectorAll('button')).find(b => b.textContent?.includes('完成'))
    expect(done).toBeTruthy()
    done?.click()
    await flushPromises()
    expect(document.querySelector('.lg-modal')).toBeNull()
    expect(wrapper.find('.lg-inline').exists()).toBe(true)
  })

  it('does not fire an export when the save dialog is cancelled', async () => {
    const wrapper = await mountFeed(true)
    saveMock.mockResolvedValue(null)
    invokeMock.mockClear()
    const exportButton = wrapper.findAll('button').find(b => b.text().includes('导出本会话'))
    await exportButton?.trigger('click')
    await flushPromises()
    expect(saveMock).toHaveBeenCalledTimes(1)
    // 取消即不导出：不许发 `export_status_log`，也不许报错误。
    expect(invokeMock.mock.calls.some(call => call[0] === 'export_status_log')).toBe(false)
    expect(wrapper.text()).not.toContain('导出失败')
  })

  /**
   * 导出「按盘扫全量」的两条前端侧验收：
   * - 入参不许再带 `limit`：这条命令的语义已经变成「按当前条件把盘上命中的全部带走」，
   *   传一个 5000 进去就是继续把旧的内存天花板冒充成上限；
   * - 扫描在飞的时候必须挂遮罩 —— 否则用户以为没点上，会再点一次，多扫一遍全目录。
   */
  it('exports the whole on-disk match without a client-side limit, with a loading overlay', async () => {
    const wrapper = await mountFeed(true)
    saveMock.mockResolvedValue('X:\\out.jsonl')
    let finishExport: (value: unknown) => void = () => {}
    invokeMock.mockImplementation((command: string) => {
      if (command === 'export_status_log') {
        return new Promise(resolve => {
          finishExport = resolve
        })
      }
      return Promise.resolve(null)
    })

    const exportButton = wrapper.findAll('button').find(b => b.text().includes('导出本会话'))
    await exportButton?.trigger('click')
    await flushPromises()

    const call = invokeMock.mock.calls.find(item => item[0] === 'export_status_log')
    expect((call?.[1] as { query?: Record<string, unknown> })?.query).toMatchObject({ key: 'codex:s1' })
    expect((call?.[1] as { query?: Record<string, unknown> }).query?.limit).toBeUndefined()
    expect(wrapper.find('.base-loading-overlay').exists()).toBe(true)

    finishExport({
      path: 'X:\\out.jsonl',
      entries: 9123,
      scanned_bytes: 4_000_000,
      stopped_by_budget: false,
    })
    await flushPromises()
    expect(wrapper.find('.base-loading-overlay').exists()).toBe(false)
    expect(wrapper.text()).toContain('已导出 9123 条（盘上命中全量）')
  })

  // 被扫描预算截断时不许只报条数：那会让人以为拿到的是全部。
  it('says so when the export was cut short by the scan budget', async () => {
    const wrapper = await mountFeed(true)
    saveMock.mockResolvedValue('X:\\out.jsonl')
    invokeMock.mockImplementation((command: string) =>
      Promise.resolve(
        command === 'export_status_log'
          ? { path: 'X:\\out.jsonl', entries: 500, scanned_bytes: 512 * 1024 * 1024, stopped_by_budget: true }
          : null
      )
    )
    const exportButton = wrapper.findAll('button').find(b => b.text().includes('导出本会话'))
    await exportButton?.trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('不完整')
  })
})
