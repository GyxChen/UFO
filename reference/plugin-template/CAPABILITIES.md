<!-- 生成物：由 UFO 的 `/create-plugin-template` 命令整理，别手改 —— 宿主能力面一变，
     src/lib/plugin-template-doc.spec.ts 就会红，那时重跑那条命令覆盖这份。 -->

# 这份宿主当前提供什么

- 快照：宿主插件契约 API **v1**、清单格式 **v1**、8 项能力、`host` 上 10 个命名空间 + 1 个顶层方法、入口 8 个字段。
- 版本取自 `src-tauri/src/plugin/paths.rs`；能力与装前披露原文取自 `src-tauri/src/plugin/capability.rs`；
  签名取自 `src/lib/plugin-host.ts` 的 `PluginHost`；「哪一档要哪份权限」取自每个命名空间里的 `require(…)`；
  入口字段取自 `src/lib/plugin-loader.ts` 实际读的 `entry.x`。这份文档不写任何宿主没实现的东西。

用法：`const host = useHost()` 必须在组件的 `<script setup>` 里取（`src/host.js`），
或在 `activate(host)` 的回调参数上拿那一份。不要在模块顶层 `inject`。

## 清单 `permissions` 能写哪些名字

写没登记过的名字 = 装的时候直接拒；登记了却在这一版宿主上找不到对应 `host` 入口 = 清单写了也用不了
（这种不一致是 `/create-plugin-template` 的对账项，会当场报出来）。

| 名字 | `host` 上的入口 | 档 | 本模板演示了 |
| --- | --- | --- | --- |
| `events:agent` | `host.event` | danger | 是 |
| `fs` | `host.fs` | danger | 是 |
| `http` | `host.http` | caution | 否 |
| `secrets` | `host.secrets` | caution | 否 |
| `ble` | `host.ble` | caution | 否 |
| `serial` | `host.serial` | caution | 否 |
| `frame` | `host.frame` | caution | 否 |
| `storage` | `host.storage` | local | 是 |

## 各能力的 API

参数类型照 `PluginHost` 接口原文。每一档都要先在清单 `permissions` 里写上对应名字，否则调用一用就抛。

### `host.storage` —— 需要 `storage`

```js
await host.storage.get(key)
await host.storage.set(key, value)
await host.storage.remove(key)
await host.storage.keys()
await host.storage.usage()
```

- `get(key: string): Promise<unknown>`
- `set(key: string, value: unknown): Promise<void>`
- `remove(key: string): Promise<boolean>`
- `keys(): Promise<string[]>`
- `usage(): Promise<PluginStorageUsage>`

### `host.http` —— 需要 `http`

```js
await host.http.request(method, url, body, options)
await host.http.get(url, options)
await host.http.post(url, body, options)
```

- `request(method: string, url: string, body?: string, options?: PluginHttpOptions): Promise<PluginHttpResult>` —— 发一次宿主代发的请求：非 2xx **也照样 resolve**，判 `result.status`（401 / 403 常表意「凭据失效」，抛错会把它吞成「没网」）。
- `get(url: string, options?: PluginHttpOptions): Promise<PluginHttpResult>` —— GET 的便捷写法：与 `request` 同一判据，非 2xx 也 resolve。
- `post(url: string, body: string, options?: PluginHttpOptions): Promise<PluginHttpResult>` —— POST 一段文本体：与 `request` 同一判据，非 2xx 也 resolve。

### `host.fs` —— 需要 `fs`

```js
await host.fs.pick(title)
await host.fs.stat(handle)
await host.fs.entries(handle)
await host.fs.read(handle, start, len)
await host.fs.readEntry(handle, entry, start, len)
host.fs.entryUrl(handle, entry)
await host.fs.write(path, data)
await host.fs.readPrivate(path, start, len)
host.fs.privateUrl(path)
await host.fs.list()
await host.fs.remove(path)
await host.fs.usage()
```

- `pick(title?: string): Promise<PluginPickedFile>` —— 弹一次原生对话框，返回用户选中的那一个文件。
- `stat(handle: string): Promise<PluginPickedFile>` —— 句柄此刻还有效吗（文件被挪走 / 重启过就会失败，据此提示重新选）。
- `entries(handle: string): Promise<PluginZipEntry[]>` —— 压缩包的目录表（条目名只当标识符）。
- `read(handle: string, start: number, len: number): Promise<Uint8Array>` —— 读选中文件的一段（最多 4 MiB 一刀）。
- `readEntry(handle: string, entry: string, start: number, len: number): Promise<Uint8Array>` —— 读压缩包里某一项的一段。⚠️ deflate 条目不能从中间开始解：`start` 大就是从头解到那。
- `entryUrl(handle: string, entry: string): string` —— 某一项的直链（给 `<iframe src>` 用：不必在项目里带任何 zip 库）。
- `write(path: string, data: string | Uint8Array): Promise<PluginPrivateFile>` —— 写私有目录：字符串按 utf8，字节按 base64 上传。
- `readPrivate(path: string, start: number, len: number): Promise<Uint8Array>` —— 读私有目录里某个文件的一段。
- `privateUrl(path: string): string` —— 私有目录里某个文件的直链。
- `list(): Promise<PluginPrivateFile[]>` —— 列私有目录。
- `remove(path: string): Promise<boolean>` —— 删私有目录里一个文件（不存在回 false）。
- `usage(): Promise<PluginFsUsage>` —— 这个包的私有目录占用。

### `host.secrets` —— 需要 `secrets`

```js
await host.secrets.get(name)
await host.secrets.set(name, value)
await host.secrets.remove(name)
await host.secrets.names()
```

- `get(name: string): Promise<string | null>` —— 按名字取值；没配过回 null（不是错误）。
- `set(name: string, value: string): Promise<void>` —— 写值（组件自己的设置页里收）。
- `remove(name: string): Promise<void>` —— 删值。
- `names(): Promise<string[]>` —— 这个包存过哪些名字（只有名字，绝不回值）。

### `host.frame` —— 需要 `frame`

```js
await host.frame.allow(url)
```

- `allow(url: string): Promise<string>` —— 过闸后回规范化后的 host；不通过就抛。 嵌网页只放 https（比 `host.http` 严一档）：宿主的 CSP `frame-src` 也只放 https， 两边判据必须一致，否则坏法是「闸说可以、iframe 一片空白 + 一句 CSP 报错」。

### `host.ble` —— 需要 `ble`

```js
await host.ble.scan(ms)
await host.ble.connect(scanHandle)
await host.ble.write(handle, service, characteristic, data, withResponse)
await host.ble.read(handle, service, characteristic)
await host.ble.subscribe(handle, characteristic)
await host.ble.unsubscribe(handle, characteristic)
await host.ble.notifications(handle, limit)
await host.ble.disconnect(handle)
await host.ble.held()
```

- `scan(ms?: number): Promise<PluginBleDevice[]>` —— 扫一会儿（默认 6 秒，宿主夹在 1–20 秒），返回扫到的外设。
- 每台结果里的 `advert` 是广播包解析出的全部字段，宿主不筛不截：`advertisementName`、`localName`、`appearance`、`txPower`、`addressType`、`deviceClass`（Windows 恒为 null）、`serviceUuids`、`manufacturerData[{companyId, data}]`、`serviceData[{uuid, data}]`；`data` 已是 `Uint8Array`，两个数组宿主已排好序（按 companyId / UUID 升序），按下标取不会漂。⚠️ 这是平台**合并后的最新快照**（同一台设备的多个包并成一份，不是逐个包给），并且**原样字节拿不到** —— btleplug 只翻译它认识的 AD 类型，其余段落在宿主能碰到的任何接口里都不存在。
- `connect(scanHandle: string): Promise<PluginBleConnection>` —— 连上扫描结果里的某一台，顺手把 GATT 树读回来。
- `write(handle: string, service: string, characteristic: string, data: Uint8Array | string, withResponse?: boolean): Promise<number>` —— 写一个特征（字符串按 utf8、字节按原样；单次上限 512 字节，长数据自己分包）。
- `read(handle: string, service: string, characteristic: string): Promise<Uint8Array>`
- `subscribe(handle: string, characteristic: string): Promise<void>` —— 开一条通知订阅（之后用 `notifications` 取）。
- `unsubscribe(handle: string, characteristic: string): Promise<void>`
- `notifications(handle: string, limit?: number): Promise<PluginBleNotification[]>` —— 取走通知，最旧的先给。
- `disconnect(handle: string): Promise<boolean>`
- `held(): Promise<string[]>` —— 这个包此刻连着哪些外设地址。

### `host.serial` —— 需要 `serial`

```js
await host.serial.ports()
await host.serial.open(path, baud, timeoutMs)
await host.serial.write(handle, data)
await host.serial.read(handle, max)
await host.serial.setTimeout(handle, timeoutMs)
await host.serial.close(handle)
await host.serial.held()
```

- `ports(): Promise<PluginSerialPort[]>`
- `open(path: string, baud: number, timeoutMs?: number): Promise<PluginSerialHandle>`
- `write(handle: string, data: Uint8Array | string): Promise<number>`
- `read(handle: string, max: number): Promise<Uint8Array>` —— 读最多 `max` 字节；这一轮没数据回空数组（轮询取数据是正常形状，不是错误）。
- `setTimeout(handle: string, timeoutMs: number): Promise<number>`
- `close(handle: string): Promise<boolean>`
- `held(): Promise<string[]>`

### `host.event` —— 需要 `events:agent`

```js
await host.event.open()
await host.event.close()
host.event.subscribe(handler)
```

- `open(): Promise<string[]>` —— 开始订阅；清单没声明 `events:agent` 会抛。返回可订阅的主题。
- `close(): Promise<void>` —— 停止订阅并清掉轮询。
- `subscribe(handler: (event: PluginAgentEvent) => void): () => void` —— 每条新事件回调一次；返回取消函数。

## 不占权限位的入口

下面这些不出权限徽标，但各有自己的判据；那句判据就是宿主抛错时的原文。

### `host.plugin` —— 这个包自己的身份与声明（只读）

- `id: string`
- `name: string`
- `version: string`
- `description: string`
- `permissions: readonly string[]`
- `hosts: readonly string[]`

### `host.tooltip`

> 授权来自清单的 `surfaces.tooltip.own`，不占权限位

```js
host.tooltip.setSize(size)
```

- `setSize(size: { width: number; height: number }): { width: number; height: number }` —— 申请这张卡的尺寸（物理像素），返回实际生效的那一份。

### `host.log(…)`

```js
host.log(level, message, fields)
```

- `log(level: PluginLogLevel, message: string, fields?: Record<string, string | number | boolean>): void` —— 打一行日志（需求点名的特例：不需要声明权限）。

## 装前给用户看的披露原文

这份抄的是宿主表（`capability.rs`），管理区的确认弹窗逐条显示的就是同一句话。

- **`events:agent`** 订阅转灯事件：可读取你发给 AI 的提问原文，其中可能含密钥或私有代码；组件可把它发往外网，发出后不受控。
- **`fs`** 读写文件：读你亲手在对话框里选中的文件（每次都要重新选），并可在组件私有目录里写任意字节；选中的若是压缩包，能读到里面每一项。
- **`http`** ufo.http 出网：由宿主代发网络请求，只能访问清单 hosts 里列出的域名；请求体与响应内容都在组件手里。
- **`secrets`** 读取密钥：从系统凭据管理器按名字取值；值不落盘、不进日志、界面上不回显。
- **`ble`** 蓝牙连接与收发：扫描并连接蓝牙设备、收发数据；连接会一直保留到你在管理区断开或卸载组件，期间占用无线电与电量。
- **`serial`** 串口读写：打开串口并读写；端口被占用期间其它程序拿不到它。
- **`frame`** 内嵌网页：在组件界面里嵌入网页（只嵌 https 页面）。嵌入的页面看起来像真站，但可能不是它；多数站点禁止被嵌，显示不出来是正常现象。
- **`storage`** ufo.storage 按 id 隔离：在自己的分区里存数据（按包 id 隔离，卸载即删），有容量上限。

## 入口：宿主会从你的默认导出里读这些名字

清单没声明的挂载点，宿主不会来取对应字段，多导出也不会出现。
声明了挂载点却没导出对应项时，包不会崩，但会被记一条降级说明显示在管理区。

| 字段 | 缺了会怎样 |
| --- | --- |
| `activate` | 可选，不导出不影响挂载 |
| `cell` | 声明了对应挂载点却没导出 → 记一条降级 |
| `extension` | 声明了对应挂载点却没导出 → 记一条降级 |
| `extensionHasContent` | 可选，不导出不影响挂载 |
| `hasContent` | 可选，不导出不影响挂载 |
| `settings` | 可选，不导出不影响挂载 |
| `tooltip` | 声明了对应挂载点却没导出 → 记一条降级 |
| `tooltipSize` | 可选，不导出不影响挂载 |

## 挂载点

| 登记名 | 说明 |
| --- | --- |
| `cell` | 悬浮窗格子 —— 在悬浮窗里申请格子，与内置组件同级共享同一块窗。 |
| `tooltip-own` | 独立信息卡（tooltip） —— 悬停它自己的格子时弹出这个组件自己的信息卡。 |
| `tooltip-extension` | 信息卡插入块 —— 往别的组件（如状态灯）的信息卡尾部追加一块，最多 3 行。 |
| `settings` | 主窗设置页 —— 在主窗口一级页签里拥有自己的设置页。 |

四种挂载点在 `public/plugin.json` 里各有一句现成的例子，删掉不要的那几行就行；
清单的键怎么映射到上面那些字段，判据在宿主 `src/lib/plugin-loader.ts`，这份文档不复制那份规则。

## 上限（宿主会夹的数）

| 常量 | 值 |
| --- | --- |
| `MAX_MANIFEST_BYTES` | 64 KiB |
| `MAX_SCRIPT_BYTES` | 8 MiB |
| `MAX_STYLE_BYTES` | 1 MiB |
| `MAX_DATA_BYTES` | 2 MiB |
| `MAX_IMAGE_BYTES` | 4 MiB |
| `MAX_FILES` | 256 |
| `MAX_TOTAL_BYTES` | 48 MiB |
| `MAX_ID_CHARS` | 40 字符 |
| `MAX_NAME_CHARS` | 40 字符 |
| `MAX_DESCRIPTION_CHARS` | 200 字符 |
| `MAX_VERSION_CHARS` | 32 字符 |
| `MAX_REL_PATH_CHARS` | 160 字符 |
| `MIN_CELLS` | 1 |
| `MAX_CELLS` | 16 |
| `MAX_EXTENSION_ROWS` | 3 |
| `MIN_TOOLTIP_WIDTH` | 220 |
| `MAX_TOOLTIP_WIDTH` | 420 |
| `MIN_TOOLTIP_HEIGHT` | 120 |
| `MAX_TOOLTIP_HEIGHT` | 620 |
| `MAX_HOSTS` | 64 |
| `STORAGE_PARTITION_BYTES` | 8 MiB |
| `STORAGE_VALUE_BYTES` | 256 KiB |
| `STORAGE_MAX_KEYS` | 1024 |
| `STORAGE_MAX_KEY_CHARS` | 128 字符 |
| `HTTP_RESPONSE_BYTES` | 4 MiB |
| `EVENT_RING_LINES` | 256 |
| `EVENT_POLL_INTERVAL_MS` | 1000 ms |
| `LOG_RING_LINES` | 200 |
| `LOG_LINE_CHARS` | 2000 字符 |
| `LOG_FILE_BYTES` | 8 MiB |
| `LOG_PAGE_LINES` | 200 |
| `FS_WRITE_BYTES` | 8 MiB |
| `FS_PACKAGE_BYTES` | 256 MiB |
| `FS_GLOBAL_BYTES` | 2 GiB |
| `FS_READ_WINDOW_BYTES` | 4 MiB |
| `FS_PICK_MAX_BYTES` | 2 GiB |
| `PICK_HANDLES_PER_PACKAGE` | 8 |
| `PICK_HANDLES_TOTAL` | 64 |
| `ZIP_ENTRIES_MAX` | 5000 |
| `ZIP_ENTRY_BYTES` | 64 MiB |
| `ZIP_RATIO_MAX` | 200 |

> 体积类超了直接拒装，格子数与行数类是按上限夹取后生效。
> 能力边界与「为什么不开某个口子」见 `reference/doc/guide/plugin-guide.md`。
