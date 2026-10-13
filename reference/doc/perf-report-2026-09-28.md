# UFO 性能实测报告（2026-09-28）

> **测量对象**：`target/debug/tauri-app.exe` + vite dev（`localhost:1420`），WebView2 运行时 153.0.4234.48，单屏 2560×1440。
> **一句话版本**：悬浮窗与提示窗上的 `backdrop-filter: blur(6px)` 是**逐像素零效果**的空转；「打开过一次主窗后 CPU 居高不下」在**事件密集时成立**（实测 2.4 倍），根因是 `app.emit` 全窗广播叠加 `hide()` 后 WebView2 不再把页面当隐藏 —— 已整改并复测抹平。
> **与其它文档的关系**：悬浮窗结构与透明策略见 `AGENTS.md`「全局视觉风格」（本次已按 §3 的实测改掉「毛玻璃」表述）；转灯日志广播门控见 `reference/doc/agent-status-log-guide.md`。本篇只记录**实测数据与由此得出的整改**，不改写其它既有约定。
> **修订**：v2 修正了 §4 —— 初版用「关窗后立刻量」的协议得出「静置也涨 8.7 倍」，事后证明那被此前注入的事件余波污染了，见 §4.1。

## 目录

1. [结论摘要](#1-结论摘要)
2. [测量口径与手段](#2-测量口径与手段)
3. [实验一：毛玻璃特效到底买到了什么](#3-实验一毛玻璃特效到底买到了什么)
4. [实验二：主窗打开过一次之后](#4-实验二主窗打开过一次之后)
5. [已实施的整改与复测](#5-已实施的整改与复测)
6. [仍待处理与没测到的部分](#6-仍待处理与没测到的部分)
7. [复现方法与现场状态](#7-复现方法与现场状态)

---

## 1. 结论摘要

| # | 结论 | 置信度 | 依据 |
|---|---|---|---|
| 1 | `.floating-shell` / `.tooltip-shell` 的 `backdrop-filter: blur(6px)` **不产生任何像素变化**，看到的"毛玻璃"完全来自 `--ufo-bg` 的 0.8 alpha | 高（逐像素 0 差异 + 对照组证明机制可用） | §3.1 §3.2 |
| 2 | 该 blur 的**绝对开销很小**：真静置、灯点闪烁、满窗动画三种实际状态下，开与关都测不出差别 | 高（多轮交替） | §3.3 |
| 3 | 唯一稳定复现的 blur 成本：**页面上存在合成层时，GPU 永不空转**（0.001% → 0.68~1.18%） | 中（该状态 App 不长期停留） | §3.3 |
| 4 | 主窗被 `show()` 过一次之后再隐藏，**事件密集时整棵进程树 CPU 涨 2.4 倍**；静置时两者都接近 0 | 高（同协议、独立实例、沉降 45s） | §4.2 |
| 5 | 贵在「每个收件人都要收一遍」：隐藏的主窗收到的广播条数与显示前几乎相同（53 vs 56），但显示过后每轮要多付渲染进程 + 浏览器进程 + Rust 侧序列化三份 | 高 | §4.3 |
| 6 | 上游缺口：Tauri 2.10.3 的 `hide()` 只隐藏 HWND，**从不通知 WebView2 控制器 `SetIsVisible(false)`** | 高（读 tauri / wry 源码确认） | §4.3 |
| 7 | 整改后同一协议复测：开过一次的压力臂 **0.1062 → 0.0287**，与从未打开（0.0305）已无差别 | 高 | §5 |

## 2. 测量口径与手段

- **主指标**：整棵 webview 进程树的 CPU（`Get-Process.TotalProcessorTime` 差值 ÷ 区间秒 = 单核占比），按 PID 拆开看 app / browser / gpu-process / 三个 renderer。
- **辅指标**：`GPU Engine(*engtype_3d)\Utilization Percentage` 区间均值（实例名按 `pid_<十进制>` 归到本 App）。
- **视觉指标**：`Graphics.CopyFromScreen` 抓整屏 → 按窗口 rect 裁剪 → 两图逐像素求差（`meanAbsPerChannel` / `maxAbs` / 差异像素占比）。
- **热切手段**：改 `src/assets/main.css` 由 vite HMR 即时应用到运行中的窗口，**同一进程内 A/B**，避免重启造成状态漂移。探针块用自定义属性 + `sed` 翻值，测完整块删除。
- **页内计数器**：在主窗临时挂若干 `listen()`，把各通道收到的广播次数画成左上角黑底绿字条，用屏幕截图读出 —— 因为 WebView2 拒绝开 `--remote-debugging-port`（`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` 确认进了浏览器进程命令行，但 `DevToolsActivePort` 不生成），CDP 这条路不通。
- **事件压力**：用 App 自己的 `--status-hook --provider qoder` 往真 inbox 注入 `PreToolUse`，约 9 条/秒。刻意复用**当前会话自己的 session_id**，所以不新增会话记录行。
- **协议要求**：每臂起干净实例 + **沉降 45 秒**再量。这条是 §4.1 那个教训换来的 —— 注入压力之后，token 重扫 / 存档刷盘 / 日志落行还要追一段历史，不沉降就会把余波读成稳态。

## 3. 实验一：毛玻璃特效到底买到了什么

### 3.1 视觉：逐像素零差异

悬浮窗当时 90×45（两格），区域共 4050 像素。

| 对比 | meanAbsPerChannel | maxAbs(3 通道和) | 差异像素占比 |
|---|---|---|---|
| `.floating-shell` blur 开 vs 关 | **0** | **0** | **0%** |
| 同区 blur 开 vs blur 开（背景漂移基线） | 0 | 0 | 0% |

### 3.2 对照组：这套 CSS 本身是好的

把 blur 从「根元素」挪到「有内容可采的中间层」（`.floating-module-host` 压住一层红黄 6px 斜条纹动画）：

| 对比 | meanAbsPerChannel | maxAbs | 差异像素占比 |
|---|---|---|---|
| host 层 blur 开 vs 关 | 58.0 | 399 | 65.3% |

肉眼：关 = 锐利条纹，开 = 明显糊开。**结论**：`backdrop-filter` 在这个 WebView2 里工作正常；§3.1 的 0 差异不是"没生效"，而是**透明 OS 窗口的桌面像素不在 CSS 的 backdrop 里** —— 桌面由 DWM 合成，而这两个元素都是各自窗口的根节点，之下什么都没有。

### 3.3 开销：只在一种状态下测得出差别

GPU = 3D 引擎利用率均值（16~30s 臂，10~20 个采样）。⚠️ 本节各臂用的是**沉降前**的协议（同实例内 ON/OFF 交替，交替顺序与结果一致，因此开关差值可信；但绝对值含余波）。

| 负载 | blur ON | blur OFF | 判定 |
|---|---|---|---|
| 真静置（原始代码、无探针，30s×2 轮） | 0% / 0.264% | 0.064% / 0% | 差异在噪声内 |
| 有合成层但画面静止（动画 paused，16s×2 轮） | 0.684% / 1.179%（GPU 进程 0.89s、1.09s） | 0.16% / **0.001%**（0.06s、0.08s） | **稳定复现** |
| 32px 灯点闪烁（复刻 667ms 完成闪烁，16s×2 轮） | 1.059% / 1.164% | 0.968% / 1.409% | 无差异 |
| 满窗动画（blur 覆盖整窗 90×45，16s×2 轮） | 1.088% / 0.961% | 1.214% / 0.695% | 无差异 |
| 真玻璃压在动画条纹上（blur 真的在干活，16s×2 轮） | 0.819% / 1.236% | 0.925% / 0.856% | 无差异 |

各臂 CPU 开与关都在同一噪声带内。**怎么读**：窗口只有 90×45，一次 6px 模糊对 GPU 是微秒级，所以「毛玻璃贵不贵」在这台机器上问不出答案；值得在意的是第 2 行 —— 只要页面上存在合成层，有 blur 的那一臂 GPU 就永远空不下来，而它对 7×24 常驻的悬浮窗是常态而非例外。

## 4. 实验二：主窗打开过一次之后

### 4.1 协议修正（先记一笔自己的错）

初版协议是「开合主窗 → 20 秒后量静置」，得到 0.0047 → 0.0409（8.7 倍）并写下「不会自行回落」。**这个结论不成立**：那 20 秒里 App 还在消化此前注入的 ~230 条事件（transcript 的 token 重扫、SQLite 刷盘、日志落行）。

换成「独立实例 + 沉降 45 秒 + 量 25 秒」后，旧版静置两臂为：

| 主窗状态（旧版） | 沉降后静置 cores |
|---|---|
| 从未打开 | 0.0062（三个 renderer 全 0） |
| 开过一次后关闭 | 0.0049（三个 renderer 全 0） |

→ **静置时隐藏的主窗本来就不干什么活**，初版那条「静置也涨」是测量污染。

### 4.2 站得住的是压力臂（同一协议、同机、事件 ~9 条/s）

| 构建 | 从未打开 | 开过一次后关闭 | 倍率 |
|---|---|---|---|
| 旧版（改动前） | 0.0435 | **0.1062** | **2.4×** |
| 新版（改动后） | 0.0305 | **0.0287** | 0.94×（已抹平） |

旧版「开过一次」那一臂的分解：`tauri-app` 0.0348、`browser` 0.0371、主窗 renderer 0.0160 —— 涨的**不只是渲染进程**，Rust 侧的序列化和 WebView2 浏览器进程的 IPC 路由都按收件人数量一起涨。

### 4.3 机制

- 页内计数器实测：显示过再隐藏的主窗在 26 秒里收到 `agent-status +56`、`ufo-floating-runtime +56`；**从未显示过**的主窗收到 `+53 / +54` —— 投递量基本相同，成本却差数倍。全程 `gpu-process ≈ 0`（没有合成、没有出帧），所以花的是主线程 JS + 样式/布局，以及进程树另外两头的开销。
- 上游缺口（读源码确认）：tauri 2.10.3 `src/webview/webview_window.rs:1933` 的 `hide()` 只转调 tao 的窗口隐藏，**整个 tauri crate 没有任何地方调用 webview 级 `set_visible`**（`set_visible` 在 tauri 里只出现在 tray 与 `set_visible_on_all_workspaces`）。wry 0.54.4 的 `WebView::set_visible()`（`src/webview2/mod.rs:1472`）才会执行 `controller.SetIsVisible(false)`，而 `SetIsVisible` 只在创建时用 `attributes.visible` 调过一次（`:544`）。也就是说：**`hide()` 之后没有任何人告诉 WebView2「这个页面不可见了」**。
- 代码侧的放大器：`src/components/floating/FloatingWindow.vue` 的 `broadcastRuntime()` 无条件 `emitTo('main', …)`，其上方注释「主窗口没开也没关系，事件没人收而已」**正是这次问题的错误前提** —— 窗口是隐藏，不是没开。

### 4.4 顺带量到的常驻成本（未整改）

- 整棵进程树 working set **725 → 742 MB**（测量期间缓慢爬升）。
- **3 个 renderer**（floating / main / tooltip）各约 80–125 MB，其中 main 与 tooltip 平时不可见但常驻 —— 这是 `tauri.conf.json` 里「声明 + 隐藏 + 按需显示」策略换来的代价。
- 产物 678 KB（JS 317 / CSS 159），字体走系统栈无 webfont。
- monitor 500ms 轮询但广播按 `changed` 门控（`status/mod.rs`），静置零 IPC；拖拽期 80ms 的鼠标状态轮询有 in-flight 去重、松手即停 —— 这两处设计是对的，实测印证。

## 5. 已实施的整改与复测

改动清单（`cargo check` / `npx vitest run` 553 项 / `npm run build` 全绿；功能回归见下）：

| 改动 | 落点 | 作用 |
|---|---|---|
| 状态快照不再全窗广播，**只跳过不可见的主窗** | `status/mod.rs` 新增 `emit_status_snapshot`，`emit_snapshot` 与 `dismiss_status_session` 都走它 | 消掉隐藏主窗的收件成本，以及 Rust 侧序列化 + 浏览器进程路由的按收件人放大 |
| monitor 每轮查一次主窗可见性，`false → true` 的那一轮按「变了」补发一份 | `monitor_loop` 的 `main_was_visible` / `became_visible` | 隐藏期间漏掉的灯态在重新显示时自动续上；托盘与前端两条显示路径都覆盖，不需要新增事件名 |
| runtime 快照**按内容去重**后再广播 | `FloatingWindow.vue` `broadcastRuntime` | 实测 26 秒里 56 条内容几乎相同；发失败会擦掉去重记号，不会永久丢广播 |
| 删掉两处空转 `backdrop-filter` | `main.css` `.floating-shell` / `.tooltip-shell`（并就地写明原因与证据） | 视觉零变化，静止时 GPU 不再被拖着空转 |
| `onBeforeUnmount` 补 `records.stop()` | `SessionRecordsPanel.vue` | 与 `log.stop()` 配平；store 有 `started` 守卫，所以是「订阅永不回收」而非叠加 |
| 文档同步 | `AGENTS.md` / `CLAUDE.md`「窗口透明策略」与视觉 token 段 | 把「悬浮窗为毛玻璃」改成「半透明浅灰底」，并写明 CSS 采不到桌面、要真糊得走原生 |

**刻意没有做的**：提示窗**不进**可见性过滤。它的 `tooltip.hasContent` 要在弹窗**之前**就答得上来，漏掉一条广播就会整块弹不出来 —— 这是 `floating-module-guide` 里「数据晚到一步就永远弹不出来」的同一条红线。

复测结果见 §4.2 表：压力臂 0.1062 → 0.0287（与从未打开的 0.0305 持平）。功能回归：提示窗悬停内容正常且是最新值（Endpoint、提问都刷到了当下这一轮）、主窗重新显示后「窗口格子」读数与真实格子一致、悬浮窗外观与改动前逐像素一致。

## 6. 仍待处理与没测到的部分

- **用户描述的「静置也居高不下」尚未被解释**：按修正后的协议，旧版静置两臂都接近 0。如果指的是 agent 正在干活那段时间，§4.2 已经复现并修好了；如果是真的静置也高，需要一个具体读数（哪个进程、多少 %、当时有没有会话在跑）再查。
- **`status-hook-event` 仍全窗广播**，且 `stores/agentStatus.ts` 里有一条 `console.log('[status-hook-event]', event.payload)` 的调试监听挂在每个窗口 —— 事件密集时是三条额外开销，看着像调试残留，建议确认后删。
- **大窗口下的 blur 成本没测**：`cell_px` 拉到 96 + 多组件叠到 3×3 时模糊面积是本次的 8 倍以上，需要改配置文件并重启，我没动。
- **DWM 侧成本读不到**：`dwm.exe` 的 `TotalProcessorTime` 恒返回 0（权限受限），系统合成器为透明窗口付的账不在视野内。
- 全部数字来自 **debug 构建 + vite dev**；§4/§5 的差值是前端与 IPC 层面的，release 同样存在，但绝对值会更低。

## 7. 复现方法与现场状态

脚本在 `.tmp/perf/`（未跟踪；`.tmp` 不在 `.gitignore` 里，所以会出现在 `git status`）。建议整组搬到 `test/evidence/perf/` 长期留着：

| 脚本 | 用途 |
|---|---|
| `pidcpu2.ps1 -Seconds -Label -AppPid` | 按进程量 CPU 区间差值与 working set（`-AppPid` 用来排除 `--status-hook` 短命进程） |
| `sample3.ps1` | CPU + `GPU Engine` 3D 利用率均值/峰值 + 内存 |
| `wins.ps1` | EnumWindows 取 UFO 各窗口 rect（裁剪与点击坐标都靠它） |
| `shot.ps1 -Crop x,y,w,h -Scale n` / `diff.ps1 -A -B -Rect` | 抓屏 / 裁剪放大 / 逐像素求差 |
| `click.ps1` / `hover.ps1` | user32 真鼠标点击 / 悬停（开合主窗、切 tab、唤出提示窗） |
| `burst.sh <秒>` | 以当前会话 id 注入 ~9 条/s 的 hook 事件造压力 |
| `arm2.sh <never\|opened> <idle\|burst>` | **修正后的单臂协议**：沉降 45s 再量（采样器放后台、注入放前台，避免工具等不到 EOF） |
| `run2.sh` / `idle.sh` / `l3check.sh` / `matrix.log` | §3 的 blur 矩阵与对照组 |

踩过的坑（写下来免得下次重走）：`.ps1` 必须**纯 ASCII**（zh-CN 下 PowerShell 5.1 按 GBK 读无 BOM UTF-8，中文注释尾字节能吞掉换行）；`$pid` 是只读自动变量；`$Out` 与 `$out` 同名互覆；bash 里 `blur(6px)` 要加引号；`GPU Engine` 的 `Running Time` 是**累计值**，按秒求和会得到单调递增的假数，只有 `Utilization Percentage` 能直接平均；把长命令接进 `| tail`/`| grep` 会因为后台子进程不退出而永远等不到 EOF。

**现场状态**：

- 上述改动都在工作区，**未提交**（按仓库约定，只有明确要求才 commit）。
- 屏上运行的是新构建的 `target/debug/tauri-app.exe` + 我手动起的 vite（1420，无 vue-devtools）；原来的 `pnpm tauri dev` 进程组在本次测量中被结束，恢复自行重启即可。
- 为造压力累计注入约 4000+ 条转灯日志（都挂在本次会话的 key 下，未新增会话记录行），今天 `transitions-2026-09-28.0.jsonl` 已到 2 MB 量级，其中大部分来自这次测量。
