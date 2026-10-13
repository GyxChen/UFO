# UFO 组件包（插件）guide

面向「要装进 UFO 的第三方包」的作者与要改宿主这一层的维护者。
模板与可跑的产物在 `reference/plugin-template/`（README 讲怎么跑），这篇讲**规矩与边界**。

## 0. 架构结论（对着需求里那六个例子）

一句话：**一个 ESM 单文件包 + 清单声明能力 + 宿主自定义协议呈递**；
不是给 JS 开运行时、不是 worker、不为组件建窗。逐条：

| 例子里要的能力 | 结论 | 代价与边界 |
| --- | --- | --- |
| npm 包安装 | **不做取源**：安装入口只接受一个本地目录 | 取 npm 就要引 registry 客户端与依赖解析，而「产物是那一个 ESM 文件」这条边界不会因此变松 —— 作者 `npm pack` / 构建完把目录指给宿主，路径已经通了。少一个能装任意代码的入口，就少一条没人审的供应链 |
| epub 这类大文件 | 走 `fs` + 协议 URL，压缩包里的**每一项**可单独寻址（§5） | 宿主自己解 zip 格式（条目名不当路径、压缩比与单条目上限、加密条目报错、目录表按 mtime 缓存）。换来的是 400 MB 的书不占 400 MB 内存 |
| 蓝牙连接与收发 | `ble` 能力实装；连接、GATT 树、通知环形都**常驻宿主**（§4） | 单写 ≤ 512 字节、通知环形 64 条；句柄不跨启动。**没上过真设备**，验到的只是形状与闸（§9） |
| echarts（画图标 / 绘制图表） | 作者构建时把库打进那一个文件，宿主**不注入第三方库**（只注入 Vue / Pinia） | 单脚本 8 MiB、整包 48 MiB、文件数 256 —— 超了就**拒装**（预览给中文原因，不是夹取）。像 echarts 这种整库打包前先量一次产物体积；清单没有 `styles` 口子，CSS 要 `?inline` 跟着 JS 进来（§6） |
| webview 视图（浏览网页） | 就是作者界面里的 `<iframe>`，宿主只回答「这个地址能不能嵌」 | 建窗会绕开面板窗那套规矩（label 要进 capability、视图要在 `src/windows/index.ts` 登记），还会多出一个能显示任意网页又能拿 host 的上下文；只放 https，且与 CSP 同判据（§4） |
| 串口读写 | `serial` 能力实装（§4） | 端口名只认 `COM3` / `/dev/tty…` 两种形状，不接受任意路径；每包最多 4 个端口 |

## 1. 一个包是什么

一个目录，里面至少两份文件：`plugin.json`（清单）与 `entry` 指向的那个 ESM 文件。
宿主用自定义协议把它呈递成网页可寻址的资源：

- macOS / Linux：`ufo-plugin://<id>/<包内相对路径>`
- Windows（WebView2 的形状）：`http://ufo-plugin.localhost/<id>/<包内相对路径>`

装载 = 宿主在自己的窗口里 `import()` 那个 URL，取默认导出。**没有运行时建窗**，
组件的界面挂进宿主已有的三种窗口（悬浮窗格子 / 提示窗信息卡 / 主窗设置页）。

外部依赖由作者自己在构建时打进那一个 ESM 文件（echarts、marked、pdf.js 都行）；
Vue / Pinia 例外 —— 必须用宿主注入的那一份（§6）。

## 2. 清单（`plugin.json`）

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `manifestVersion` | 数 | 清单格式版本，宿主是 1；不认识就拒装 |
| `apiVersion` | 数 | 要求的宿主 API 版本；比宿主新就拒装（宁可不装也别挂上后到处抛错） |
| `id` | 串 | **同时是目录名、配置键、存储分区名**。只允许 `a-z0-9-`，短横线不在首尾、不连续 |
| `name` / `description` / `version` | 串 | 展示用；过长会被夹（见 `paths.rs` 的字符帽） |
| `entry` | 串 | ESM 入口的包内相对路径（多段允许，如 `dist/main.js`） |
| `surfaces` | 对象 | 界面挂载点（§3）——只说「插在哪」，不授权碰任何东西 |
| `permissions` | 串数组 | 宿主能力名（§4）。**未登记的名字 = 拒装**，不是忽略 |
| `hosts` | 串数组 | 出网域名白名单，只在声明 `http` 时生效；**整名相等**，不做后缀匹配 |

结构性错误（缺字段、id 不合法、能力名不认识、路径越界）= 拒装并给一句中文原因；
数字类问题（格子申请太多、tooltip 超一屏预算）= **夹取 + 一条告警**，不拒装。

## 3. 四个挂载点

- `surfaces.cell: 1..16` —— 悬浮窗里占几格。`demand()` 只能读自己的数据与当前形态，
  **不许反过来读已分配的格子**（会成环）。返回 0 也至少按 1 格算。
- `surfaces.tooltip.own: true` + `size: [宽, 高]` —— 悬停**自己的格子**时弹的独立信息卡。
  `size` 只是**初值**：卡里放不下就按内容再要地方（`host.tooltip.setSize`，见 §4），
  两条路共用同一对常量（220×120 起、420×620 止，与内置组件的信息卡同一档预算），提示窗再按工作区夹一次。
  ⚠️ 这张卡由「哪个模块正被悬停」驱动，因此**格子自己得报悬停**：
  `inject(Symbol.for('ufo.floating.module'))` 取到外壳的 `{ allocatedCells, reportHover }`，
  在根节点的 `pointerenter` / `pointerleave` 里报与撤（模板 `src/Cell.vue` 是样本）。
  不报的后果不是报错，是「格子画得出来、卡永远不弹」—— 只有真窗看得见。
- `surfaces.extension: { into, maxRows }` —— 往**别的组件**的信息卡尾部追加一块，上限 3 行。
  这条通道不在 `FloatingModuleDefinition` 里：`plugin-loader` 另发一份 `pluginExtensions`，
  由提示窗在卡尾渲染，**来源标注（「插件 · 名字」）常驻**，虚线分隔。
  宿主按三重筛选用：目标宿主匹配 + 这个包在外壳配置里启用 + 块自己说有内容。
- `surfaces.settings: true` —— 主窗一级页签里自己的设置页（走模块注册表的 `settings.sections`，
  与内置模块同一条通道，因此接新组件不改 `App.vue`）。

入口的 `hasContent` / `extensionHasContent` 决定「此刻弹不弹」：假 = 整块不出现，
不留一块空糊在屏幕上。提示窗因为要装下插件块会**先加长再夹进工作区**。

## 4. 宿主能力（`host.*`）

能力名与徽标、披露文案的唯一来源是 `src-tauri/src/plugin/capability.rs` 那张表
（前端 `src/lib/plugin-capabilities.ts` 逐字对撞，改文案必须两边一起改）。

| 能力名 | API | 边界 |
| --- | --- | --- |
| —— | `host.log(level, msg, fields?)` | 需求点名的特例：**不需要声明**。行只进内存环形 + 按包 JSONL；调试开关关着就整条丢弃。日志窗：齿轮 → 详情卡 → 拓展 |
| `events:agent` | `host.event.open/close/subscribe` | 转灯事件（含 `endpoint` / `prompt` / `sessionId` / 时间）。**按包拉增量、不广播**；环形 256 条，拉得慢会丢最旧的并有读数 |
| `http` | `host.http.request/get/post` | 宿主代发，只到清单 `hosts` 里的**整名**；过 `validate_http_url`（本机 / 私有 / 元数据地址一律拒）；**非 2xx 不抛、照样 resolve**，作者判 `result.status`（401 / 403 常表意「凭据失效」，抛错会把它吞成「没网」；与内置流程脚本 `flow/api.rs` 同一口径）；响应上限 4 MiB，截断会如实标 |
| `storage` | `host.storage.get/set/remove/keys/usage` | 按包 id 隔离的 JSON 分区：8 MiB/包、256 KiB/值、1024 键；**卸载即删** |
| `fs` | `host.fs.*`（§5） | 用户亲手选中的文件 + 组件私有目录 |
| `secrets` | `host.secrets.get/set/remove/names` | 值只进出 OS 凭据管理器（service = `<App 标识>/plugin/<id>`，一格一包）。值不落盘、不进日志、界面不回显；盘上只记名字，卸载按名册删干净 |
| `frame` | `host.frame.allow(url)` | 内嵌网页就是作者界面里的 `<iframe>`：宿主只回答「这个地址能不能嵌」，**不为组件建窗**；只放 https（与 CSP `frame-src` 同一判据 —— 一边放行一边拦死等于让作者猜）。多数站点禁止被嵌，显示不出来是正常现象 |
| —— | `host.tooltip.setSize({ width, height })` | 改**自己那张独立信息卡**的尺寸（物理像素），返回实际生效的那一份（被夹过就如实报）。授权来自挂载点声明：清单没写 `surfaces.tooltip.own` 就抛，所以它**不占权限位、不加徽标**（那张卡本身就是披露）。也不走 IPC —— 卡就渲染在宿主窗口里，绕一圈只多出一条能被滥用的命令。区间与清单同一对常量，提示窗再按工作区夹一次 |
| `ble` | `host.ble.scan/connect/write/read/subscribe/unsubscribe/notifications/disconnect/held` | 连接、GATT 树与通知环形**常驻宿主**（组件热重载 / 窗口重建都不会断连）。句柄不跨启动、卸载即断；单写 ≤ 512 字节（长数据自己分包）、环形 64 条溢出丢最旧的并有读数；UUID 只认 128 位写法（`2A37` 那种短 UUID 不翻译）；`scan` 把广播包解析出的字段整份给组件（厂商数据、服务数据、appearance、txPower、地址类型），不筛不截，但**原样字节拿不到** |
| `serial` | `host.serial.ports/open/write/read/setTimeout/close/held` | 端口名只认 `COM3` 与 `/dev/tty…` 两种形状，**不接受任意路径**；波特率与超时是夹取不是拒绝；每包最多 4 个端口；这一轮读不到回空数组，不算错误 |

`host` 是冻结对象，且**只有这些字段**：不给 `invoke`、不给 `Channel`、不给任何 `@tauri-apps/*` 原物。
没声明的能力一用就抛，消息点名该在 `permissions` 里加什么。
真正的闸在 Rust 命令层（每条能力命令第一行 `require_permission(store, id, 能力名)`）：
窗口级 capability 只答得出「哪个窗口在调」，答不出「哪个包在调」。

## 5. `fs` 的两条路

1. **系统里的文件只能由用户亲手选**：`host.fs.pick(title?)` 弹一次原生对话框，返回
   `{ handle, name, size, archive, url }`。`handle` 是**能力 URL 型授权**——句柄自带一段进程内随机数，
   谁拿到句柄谁能读；它不写入安装记录、**不跨启动**，卸载 / 重载即作废。这就是披露里那句
   「每次都要重新选」的实现形状。
2. **私有目录随便写**：`app-data/plugins/<id>/files`（与来源无关，本地包也不会写进作者的源码树）。
   三顶帽：单文件 8 MiB、整包 256 MiB、所有包合计 2 GiB；逐段路径校验 + 新建文件的父目录先规范化
   再对表，符号链接不跟。卸载整片删。

大块二进制**不要**走 IPC 的 base64，走 URL：`ufo-plugin://__pick/<handle>` 与
`ufo-plugin://__fs/<id>/<相对路径>`，支持 `Range`，浏览器自己分段取。

压缩包（`zip epub jar docx xlsx pptx apk whl cbz…`）里的每一项也可寻址：
`fs.entryUrl(handle, 条目名)` → `…/__pick/<handle>?entry=<编码后的条目名>`。
动机不是方便，是流行 zip 库只有「整包读成一块」这一条 API —— 把容器当不透明字节交给作者，
400 MB 的书就真占 400 MB 内存。宿主因此要解格式，接了四件事：

- 条目名**只当标识符**，永不拼进文件系统路径（`../../win.ini` 是合法条目名）；
- 压缩炸弹：单条目解压上限 64 MiB + 压缩比上限 200 倍，边解边数，不信头部声明值；
- 加密 / 损坏条目：明确报错，不静默回空字节；
- 目录表按 `(句柄, mtime)` 缓存。

⚠️ 披露义务：「你选中的若是压缩包，组件能读到里面的每一项」。「给他一本书」在权限上等于
「给他这本书里的所有东西」，这句不能让用户自己推断。

## 6. 作者侧构建的四条硬规矩

1. **Vue / Pinia 用宿主注入的那一份**（`globalThis.__UFO_VUE__` / `__UFO_PINIA__`，
   由 `main.ts` 在 `app.mount()` **之前**挂上）。产物里必须没有第二份 Vue，否则
   `provide/inject` 与响应式静默错乱 —— 表现是「HMR 之后又好了」那一类最难查的不对称。
   做法是一个 `resolveId` 把 `vue` / `pinia` 换成生成模块，模块体逐名转发到那个全局。
   ⚠️ 两个坑（都被第一次真实构建抓到过）：
   - **必须 `enforce: 'pre'`**。vite 的核心解析器排在普通插件之前，漏了这句它就把 `node_modules`
     里的 Vue 真的打进包里 —— 构建与类型检查全绿，产物里一个 `__UFO_VUE__` 都没有，
     只有运行时才暴露。
   - **导出名单要建包时从模板自己依赖的 vue 现取**，不是手写。`.vue` 编译产物会 import
     `unref / toDisplayString / createElementVNode / openBlock…` 这类编译器内部助手名，
     手写名单必漏（第一版漏出 23 个 MISSING_EXPORT）。名单缓存放插件闭包里：
     rolldown-vite 给每个钩子独立上下文，挂在 `this` 上到 `load` 时就是 undefined。
2. **一个入口文件**（`rollupOptions.output.codeSplitting: false`）。带 hash 的多 chunk 会让
   「包目录里有什么」由构建决定；宿主的读边界是「包目录内的合法路径」，声明不了未来才生成的文件名。
   ⚠️ 这个开关**只能挂在 `output` 上**：放在 `rollupOptions` 上只得到一句「Invalid key」警告，
   然后被静默忽略。
3. **样式只能跟着 JS 进来**。清单**没有** `styles` 字段，宿主装载的就是 `entry` 那一个文件 ——
   所以模板把 CSS 写成 `import styles from './styles.css?inline'`，在**模块顶层**挂一个带 id 的 `<style>`
   （有则换文本、无则插入），`.vue` 里不写 `<style>` 块。构建产物目录里出现第二个 `.css` 就是死文件，宿不会读它。
   ⚠️ 挂点必须在顶层而不是 `activate()` 里：热重载只重新 `import`，同一个包的 `activate` 不会重跑
   （§7 第 5 条），放进 activate 的样式就永远停在第一次那份 —— 表现是「类名对了、样子没变」。
4. **产物目录就是要安装的目录**（`dist/my-plugin/`），文件名固定 `main.js` 且与清单的 `entry` 一字不差
   （对不上就是「装上了但 import 404」，而那句会被作者当成路径写错）；
   文件数 ≤ 256、总体积 ≤ 48 MiB、单脚本 ≤ 8 MiB —— 超出的**装不上**（预览直接给中文原因），
   已经躺在安装记录里的也不再挂载。数字的唯一来源是 `src-tauri/src/plugin/paths.rs`，
   前端 `plugin-limits.ts` 手抄对撞。

## 7. 热重载怎么工作

1. 详情卡的「热重载」开关只对 `source === 'local'`（就地安装的作者目录）有效；
   安装记录在写入处就把它夹成 `devReload ⇒ local`。
2. 每个窗口按 2 秒问一次 `get_plugin_dev_digest`（**读命令**，所以悬浮窗也能问）：
   所有 dev 包的「最新 mtime + id」指纹。没包开热重载时一次都不问。
3. 指纹变了 → 重跑 `syncPluginDefinitions` → dev 包的入口 URL 换成新的 `?v=<第几次同步>`。
   ESM 按 URL 缓存模块，不带这个串重 `import()` 拿回来的还是旧的那份。
4. 安装记录本身**不重扫**（记录没变，要绕开的只是模块缓存）。作者侧的「文件变动就 build」由
   `yarn dev`（`vite build --watch`）负责。
5. 热重载换掉的是**组件与 host 闭包**，`activate` 不重跑：外壳按包 id 记激活账（重跑会把内置模块的
   订阅一起拆了重建，灯会闪）。所以分工是硬的 ——「每次装载都要做一次」的事（挂 `<style>`）写在模块顶层，
   「一扇窗一次」的事（订阅、恢复读数）留在 `activate`。
6. 装载失败会写进**这个包的日志**（`装载失败：…`，字段 `window` 说明出自哪扇窗）。
   因为 `pluginLoadErrors` 只活在出事的那个窗口里，而悬浮窗 / 提示窗一块插件界面都没有：
   没有这一条，「装上了、徽标也齐，但格子少一个」在界面上是查不出来的。

## 8. 界面与安全规约（组件包也适用）

- 不用 `window.alert` / `confirm`（被宿主的 `tauri-plugin-prevent-default` 拦掉）；
  反馈一律走行内 `role="status"`，破坏性动作走两步确认。
- 指示灯一类的小元素不加 `title`（Windows 原生提示会与自绘 tooltip 重复），无障碍信息走 `aria-label`。
- 「当前是什么状态」的读数必须常驻，不许藏进 hover。
- **样式只保证一套东西：`--ufo-*` 那批 CSS 变量**（`:root` 上的 token，见 `reference/doc/rule/visual-style.md`）
  —— 包的产物注入的就是宿主文档，变量本来就继承得到。宿主的**类名不在契约里**：
  `.ufo-card` / `.base-*` / `.tooltip-row` 这些是宿主 UI kit 的内部实现，重命名一次就静默变形，
  而宿主一侧没有任何 spec 会为此变红。现状是**没有明文规定**，样本包已经借了一枚
  `.ufo-mono`（`ble-console` 的 SettingsPanel 八处），`weather` / `todo` 两个包一处不借、
  自带按钮与输入框的样式。要在 guide 里定死之前，先决定这两条路只留一条。
- 明文落盘是这套系统的已知代价：`prompt` 会长期留在事件环形与日志里，
  所以 `events:agent` 的披露是红档（danger），装前要逐条摊给用户看。

## 9. 待验与已知边界

- 样本包 `E:\workspace\AIProjects\ufo-plugins\epub-shelf` 的真窗验收只走到
  「格子出现 → 悬停弹出阅读器 → 渲染真 epub 正文 → 运行时改卡尺寸并重新定位」；
  **目录展开那一档（同一条 `setSize` 路径）没在真窗点过**，只在单测里钉过。
- 蓝牙 / 串口的代码路径都在，但**没有真设备就没验过收发**：单测覆盖的是形状与闸
  （端口名规则、句柄归属、超大包与压缩/超时夹取、通知环形溢出），
  扫描能不能出设备、连上能不能读写、断连会不会留僵尸 —— 都要接上外设才算验收。
- **装载是同步的**：安装记录一变就把每个包的入口 `import()` 一遍。有格子的包开机就要挂进可见槽位，
  所以「懒装载」必须先有「哪个槽位真的可见」的回调，不是清单里补一个字段能解决的事。
  这条风险的兜底是 §6 那道 8 MiB 入口体积帽（超了装之前就被拒）。
- **宿主按注册表全量激活**：装了但**没启用**的包同样会收到一次 `activate` —— `App.vue` 的
  `syncActivations` 读的是 `FLOATING_MODULE_DEFINITIONS.value`（全量），挂载那一侧才按
  `enabledModuleDefinitions` 过滤，而 `floating-modules.spec.ts` 里那条
  「activates every module in registry order」钉的就是这个行为，不是 bug。内置用
  `isModuleEnabled` 挡掉后台工作，**包读不到外壳的启用状态**：带轮询 / 定时出网的包只能把
  后台任务挂在格子的 `onMounted` / `onBeforeUnmount` 上（格子被挂出来才等于「这一路要工作」）。
- **包没有跨窗口通道**：`PluginHost` 的九个命名空间里没有窗口 label，也没有窗口间事件
  （`host.event` 只是 `events:agent` 的按包拉取）。宿主自己那两处 `app.emit`
  （安装记录变更、日志清空）订阅方是宿主前端，包碰不到。所以内置那套「owner 窗口轮询 +
  广播给三窗」在包侧只能换成 `host.storage` 共享缓存 + 取数锁 + 一格定时回读 ——
  `weather` / `todo` 两个包就是这么做的，代价是最坏同一周期打两次接口（两窗同毫秒抢锁）。
- 从 npm 直接装包没做：那是 §0 里 chosen 的取舍，不是遗漏。
- CSP 的 `script-src` 不含 `'unsafe-inline'`，Tauri 的 IPC 引导脚本吃不吃这条只有真窗知道
  （`pnpm tauri dev` 待验）。`frame-src` 已放行组件产物源与 `https:`。
- 四个挂载点的**观感**（方形 / 四角 / 横条 / 竖条下插件格子的挤与空）只有真窗能给结论，
  几何与缩放走的是外壳同一套槽位规则，不由组件包决定。

## 10. 这条链路上的护栏（改哪里的哪条会红）

- 命令名的四段同源（Rust 注册 ↔ `permissions/*.toml` ↔ `capabilities/*.json` ↔ 前端 `invoke`）
  由 `plugin-acl.spec.ts` 钉住，顺带钉「写命令没漏给含悬浮窗 / 提示窗的那份 capability」。
  这一段对不上时**构建与类型检查全绿**，坏法发生在运行时（那条窗调不动这条命令），所以它值得独立一条。
- 能力面与 CSP 同向由 `plugin-capabilities.spec.ts` 钉（含「`frame` 只放 https」两侧一致）。
- 模板**源码**（不是 `dist/` 产物，那份 gitignore）过一遍真实装载链路：`plugin-template-e2e.spec.ts`
  用 `.vue` 编译出来的默认导出喂 `syncPluginDefinitions`，再逐个挂载点真的 `mount()`。
  它证的是「照这份模板做，四个面真的挂得上、host 真的绑到每一面、样式真的进了文档」；
  测试进程自己就是宿主，所以 `from 'vue'` 与组件实例用的是同一份 Vue —— 正是「不许第二个 Vue」那条。
  样式那半条钉的是「**import 时**就挂上」（读数在 `beforeAll` 之前取，晚一步就分不清是 activate 挂的），
  因为热重载不重跑 activate。
- **格子少一格 / 卡不弹**这一类只有真窗才看得见，所以两条同源的账由 `floating-module-host.spec.ts` 钉住：
  宿主不注入清单时必须读**响应式**的注册表（prop 的默认值工厂只在 setup 求一次，异步装载的包永远进不来），
  以及格子必须能用 `Symbol.for('ufo.floating.module')` 拿到 `reportHover`（键退回 `Symbol()` 时组件包那一侧
  只会静默拿不到上下文）。
- `host.tooltip.setSize` 的三段各有一处钉：夹取与「没声明 `surfaces.tooltip.own` 就抛」在
  `plugin-host.spec.ts`，「运行时那份优先于清单初值 + 只重排正显示的那张卡」在 `TooltipWindow.spec.ts`，
  数值本身（420×620）在 `plugin-limits.spec.ts` 手写字面量 —— 前两处只断言「被夹到常量」，
  真被抬帽是第三条红。

## 11. 把内置模块迁成组件包（`weather` / `todo` 是第一条走通的路）

内置模块与组件包共用同一张注册表，但**内置有四个东西包没有**：Rust 半边、宿主的
`module_settings`、跨窗口事件、宿主 UI kit。迁移不是搬文件，是把这四样换成宿主侧的等价物 ——
下面每行右边那一栏是**已经付掉的代价**，不是待办。

| 内置的做法 | 组件包的做法 | 代价 |
| --- | --- | --- |
| `demand(layout)` 恒返回一个数 | 清单 `surfaces.cell: N` | 无损（`plugin-loader.ts` 的 `demandOf(cells)` 本来就是常数函数）；`demand` 随形态变的模块要先解掉这条 |
| Rust 客户端直连（自带 256 / 512 KB 上限） | `host.http` + 清单 `hosts` 整名表 | 响应 4 MiB 帽、**只搬 UTF-8 文本**（二进制得自己在包里解码）；用户自填代理域名做不了 —— `hosts` 是装前逐条给他看过的静态表 |
| `modules/<id>/secrets.rs`（固定 service 名） | `host.secrets`（service = `<App 标识>/plugin/<id>`） | 两格互读不到 ⇒ **凭据必须重填**，旧条目成孤儿；外壳里不许出现包 id，所以连迁移钩子都写不了 |
| `module_settings[<id>]`（宿主配置文件，跨卸载） | `host.storage`（按包 id 隔离） | **卸载即删**，设置不再进用户配置文件 |
| owner 窗口轮询 + `app.emit` 广播给三窗 | 共享缓存 + 取数锁 + 定时回读（§9） | 最坏同一周期打两次接口；改设置要等下一轮取数才落到格子 |
| Pinia store | 一份 `reactive`（`src/state.js`） | 包根本不必带 Pinia；真要用就 `npm i -D pinia` 让 shim 转发到宿主那一份 |
| 宿主 UI kit（`BaseSelect` / `SettingsField` / `BaseContextMenu` …） | 包里自带 markup + CSS | 设置页那种表单页是**重写不是搬**：todo 那 883 行 `GeneralSection.vue` 的全部工作量都在这一行 |
| 模块自带的 spec（`demand.spec.ts` / `todo-css.spec.ts` …） | 随目录一起删 | 行为等价性另找证据：**把 Rust `#[test]` 的原输入喂给移植后的 JS** 逐条对撞。这一轮跑下来抓到 `parseDueDateMs` 少一个捕获组 —— 带时刻的截止全成 NaN，任务照列但逾期分档与排序静默塌掉 |

迁完的收尾只有真窗能给结论：`pnpm tauri dev` → 装 `dist/<id>` → 四形态看排版与悬停
（**忘了报 `reportHover` 的话格子画得出来、卡永远不弹**）→ 日志窗（齿轮 → 详情卡 → 拓展）
看有没有一句「装载失败：」。
