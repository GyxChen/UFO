/**
 * 组件包能拿到的宿主能力：`host`。
 *
 * 三条硬规矩：
 * 1. **只暴露这一个对象**。不给 `invoke`、不给 `Channel`、不给任何 `@tauri-apps/*` 原物 ——
 *    否则徽标就只是装饰，任何人都能直接调命令。真正的闸在 Rust 命令层
 *    （每条命令第一行按 pluginId 查清单），这里做的只是「用得顺 + 说不清就抛」。
 * 2. **没声明的能力一用就抛**，消息里点名该在清单的 `permissions` 里加什么。
 *    （抛错比塞一个 `undefined` 有用：表现是「作者立刻知道自己没声明」，不是「代码静默不工作」。）
 * 3. 对象整体 `Object.freeze`，属性也是冻结的：组件改不动别人的 host。
 */

import { invoke } from '@tauri-apps/api/core'
import { PLUGIN_LIMITS } from '@/lib/plugin-limits'
// 只在值方向用它：`plugin-runtime.ts` 反过来只 `import type` 本文件，所以不构成运行时环。
import { PLUGIN_ORIGIN } from '@/lib/plugin-runtime'
import type { PluginRecordJson } from '@/lib/plugin-types'
import { setPluginTooltipSize } from './plugin-tooltip-size'

/** 组件用 `inject(PLUGIN_HOST_KEY)` 取到自己的 host（作者 SDK 里 re-export 这一个符号）。 */
export const PLUGIN_HOST_KEY = Symbol.for('ufo.plugin.host')

/** `host.log` 的三个级别（与日志窗的过滤档同名）。 */
export type PluginLogLevel = 'info' | 'warn' | 'error'

/** 一次出网请求的可选项。 */
export interface PluginHttpOptions {
  /** 请求头（`Host` 一类逐跳头由宿主剥掉）。 */
  headers?: Record<string, string>
  /** 超时（毫秒），宿主夹在 500–60000。 */
  timeoutMs?: number
}

/** 出网的返回。 */
export interface PluginHttpResult {
  status: number
  body: string
  headers: Record<string, string>
  /** 体积到上限被截过：不藏，组件自己决定怎么办。 */
  truncated: boolean
}

/** `host.storage.usage()` 的读数。 */
export interface PluginStorageUsage {
  bytes: number
  limit: number
  keys: number
  keyLimit: number
}

/** 一次「用户亲手选中的文件」（句柄只在这次启动里活着）。 */
export interface PluginPickedFile {
  /** 后续读都靠它。 */
  handle: string
  /** 文件名（显示与扩展名判断用）。 */
  name: string
  /** 总字节数。 */
  size: number
  /** 宿主认不认得出这是压缩包（zip 家族）。 */
  archive: boolean
  /** 整本的直链：给 `fetch(url, {headers:{Range:'bytes=…'}})` 或 `<iframe src>` 用。 */
  url: string
}

/** 压缩包目录表里的一项。 */
export interface PluginZipEntry {
  /** 条目名（只当标识符，不是文件系统路径）。 */
  name: string
  /** 解压后的字节数。 */
  size: number
}

/** 私有目录里的一项。 */
export interface PluginPrivateFile {
  /** 相对私有目录的路径。 */
  name: string
  /** 字节数。 */
  size: number
}

/** 私有目录的占用读数。 */
export interface PluginFsUsage {
  bytes: number
  limit: number
  files: number
}

/** 命令层返回的裸形状（`url` 是这里补的便利字段）。 */
interface PickedJson {
  handle: string
  name: string
  size: number
  archive: boolean
}

/** 扫到的一台蓝牙外设。 */
export interface PluginBleDevice {
  /** 连接用它；只在这次启动里有效。 */
  handle: string
  /** 设备地址（给人看的）。 */
  address: string
  /** 广播名；没有就是空串。 */
  name: string
  /** 信号强度；平台没给就 null。 */
  rssi: number | null
  /** 广播包里解析出来的字段，宿主一个不丢（值是合并后的最新快照，不是逐个包）。 */
  advert: PluginBleAdvertisement
}

/** 一次广播里解析出来的字段。 */
export interface PluginBleAdvertisement {
  /** 平台给的广播名（Windows 侧来自 WinRT 的 LocalName()）。 */
  advertisementName: string | null
  /** AD 结构里的完整名 / 简短名：与上面是两个来源，可以不一样。 */
  localName: string | null
  /** GAP appearance（蓝牙规范给设备分类的 16 位数）。 */
  appearance: number | null
  /** 广播里的发射功率（dBm），和 rssi 一起用才能折算距离。 */
  txPower: number | null
  /** 地址类型：`public` / `random` / `unknown`；随机地址重启就变，别当设备身份用。 */
  addressType: string | null
  /** 经典蓝牙的设备类别。只有 BlueZ 会填，Windows 恒为 null。 */
  deviceClass: number | null
  /** 广播里声明的服务 UUID（升序、128 位写法）。 */
  serviceUuids: string[]
  /** 厂商自定义数据（AD 类型 0xFF），按 companyId 升序，值是原始字节。 */
  manufacturerData: PluginBleManufacturerData[]
  /** 按服务带的数据（AD 类型 0x16 / 0x20 / 0x21），按 UUID 升序。 */
  serviceData: PluginBleServiceData[]
}

/** 一条厂商自定义数据。 */
export interface PluginBleManufacturerData {
  /** 公司编号（蓝牙联盟分配）。 */
  companyId: number
  /** 原始字节（宿主不解释内容，iBeacon 这类格式自己解）。 */
  data: Uint8Array
}

/** 一条按服务带的数据。 */
export interface PluginBleServiceData {
  /** 服务 UUID。 */
  uuid: string
  /** 原始字节。 */
  data: Uint8Array
}

/** GATT 特征一行。 */
export interface PluginBleCharacteristic {
  uuid: string
  /** 属性名（read / write / writeWithoutResponse / notify / indicate / broadcast）。 */
  properties: string[]
}

/** GATT 服务一行。 */
export interface PluginBleService {
  uuid: string
  primary: boolean
  characteristics: PluginBleCharacteristic[]
}

/** 一次蓝牙连接的返回。 */
export interface PluginBleConnection {
  handle: string
  address: string
  name: string
  mtu: number | null
  services: PluginBleService[]
}

/** 一条蓝牙通知（值已经是字节，不用作者自己解 base64）。 */
export interface PluginBleNotification {
  uuid: string
  serviceUuid: string
  value: Uint8Array
  /** 这一条之前环形里还压着几条（拉得慢会丢最旧的，这里能看出来）。 */
  queuedBefore: number
}

/** 命令层的裸通知形状。 */
interface BleNotificationJson {
  uuid: string
  serviceUuid: string
  value: string
  queuedBefore: number
}

/** 命令层的一台外设（广播里的字节段是 base64）。 */
interface BleDeviceJson {
  handle: string
  address: string
  name: string
  rssi: number | null
  advert: BleAdvertisementJson
}

/** 命令层的广播字段。 */
interface BleAdvertisementJson {
  advertisementName: string | null
  localName: string | null
  appearance: number | null
  txPower: number | null
  addressType: string | null
  deviceClass: number | null
  serviceUuids: string[]
  manufacturerData: { companyId: number; data: string }[]
  serviceData: { uuid: string; data: string }[]
}

/** 广播的字节段只有命令层是 base64：作者侧一律拿到 Uint8Array。 */
function decodeAdvertisement(json: BleAdvertisementJson): PluginBleAdvertisement {
  return {
    ...json,
    manufacturerData: json.manufacturerData.map(row => ({
      companyId: row.companyId,
      data: decodeBase64(row.data),
    })),
    serviceData: json.serviceData.map(row => ({ uuid: row.uuid, data: decodeBase64(row.data) })),
  }
}

/** 一个可打开的串口。 */
export interface PluginSerialPort {
  name: string
  /** 来源描述（`USB 1a86:7523` 这类）。 */
  detail: string
}

/** 一次串口打开的返回。 */
export interface PluginSerialHandle {
  handle: string
  name: string
  /** 实际生效的波特率（被夹取过也如实报）。 */
  baud: number
}

/** 把句柄拼成整本直链（`__pick` 是宿主的保留入口）。 */
function pickUrl(handle: string): string {
  return `${PLUGIN_ORIGIN}/__pick/${encodeURIComponent(handle)}`
}

/** 把压缩包里的某一项拼成可寻址的直链（条目名走 query，逐段编码保住 `/`）。 */
function entryUrlOf(handle: string, entry: string): string {
  return `${pickUrl(handle)}?entry=${encodeURIComponent(entry)}`
}

/** 私有目录里某个文件的直链。 */
function privateUrlOf(pluginId: string, path: string): string {
  const segments = path
    .split('/')
    .filter(Boolean)
    .map(segment => encodeURIComponent(segment))
  return `${PLUGIN_ORIGIN}/__fs/${encodeURIComponent(pluginId)}/${segments.join('/')}`
}

/** base64 → 字节（命令层搬二进制走的就是这一串文本）。 */
function decodeBase64(text: string): Uint8Array {
  const binary = atob(text)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index)
  }
  return bytes
}

/** 字节 → base64。 */
function encodeBase64(bytes: Uint8Array): string {
  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return btoa(binary)
}

/** 字符串按 utf8 编字节、字节原样：写外设时作者不该自己碰 base64。 */
function toBytes(data: Uint8Array | string): Uint8Array {
  return typeof data === 'string' ? new TextEncoder().encode(data) : data
}

/** 一条灯态事件（与 Rust `AgentEvent` 同形）。 */
export interface PluginAgentEvent {
  seq: number
  ts: number
  topic: string
  key: string
  provider: string
  sessionId: string
  phase: string
  previousPhase: string | null
  endpoint: string | null
  prompt: string | null
  startedAtMs: number
  lastSeenAtMs: number
  errorCode: string | null
  waitingReason: string | null
}

/** 组件包看到的宿主对象。 */
export interface PluginHost {
  /** 这个包自己的身份与声明（只读）。 */
  readonly plugin: {
    id: string
    name: string
    version: string
    description: string
    permissions: readonly string[]
    hosts: readonly string[]
  }
  /** 打一行日志（需求点名的特例：不需要声明权限）。 */
  log(level: PluginLogLevel, message: string, fields?: Record<string, string | number | boolean>): void
  /** 按包隔离的持久化存储。 */
  storage: {
    get(key: string): Promise<unknown>
    set(key: string, value: unknown): Promise<void>
    remove(key: string): Promise<boolean>
    keys(): Promise<string[]>
    usage(): Promise<PluginStorageUsage>
  }
  /** 出网（只走宿主，且只到清单 `hosts` 里的域名）。 */
  http: {
    request(method: string, url: string, body?: string, options?: PluginHttpOptions): Promise<PluginHttpResult>
    get(url: string, options?: PluginHttpOptions): Promise<PluginHttpResult>
    post(url: string, body: string, options?: PluginHttpOptions): Promise<PluginHttpResult>
  }
  /**
   * 文件（清单要声明 `fs`）。
   *
   * 两条路各自的边界：**系统里的文件只能由用户亲手在对话框里选**（拿到的是会话内句柄，
   * 重启就失效），**私有目录里随便写**（`plugins/<id>/files`，卸载即删）。
   * 大块二进制请走 `url` / `entryUrl` / `privateUrl` —— 那三条让浏览器自己分段取，
   * 不必把整本书 base64 进内存。
   */
  fs: {
    /** 弹一次原生对话框，返回用户选中的那一个文件。 */
    pick(title?: string): Promise<PluginPickedFile>
    /** 句柄此刻还有效吗（文件被挪走 / 重启过就会失败，据此提示重新选）。 */
    stat(handle: string): Promise<PluginPickedFile>
    /** 压缩包的目录表（条目名只当标识符）。 */
    entries(handle: string): Promise<PluginZipEntry[]>
    /** 读选中文件的一段（最多 4 MiB 一刀）。 */
    read(handle: string, start: number, len: number): Promise<Uint8Array>
    /** 读压缩包里某一项的一段。⚠️ deflate 条目不能从中间开始解：`start` 大就是从头解到那。 */
    readEntry(handle: string, entry: string, start: number, len: number): Promise<Uint8Array>
    /** 某一项的直链（给 `<iframe src>` 用：不必在项目里带任何 zip 库）。 */
    entryUrl(handle: string, entry: string): string
    /** 写私有目录：字符串按 utf8，字节按 base64 上传。 */
    write(path: string, data: string | Uint8Array): Promise<PluginPrivateFile>
    /** 读私有目录里某个文件的一段。 */
    readPrivate(path: string, start: number, len: number): Promise<Uint8Array>
    /** 私有目录里某个文件的直链。 */
    privateUrl(path: string): string
    /** 列私有目录。 */
    list(): Promise<PluginPrivateFile[]>
    /** 删私有目录里一个文件（不存在回 false）。 */
    remove(path: string): Promise<boolean>
    /** 这个包的私有目录占用。 */
    usage(): Promise<PluginFsUsage>
  }
  /**
   * 密钥（清单要声明 `secrets`）：值只进出 OS 凭据管理器，
   * 不落盘、不进日志、界面上不回显；盘上只记「这个名字被这个包用过」，卸载时按名册删干净。
   */
  secrets: {
    /** 按名字取值；没配过回 null（不是错误）。 */
    get(name: string): Promise<string | null>
    /** 写值（组件自己的设置页里收）。 */
    set(name: string, value: string): Promise<void>
    /** 删值。 */
    remove(name: string): Promise<void>
    /** 这个包存过哪些名字（只有名字，绝不回值）。 */
    names(): Promise<string[]>
  }
  /**
   * 内嵌网页（清单要声明 `frame`）。
   *
   * 宿主只回答「这个地址能不能嵌」，真正的嵌入动作是作者界面里的 `<iframe src>` ——
   * 刻意不为组件开新窗口：那样会绕开「声明 + 隐藏 + 按需显示」的窗口规矩，也把
   * 一个能显示任意网页、又能拿 host 的独立上下文留在屏幕外。
   */
  frame: {
    /**
     * 过闸后回规范化后的 host；不通过就抛。
     *
     * 嵌网页只放 https（比 `host.http` 严一档）：宿主的 CSP `frame-src` 也只放 https，
     * 两边判据必须一致，否则坏法是「闸说可以、iframe 一片空白 + 一句 CSP 报错」。
     */
    allow(url: string): Promise<string>
  }
  /**
   * 自己的信息卡（清单声明 `surfaces.tooltip.own` 才有这张卡）。
   *
   * 卡就渲染在本窗口里，所以这条不碰 IPC：只把尺寸诉求交给宿主，由提示窗套用。
   * ⚠️ 区间与清单同一对常量（超了会被夹），提示窗另外还会按工作区夹一次。
   */
  tooltip: {
    /** 申请这张卡的尺寸（物理像素），返回实际生效的那一份。 */
    setSize(size: { width: number; height: number }): { width: number; height: number }
  }
  /**
   * 蓝牙（清单要声明 `ble`）。
   *
   * 连接、GATT 树与通知队列都**常驻在宿主**：组件的 UI 会随热重载 / 窗口隐藏重建，
   * 「重建就断连」不可接受，所以组件看到的只是按句柄拉取的接口。
   * 句柄不跨启动，卸载 / 停用即断开 —— 界面上那句披露说的是同一件事。
   */
  ble: {
    /** 扫一会儿（默认 6 秒，宿主夹在 1–20 秒），返回扫到的外设。 */
    scan(ms?: number): Promise<PluginBleDevice[]>
    /** 连上扫描结果里的某一台，顺手把 GATT 树读回来。 */
    connect(scanHandle: string): Promise<PluginBleConnection>
    /** 写一个特征（字符串按 utf8、字节按原样；单次上限 512 字节，长数据自己分包）。 */
    write(
      handle: string,
      service: string,
      characteristic: string,
      data: Uint8Array | string,
      withResponse?: boolean
    ): Promise<number>
    read(handle: string, service: string, characteristic: string): Promise<Uint8Array>
    /** 开一条通知订阅（之后用 `notifications` 取）。 */
    subscribe(handle: string, characteristic: string): Promise<void>
    unsubscribe(handle: string, characteristic: string): Promise<void>
    /** 取走通知，最旧的先给。 */
    notifications(handle: string, limit?: number): Promise<PluginBleNotification[]>
    disconnect(handle: string): Promise<boolean>
    /** 这个包此刻连着哪些外设地址。 */
    held(): Promise<string[]>
  }
  /**
   * 串口（清单要声明 `serial`）。端口被占用期间其它程序拿不到它，所以用完要 `close`。
   * 端口名只认设备名形状（`COM3` 或 `/dev/tty…`），这一层不接受任意路径。
   */
  serial: {
    ports(): Promise<PluginSerialPort[]>
    open(path: string, baud: number, timeoutMs?: number): Promise<PluginSerialHandle>
    write(handle: string, data: Uint8Array | string): Promise<number>
    /** 读最多 `max` 字节；这一轮没数据回空数组（轮询取数据是正常形状，不是错误）。 */
    read(handle: string, max: number): Promise<Uint8Array>
    setTimeout(handle: string, timeoutMs: number): Promise<number>
    close(handle: string): Promise<boolean>
    held(): Promise<string[]>
  }
  /** 灯态事件（订阅即开始按秒拉增量）。 */
  event: {
    /** 开始订阅；清单没声明 `events:agent` 会抛。返回可订阅的主题。 */
    open(): Promise<string[]>
    /** 停止订阅并清掉轮询。 */
    close(): Promise<void>
    /** 每条新事件回调一次；返回取消函数。 */
    subscribe(handler: (event: PluginAgentEvent) => void): () => void
  }
}

/** 造一个包的 host（装载入口时调一次，一个包一份）。 */
export function createPluginHost(record: PluginRecordJson): PluginHost {
  const manifest = record.manifest
  const permissions = new Set(manifest?.permissions ?? [])
  const id = record.id
  /** 声明检查：没声明就抛，消息点名要加哪一项。 */
  const require = (permission: string) => {
    if (!permissions.has(permission)) {
      throw new Error(`清单的 permissions 里没有 "${permission}"，用不了这个能力`)
    }
  }
  let pollTimer: ReturnType<typeof setInterval> | undefined
  let cursor = 0

  /** 订阅者集合（`subscribe` 加、`close` 清）。 */
  const handlers = new Set<(event: PluginAgentEvent) => void>()

  /**
   * 出网这一档单独成对象：`get / post` 要转调同层的 `request`，
   * 而写在 host 字面量里只能靠 `this` —— 组件常把方法拆下来解构（`const { get } = host.http`），
   * 拆开后 this 就没了。具名引用这个对象才受得住解构。
   */
  const httpApi: PluginHost['http'] = {
    async request(method, url, body, options) {
      require('http')
      return await invoke<PluginHttpResult>('plugin_http_fetch', {
        id,
        request: {
          method: String(method).toUpperCase(),
          url,
          headers: options?.headers ?? {},
          body: body ?? null,
          timeoutMs: options?.timeoutMs ?? 10_000,
        },
      })
    },
    get(url, options) {
      return httpApi.request('GET', url, undefined, options)
    },
    post(url, body, options) {
      return httpApi.request('POST', url, body, options)
    },
  }

  const host: PluginHost = {
    plugin: Object.freeze({
      id: record.id,
      name: record.name,
      version: record.version,
      description: manifest?.description ?? '',
      permissions: Object.freeze([...permissions]),
      hosts: Object.freeze([...(manifest?.hosts ?? [])]),
    }),
    log(level, message, fields) {
      // 特例：不留徽标、不查权限。
      // 日志窗的过滤档只认这三值，不认识的级别在这里夹成 info —— 否则那一行会「写了但任何档都筛不到」。
      const known = level === 'warn' || level === 'error' ? level : 'info'
      if (known !== level) {
        console.warn(`[plugin ${id}] 未知日志级别「${level}」，按 info 记录`)
      }
      void invoke('plugin_log_write', {
        id,
        level: known,
        msg: String(message ?? ''),
        fields: fields ?? null,
      }).catch(error => console.warn(`[plugin ${id}] 日志没写进去`, error))
    },
    storage: {
      async get(key) {
        require('storage')
        return await invoke<unknown | null>('plugin_storage_get', { id, key })
      },
      async set(key, value) {
        require('storage')
        await invoke('plugin_storage_set', { id, key, value })
      },
      async remove(key) {
        require('storage')
        return await invoke<boolean>('plugin_storage_remove', { id, key })
      },
      async keys() {
        require('storage')
        return await invoke<string[]>('plugin_storage_keys', { id })
      },
      async usage() {
        require('storage')
        return await invoke<PluginStorageUsage>('get_plugin_storage_usage', { id })
      },
    },
    http: httpApi,
    fs: {
      async pick(title) {
        require('fs')
        const picked = await invoke<PickedJson>('plugin_fs_pick', { id, title: title ?? null })
        return { ...picked, url: pickUrl(picked.handle) }
      },
      async stat(handle) {
        require('fs')
        const picked = await invoke<PickedJson>('plugin_fs_stat', { id, handle })
        return { ...picked, url: pickUrl(picked.handle) }
      },
      async entries(handle) {
        require('fs')
        return await invoke<PluginZipEntry[]>('plugin_fs_entries', { id, handle })
      },
      async read(handle, start, len) {
        require('fs')
        return decodeBase64(await invoke<string>('plugin_fs_read', { id, handle, start, len }))
      },
      async readEntry(handle, entry, start, len) {
        require('fs')
        return decodeBase64(
          await invoke<string>('plugin_fs_read_entry', { id, handle, entry, start, len })
        )
      },
      entryUrl(handle, entry) {
        require('fs')
        return entryUrlOf(handle, entry)
      },
      async write(path, data) {
        require('fs')
        const text = typeof data === 'string' ? data : encodeBase64(data)
        return await invoke<PluginPrivateFile>('plugin_fs_write', {
          id,
          path,
          data: text,
          encoding: typeof data === 'string' ? 'utf8' : 'base64',
        })
      },
      async readPrivate(path, start, len) {
        require('fs')
        return decodeBase64(await invoke<string>('plugin_fs_read_private', { id, path, start, len }))
      },
      privateUrl(path) {
        require('fs')
        return privateUrlOf(id, path)
      },
      async list() {
        require('fs')
        return await invoke<PluginPrivateFile[]>('plugin_fs_list', { id })
      },
      async remove(path) {
        require('fs')
        return await invoke<boolean>('plugin_fs_remove', { id, path })
      },
      async usage() {
        require('fs')
        return await invoke<PluginFsUsage>('get_plugin_fs_usage', { id })
      },
    },
    secrets: {
      async get(name) {
        require('secrets')
        return await invoke<string | null>('plugin_secret_get', { id, name })
      },
      async set(name, value) {
        require('secrets')
        await invoke('plugin_secret_set', { id, name, value })
      },
      async remove(name) {
        require('secrets')
        await invoke('plugin_secret_remove', { id, name })
      },
      async names() {
        require('secrets')
        return await invoke<string[]>('get_plugin_secret_names', { id })
      },
    },
    frame: {
      async allow(url) {
        require('frame')
        return await invoke<string>('plugin_frame_allow', { id, url })
      },
    },
    tooltip: {
      setSize(size) {
        // 授权来自挂载点声明本身：没有这张卡就没有可改的尺寸。
        if (manifest?.surfaces.tooltip?.own !== true) {
          throw new Error('没声明 surfaces.tooltip.own：这个包没有自己的信息卡，改不了它的尺寸')
        }
        if (!Number.isFinite(size?.width) || !Number.isFinite(size?.height)) {
          throw new Error('setSize 要的是 { width, height } 两个数（物理像素）')
        }
        return setPluginTooltipSize(id, { width: size.width, height: size.height })
      },
    },
    ble: {
      async scan(ms) {
        require('ble')
        const rows = await invoke<BleDeviceJson[]>('plugin_ble_scan', { id, ms: ms ?? null })
        return rows.map(row => ({ ...row, advert: decodeAdvertisement(row.advert) }))
      },
      async connect(scanHandle) {
        require('ble')
        return await invoke<PluginBleConnection>('plugin_ble_connect', { id, handle: scanHandle })
      },
      async write(handle, service, characteristic, data, withResponse) {
        require('ble')
        return await invoke<number>('plugin_ble_write', {
          id,
          handle,
          service,
          characteristic,
          data: encodeBase64(toBytes(data)),
          withResponse: withResponse ?? true,
        })
      },
      async read(handle, service, characteristic) {
        require('ble')
        return decodeBase64(await invoke<string>('plugin_ble_read', { id, handle, service, characteristic }))
      },
      async subscribe(handle, characteristic) {
        require('ble')
        await invoke('plugin_ble_subscribe', { id, handle, characteristic })
      },
      async unsubscribe(handle, characteristic) {
        require('ble')
        await invoke('plugin_ble_unsubscribe', { id, handle, characteristic })
      },
      async notifications(handle, limit) {
        require('ble')
        const rows = await invoke<BleNotificationJson[]>('plugin_ble_notifications', {
          id,
          handle,
          limit: limit ?? 32,
        })
        return rows.map(row => ({
          uuid: row.uuid,
          serviceUuid: row.serviceUuid,
          value: decodeBase64(row.value),
          queuedBefore: row.queuedBefore,
        }))
      },
      async disconnect(handle) {
        require('ble')
        return await invoke<boolean>('plugin_ble_disconnect', { id, handle })
      },
      async held() {
        require('ble')
        return await invoke<string[]>('get_plugin_ble_held', { id })
      },
    },
    serial: {
      async ports() {
        require('serial')
        return await invoke<PluginSerialPort[]>('plugin_serial_ports', { id })
      },
      async open(path, baud, timeoutMs) {
        require('serial')
        return await invoke<PluginSerialHandle>('plugin_serial_open', {
          id,
          path,
          baud,
          timeoutMs: timeoutMs ?? null,
        })
      },
      async write(handle, data) {
        require('serial')
        return await invoke<number>('plugin_serial_write', { id, handle, data: encodeBase64(toBytes(data)) })
      },
      async read(handle, max) {
        require('serial')
        return decodeBase64(await invoke<string>('plugin_serial_read', { id, handle, max }))
      },
      async setTimeout(handle, timeoutMs) {
        require('serial')
        return await invoke<number>('plugin_serial_timeout', { id, handle, timeoutMs })
      },
      async close(handle) {
        require('serial')
        return await invoke<boolean>('plugin_serial_close', { id, handle })
      },
      async held() {
        require('serial')
        return await invoke<string[]>('get_plugin_serial_held', { id })
      },
    },
    event: {
      async open() {
        require('events:agent')
        const topics = await invoke<string[]>('open_plugin_events', { id })
        if (!pollTimer) {
          // 按包拉增量（不广播）：闸在 Rust，拉的动作本身就是「这个包在要事件」。
          pollTimer = setInterval(() => {
            void drain()
          }, PLUGIN_LIMITS.EVENT_POLL_INTERVAL_MS)
        }
        return topics
      },
      async close() {
        if (pollTimer !== undefined) {
          clearInterval(pollTimer)
          pollTimer = undefined
        }
        handlers.clear()
        await invoke('close_plugin_events', { id }).catch(() => undefined)
      },
      subscribe(handler) {
        handlers.add(handler)
        return () => handlers.delete(handler)
      },
    },
  }

  /** 一次拉取：把游标之后的事件分给订阅者。 */
  async function drain() {
    try {
      const batch = await invoke<{ events: PluginAgentEvent[]; nextSeq: number }>('poll_plugin_events', {
        id,
        since: cursor,
      })
      cursor = batch.nextSeq
      for (const event of batch.events) {
        for (const handler of handlers) {
          try {
            handler(event)
          } catch (error) {
            // 一个订阅者抛错不能带走别人。
            console.warn(`[plugin ${id}] 事件回调抛错`, error)
          }
        }
      }
    } catch (error) {
      // 拉不到不算组件的错（比如包被停用）：停轮询，别一秒抛一次刷日志。
      console.warn(`[plugin ${id}] 事件拉取停了`, error)
      if (pollTimer !== undefined) {
        clearInterval(pollTimer)
        pollTimer = undefined
      }
    }
  }

  return deepFreeze(host)
}

/** 逐层冻结（一层不够：`host.storage.set = ...` 也是能改的）。 */
function deepFreeze<T extends object>(value: T): T {
  Object.freeze(value)
  for (const key of Object.keys(value)) {
    const child = (value as Record<string, unknown>)[key]
    if (child && typeof child === 'object') Object.freeze(child)
  }
  return value
}
