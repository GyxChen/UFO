# Agent 状态灯 · Endpoint 档案与流程脚本沙盒

> **管什么**：信息卡 Endpoint 行背后的三件事 —— ① 把本地代理地址**反查**成真实上游（代理解析器）；② 按 endpoint 持久化的**档案表**（`endpoint-profiles.json`）；③ 挂在档案上、由用户自写 JS 的**流程脚本沙盒**（QuickJS），它产出信息卡的拓展字段。代码在 `src-tauri/src/modules/agent_status/{proxy/*,endpoint_profiles.rs,flow/*}` 与 `src/modules/agent-status/{lib,stores,panels}/`。
> **不管什么**：灯色怎么转、平台 payload 里字段叫什么、这些值进快照之后怎么落存档、面板的版面。
> **一句话版本**：拓展字段**跟着 endpoint 走**而不是跟着会话走（会话会死，endpoint 不会）；脚本能碰到外界的路径只有一个 `http`，而 `http` 只准往公网发。
> **与其它文档的关系**：
> - `../rule/agent-status-hooks.md` —— `base_url` 从平台配置文件/环境变量的取法（权威在那边），以及 Codex 的 `config.toml` 读法；本篇的「上游反查」与它**刻意同源**。
> - `../guide/agent-status-platform-onboarding-guide.md`（下称**接入指南**）—— 接新平台时若该平台也被本地代理接管，要在解析器里登记两处（§8 表 T3 第 7\* 步）；协议演进红线 `Option` + `#[serde(default)]`、不升 `SCHEMA_VERSION`（§11）；`window.alert`/`confirm` 被拦（§11）。本篇不重述这些红线本身。
> - `agent-status-session-data.md` —— `base_url` / `upstream_url` 在快照与存档里怎么走、`displayEndpoint` 口径、token 单位。
> - `../guide/floating-module-guide.md` —— tooltip 尺寸预算（§8.3）、`activate` 生命周期（§9）、线程与 `async` 命令纪律（§12.3）、panic 与保密两条红线（§12.4）、命令注册三处（§12.5）。
> - `../guide/agent-status-push-guide.md` —— 推送正文取脚本当前值（§3）、**共享**「agent 全停 = 零出网」与退避口径（§7），以及「地址策略与 flow 刻意不同」的原因。
> - 面板版面（二级分区、左右主从、高度链与 `fill`、`max-w-md` 覆盖权重、两级 tabbar）见 `visual-style.md`，本篇一行都不展开。

## 目录

1. [全景：endpoint 从会话到档案到拓展字段](#1-全景endpoint-从会话到档案到拓展字段)
2. [代理解析器：本地地址反查上游](#2-代理解析器本地地址反查上游)
3. [Endpoint 档案：持久化与登记口径](#3-endpoint-档案持久化与登记口径)
4. [沙盒的能力面：脚本能做什么、碰不到什么](#4-沙盒的能力面脚本能做什么碰不到什么)
5. [为什么出网只收成一个 `http`](#5-为什么出网只收成一个-http)
6. [地址策略与其固有代价](#6-地址策略与其固有代价)
7. [调度、去抖与退避](#7-调度去抖与退避)
8. [密钥与凭据记账](#8-密钥与凭据记账)
9. [结果广播与前端消费](#9-结果广播与前端消费)
10. [展示预算与解析上限](#10-展示预算与解析上限)
11. [rquickjs 与 panic 纪律](#11-rquickjs-与-panic-纪律)
12. [落点、命令与授权](#12-落点命令与授权)
13. [红线汇总](#13-红线汇总)
14. [验收清单](#14-验收清单)
- [附录：常量与不变量速查](#附录常量与不变量速查)

---

## 1. 全景：endpoint 从会话到档案到拓展字段

```
会话快照 { base_url, upstream_url }                ← session-data 篇 §3、本文 §2
        │  monitor 每 500ms 采集 upstream ?? base（loopback 不登记）
        ▼
EndpointProfileState（<app-data>/endpoint-profiles.json）      ← 本文 §3
        │  extensions["flow"] = FlowConfig（脚本 + 间隔 + 密钥名清单 + 指纹）
        ▼
FlowState::tick（只跑「存活会话涉及的 endpoint」且到点者）      ← 本文 §7
        │  专用 worker 线程串行跑 QuickJS（runner::run_flow）
        ▼
按会话 key 索引的结果 → 广播 flow-results → tooltip 查表渲染    ← 本文 §9
```

| 概念 | 键 | 存储位置 | 事件名 |
|---|---|---|---|
| 会话 | `provider:session_id` | 内存 + `session-records.db`（session-data 篇） | `agent-status` / `session-records` |
| Endpoint 档案 | `id` = `proxy::endpoint_identity` 归一的 `host:port` | `<app-data>/endpoint-profiles.json` | `endpoint-profiles` |
| 流程脚本配置 | 档案的 `extensions["flow"]`（`FLOW_EXTENSION_KEY`） | 同上，**不给 `EndpointProfile` 加字段** | —— |
| 脚本结果 | 会话 key（**不是** endpoint id） | 只在内存（`FlowState`） | `flow-results` |
| 密钥值 | `<APP_ID>/flow/<endpoint_id>` + 名字 | **OS 凭据管理器**，绝不落盘 | —— |

**为什么以 endpoint 而不是会话为配置对象**：同一上游可能被多个会话共用，而会话快照里的 `base_url`/`upstream_url` 只在会话存活时存在 —— 配置若挂在会话上，重启就没了。档案表因此持久化，**不靠会话存活**。

## 2. 代理解析器：本地地址反查上游

用 cc-switch 这类本地代理时，`base_url` 只是 `127.0.0.1:<port>`，真实上游写在代理自己的配置里。解析器的作用就是把本地地址反查成上游，显示在信息卡的 Endpoint 行。

- 下拉选的是**解析器工具**（`不解析(none)` / `cc-switch` / 将来更多），**不是**代理内部的那个 provider。
- 配置字段：`module_settings['agent-status'].proxy_resolver`。外壳整块透传、不解释；解析与夹取在 `src-tauri/src/modules/agent_status/settings.rs` 的 `AgentStatusSettings::from_block`，缺字段回落 `NONE_RESOLVER`（`"none"`）。
- 接口：`ProxyResolver` trait（`proxy/mod.rs`）+ `ResolverRegistry`；**在 `build_registry` 里注册一行即接入新工具**，下拉清单与填充逻辑零改动。
- 命中解析时 `SessionSnapshot.upstream_url` 被填充；展示口径 `displayEndpoint = upstream_url ?? base_url`（上游优先、回落本地，两者都无显示 `—`），细节见 session-data 篇 §3。
- ⚠️ `upstream_url` 是 `Option<String>` + `#[serde(default)]`，**不得升 `SCHEMA_VERSION`**（接入指南 §11）；它随快照落库但**不是回填字段**。

### 2.1 `CcSwitchResolver` 的读法

只读打开 `~/.cc-switch/cc-switch.db`，按 `proxy_config` + `providers` 两张表反查。

| 环节 | 规则 | 踩过的坑 |
|---|---|---|
| 命中判定 | 该 app 行**两列同时为 1**：`proxy_enabled`（全局代理服务器开关，三行镜像同值）且 `enabled`（该 app 是否被接管）。语义来自 cc-switch 源码 | 只看其一会在「服务器开着但该 app 未接管」时误命中。护栏：`resolve_returns_none_when_global_server_off_even_if_app_enabled` / `resolve_returns_none_when_proxy_disabled_for_app` |
| 端口匹配 | ⚠️ `listen_port` 是 `INTEGER`（`DEFAULT 15721`），**必须按整数读** | 按 TEXT 读会让 rusqlite 整行 `InvalidType` 失败、功能**空转不报错**。护栏：`resolve_returns_none_when_listen_port_mismatches` |
| 上游取值 | 该 app 下 `is_current = 1` 的 provider 的 endpoint | —— |
| 缓存 | `CACHE_TTL = 5s` | 切换 provider 后**最多 5s** 反映到悬浮窗，这是已知延迟 |
| 打开方式 | `SQLITE_OPEN_READ_ONLY` + 短 `busy_timeout`（200ms） | ⚠️ 该库被 cc-switch 应用持有，任何写入或长锁都是越界 |

### 2.2 ⚠️ `settings_config` 必须按 app 分流

两个 app 在同一列里的**存法不同**，取上游必须走 `upstream_from_settings` 分流：

| app | 上游位置 |
|---|---|
| claude | `settings_config.env.ANTHROPIC_BASE_URL` |
| codex | **没有 `env` 块**：base_url 埋在 `settings_config.config` 字段那段 **TOML 文本**里（顶层 `model_provider` → `[model_providers.<name>].base_url`） |

早先只读 `env.ANTHROPIC_BASE_URL`，导致 Codex 会话的 `upstream` 恒为 None、Endpoint 一直停在本地代理地址。解析规则**复用适配器那份**（`status/compat/codex.rs` 的 `base_url_from_config_text`），保证「信息卡显示的 endpoint」与「反查出的上游」同源 —— 两处各写一份解析迟早会分家。护栏：`claude_settings_read_only_env_base_url`、`codex_settings_read_base_url_from_embedded_toml`、`codex_settings_ignore_the_auth_block`。

接入**新平台**且该平台也被本地代理接管时，要登记两处：`proxy/ccswitch.rs` 的 `app_key` 与 `upstream_from_settings`，且必须复用同一份解析规则 —— 清单见接入指南 §8 表 T3 第 7\* 步。

### 2.3 保密红线

- ⚠️ `settings_config` 整块 JSON 会进内存解析，但**只按名字取 base_url 这一个 key**；codex 侧只读 `config`，与它同级的 **`auth` 存的是 API key，绝不读取**。
- ⚠️ token / api key 一律**不读取、不缓存、不上报**（通用红线见 floating-module-guide §12.4）。
- endpoint 按需求**不脱敏**（含 userinfo / query），仅 trim；上限 4096。

## 3. Endpoint 档案：持久化与登记口径

`endpoint_profiles.rs` 的 `EndpointProfileState` 是一张 JSON 表（`<app-data>/endpoint-profiles.json`），供三窗口读取、供拓展面板增删。

| 规则 | 说明 |
|---|---|
| 登记时机 | monitor 每轮把存活会话的 `upstream_url ?? base_url` 交给 `record_all`，**只在出现未见过的 `id` 时才写盘** —— 同一批 endpoint 每 500ms 都进来，无脑写盘会把文件句柄打爆 |
| 去重键 `id` | `proxy::endpoint_identity` 归一后的 `host:port` |
| ⚠️ **loopback 一律不登记** | `127.0.0.1` / `localhost` / `::1` 是**本地代理监听地址**，不是可配置对象，端口还会随重启变。有效 endpoint 口径必须与前端 `displayEndpoint` 一致：命中解析时登记反查出的**上游**，不是代理监听地址 |
| 拓展字段载体 | `EndpointProfile.extensions` 是 JSON map，**加拓展字段不改协议**；流程脚本的 `FlowConfig` 整体存在 `extensions["flow"]` 下，**不给 `EndpointProfile` 结构加字段** |
| 档案内版本号 | `endpoint_profiles.rs` 的 `SCHEMA_VERSION`（当前 `1`）是**这份 JSON 自己的**版本，与 `AgentEvent` 的线协议版本、存档库的 `PRAGMA user_version` 是三套东西，别混（线协议那条见接入指南 §11） |
| 上限 | `MAX_PROFILES = 64`、`MAX_URL_CHARS = 4096` |

- ⚠️ **历史 bug 已锁**：`normalize_host_port` 早期把 `[::1]:8080` 切成 `[::1:8080`（漏了 `]`），会让 loopback 判定失效、本地地址被登记成档案。护栏：`proxy/mod.rs` 的 `normalize_host_port_keeps_ipv6_brackets_and_port`。
- ⚠️ **tick 读配置必须走 `profiles.with_profile(id, …)`**（只 clone 该档案的 `extensions`），**不要用 `snapshot()`** —— 后者整表 clone 含脚本正文（单份上限 64KB × 64 条），每 500ms 一次是纯浪费。同样的纪律写在推送那边（push 篇 §3：取脚本内容只查 `session_fields(key)` 一个 key）。
- 读写分离：`get_endpoint_profiles` 给三窗口；`add_endpoint_profile` / `delete_endpoint_profile` **只在 `main-module-settings.json` 授权**（仅主窗）。
- ⚠️ 档案面板的反馈**不能用 `window.alert` / `confirm`**（装了 `tauri-plugin-prevent-default`，原生对话框会被拦，红线见接入指南 §11）；一律 `role="status"` 行内提示，移除用两步确认。
- 配置面板（齿轮页签 → 组件列表行的齿轮 / 点悬浮窗上的状态灯 → 二级 tab「信息卡拓展」）用左右主从布局配各 endpoint；**版面规范见 `visual-style.md`**。

## 4. 沙盒的能力面：脚本能做什么、碰不到什么

「信息卡拓展」面板右侧让用户为某 endpoint 写一段 JS，返回 `{label, value, color}` 数组，作为信息卡的拓展字段（典型用途：查上游余额）。

- **执行方式**：内嵌 QuickJS（`rquickjs`）。features 只开 `std` / `parallel` / `classes` / `properties`，**刻意不含 `loader` / `dyn-load`** ⇒ 脚本里 `import` / `require` 根本不存在，没有读文件、没有隐式出网路径。护栏：`runner.rs` 的 `module_loader_unavailable`。
- **沙盒三件套**：内存帽 2MB（`MEMORY_LIMIT_BYTES`）+ 中断超时（`timeout_ms`，clamp 到 500ms–15s）+ 无模块加载器。见 `flow/runner.rs`。
- **注入的宿主 API**（脚本唯一能做的事）：

| 全局 | 方法 | 说明 |
|---|---|---|
| `http` | `get` / `post` / `request` | Rust 侧 `ureq` 代发（`HttpFetch`），响应体封顶 256KB；返回对象带 `status` / `ok` / `headers` 与 `text()` / `json()` |
| `secrets` | `get(name)` | 按名字从 OS 凭据管理器取（§8） |
| `cache` | `get` / `set` | 进程内缓存（带 TTL），不给脚本任何磁盘通道 |
| `format` | `num` / `pct` / `bytes` | 纯格式化，避免脚本自己拼小数 |
| `ctx` | **只读** `endpoint` / `session` / `lastResult` | 会话摘要在**入队时**就拷进 job，worker 不回头查 registry / profiles 锁 |
| `console` | `log` / `info` / `warn` / `error` | ⚠️ **只写本次运行的内存缓冲、不广播**（§4.1） |

### 4.1 ⚠️ `console` 不广播

`console` 的输出只进本次运行的环形缓冲（`MAX_LOG_LINES = 50`，超限只留最新），**仅供「运行测试」按钮回显**，绝不进 `flow-results` 广播、绝不进状态日志。原因很直白：脚本作者（或抄来的一段脚本）很可能写 `console.log(secrets.get('KEY'))`，一旦广播就等于把凭据推给三个窗口并可能落盘。行数与单行长度都有帽（`MAX_LOG_CHARS`），防止脚本用日志把内存吃光。

## 5. 为什么出网只收成一个 `http`

⚠️ **要改沙盒能力面之前先读这段。**

脚本是用户原文，安全性完全依赖「他能碰到什么」。QuickJS 本身**没有任何 syscall** —— 没有 `loader` / `dyn-load` feature 就连 `import` / `require` 都不存在，JS 层也没有 fs / net / child_process 这些宿主接口。因此脚本唯一能「影响外界」的路径，就是我们主动 `globals.set` 进去的那几个闭包。收成一条通道换来三件事：

1. **地址策略只有一处可管** —— `validate_http_url` 把 loopback / 私网 / 云元数据挡在发起连接**之前**；若同时开放 socket 或模块加载，端口扫描与 SSRF 根本无从拦起。
2. **资源可控** —— 单次请求 500ms–15s 超时、响应体 256KB 封顶，配 worker 串行队列，不会出现一个脚本把带宽和内存吃光。
3. **可审计、可静默** —— agent 全停 = 零出网（调度只看存活会话，§7），用户能靠「网络有没有动静」判断脚本在干什么；多一条出网路径这条不变量就破了。

✅ 要加新能力（读本地文件、走 IPC 等）**必须同样收敛成一个带策略校验的宿主闭包，绝不能给脚本原生网络**。

## 6. 地址策略与其固有代价

`validate_http_url`（`flow/api.rs`）是发起连接前的唯一一道闸：

| 检查 | 行为 |
|---|---|
| scheme | 只允许 `http` / `https`；`file://`、`gopher://` 等直接拒 |
| 长度 | 空 / 超 4096 拒 |
| host 提取 | 剥掉 userinfo / path / query / fragment；IPv6 先剥 `[...]` 再判端口 |
| 禁段（`is_denied_host`） | `localhost` 与 `.localhost` / `.local` 后缀、loopback、**私网**、链路本地（IPv4 `169.254/16`、IPv6 `fe80::/10`）、unspecified / multicast、**CGNAT `100.64.0.0/10`**（不少 Tailscale 与云内网用它）、IPv6 唯一本地 `fc00::/7`、**云元数据 `169.254.169.254`**、无点短主机名（非 `localhost`） |

- ⚠️ **固有代价：域名解析到内网 IP 挡不住**（不发连接就无法预解，而发连接本身就绕过了这道闸）。这是刻意接受的风险，必须在文档与面板里写明，不许假装它不存在。护栏：`runner.rs` 的 `http_blocks_private_addresses`。
- ⚠️ 这条策略与**推送**那边**刻意不同**：push 的地址是用户在设置页显式填的固定字段，允许局域网自建服务器（挡掉等于砍功能）；flow 那边发的是用户脚本拼出的任意地址。差异的理由写在 push 篇 §7，改任何一边都要同时读那段 —— 不要把它们「统一」成一份校验。
- 错误文案经 `sanitize_http_error` 收敛（超时 / 无法解析主机 / 状态码 / URL 非法 / 重定向失败），**不含堆栈与内部类型名**：脚本侧看得见。

## 7. 调度、去抖与退避

status monitor 每轮（约 500ms）调一次 `FlowState::tick(&snapshot, &profiles, now)`：

| 规则 | 说明与护栏 |
|---|---|
| **只跑存活会话涉及的 endpoint** | 口径是「还在 `StatusSnapshot.sessions` 里」，**不看 phase** —— 黄/绿/闪烁灯都可能被悬停，都需要数据。⚠️ **agent 全停 = 零出网**（这条不变量与推送共享，破坏它的改动不予接受，见 push 篇 §7） |
| 到点才入队 | `tick_skips_before_interval_elapses`；`FlowConfig.enabled` 为假即停止调度、已缓存结果视为作废。⚠️ **新建配置默认 `enabled = false`** —— 用户写完脚本、点保存之前不该自动开始轮询出网（`default_new_config_is_disabled`） |
| ⚠️ **同 endpoint 多会话只跑一次** | `running` 去抖（`tick_skips_when_already_running`），但结果**回填到每个会话 key**（`tick_enqueues_once_per_endpoint_not_per_session`） |
| 会话消失即丢映射 | `tick_drops_mapping_for_vanished_sessions`，结果表不随会话数膨胀 |
| 串行执行 | 专用 worker 线程（`spawn_worker`）+ 队列，**一个脚本一次只跑一份**：一次网络抖动或死循环不能拖住 500ms 的状态循环，也不能让两个脚本并发写同一份状态 |
| ⚠️ 连续失败**指数退避** | `interval × 2^n`、封顶 `MAX_BACKOFF_SEC = 600s`（否则脚本挂了还每秒打网）。`backoff_seconds` 是纯函数，护栏 `backoff_caps_at_max` |
| 锁与 I/O 次序 | tick 与命令只在锁内做纯内存查表；QuickJS 执行与 `emit` 一律在锁外；worker 不回头查 registry / profiles 锁 |
| 保存/删除后立即刷新 | `save_endpoint_flow` / `delete_endpoint_profile` 之后**立刻再广播一次 `flow-results`**（字段已被 `invalidate` 清掉），否则提示窗手里仍是旧脚本的字段、要等下次轮询才刷新。护栏 `invalidate_clears_state_and_cache` |
| 档案删除时清账 | `cleanup_for_profile` 连带清缓存与凭据（§8） |

## 8. 密钥与凭据记账

- `secrets.get(name)` 按名字从 **OS 凭据管理器**取（`keyring`，`KeyringSecrets`），service = `<APP_ID>/flow/<endpoint_id>`。⚠️ 值**不落盘、不进档案、不进日志、不广播**。
- ⚠️ **Windows 凭据管理器不能按 service 列举**，因此档案的 `FlowConfig.secret_names` 里存一份**名字清单** —— 它是删档案时唯一能级联清凭据的依据（`secret_names_for` + `cleanup_secrets_for`）。**清单丢了，凭据就成了孤儿**，只能手工去「控制面板 → 凭据管理器 → Windows 凭据」删。
- ⚠️ `secret_names` 与 `script_hash` 由**服务端权威维护**：`save_endpoint_flow` 会保留旧 `secret_names`、按脚本内容重算 `script_hash`，**不接受前端传入这两项** —— 前端漏传一次就会把凭据记账清空，直接制造孤儿凭据。
- 密钥名本身要过校验（`validate_secret_name`，上限 `MAX_SECRET_NAME_CHARS = 128` 字符）：超长名在凭据管理器里会被截成不可预测的键 → 读不出来是**静默失败**，所以宁可在入口就拒。
- `script_hash`（长度 + FNV-1a，**非加密**）只用于判断「改没改脚本」：脚本没改才把上次结果作为 `ctx.lastResult` 继承（增量语义），改了就不给（否则语义错位）。护栏 `script_fingerprint_changes_only_on_content`、`remember_and_forget_secret_dedupe`。
- ⚠️ 推送那侧**已经不用了**这套凭据路径（主题串按用户要求明文存配置，见 push 篇 §6）；别把 push 的删除当成「flow 也不需要密钥」。

## 9. 结果广播与前端消费

- 事件名 `flow-results`（`FLOW_RESULTS_EVENT`），载荷 `FlowSnapshot`，**按会话 key 索引** —— 提示窗零归一逻辑，直接 `map[session.key]` 查表渲染。
- ⚠️ **广播值类型是 `FlowSessionResult { fields, updated_at_ms, interval_sec }`，不是裸 `FlowField[]`**：信息卡要在字段下方标注「更新于 HH:MM:SS」，让用户分辨屏上是刚跑的值还是旧值（脚本挂掉时字段整块不显，更需要一个时间锚点）。
  - `interval_sec` 仍随结果一路带下来（后端调度与退避要用），但**前端已不展示刷新周期** —— 对用户是噪音。
  - `apply_result` 每次回写都刷新标注；格式化在 `src/modules/agent-status/lib/flow.ts` 的 `formatFlowUpdatedAt`。
  - `isFlowSnapshot` 对**整份结果对象**严格校验：缺新鲜度字段的旧形态直接拒绝，不做「兼容成裸数组」。
- 前端落点：编辑器 `src/modules/agent-status/panels/EndpointFlowEditor.vue`、档案面板 `panels/EndpointExtensionPanel.vue`、纯逻辑 `lib/flow.ts` + `lib/endpoint-profiles.ts`、结果 store `stores/flowResults.ts`、档案 store `stores/endpointProfiles.ts`。
- 结果 store 由**模块定义**的 `activate()` 启动（契约见 floating-module-guide §9），不依赖 `App.vue`：⚠️ 不启动它，提示窗就永远收不到脚本字段（`hasContent` 在内容组件挂载之前就要答案）。
- `run_flow_test`（「运行测试」）**不进调度队列**，日志只回显给主窗口。⚠️ 它必须是 `async` 命令 + `spawn_blocking`：非 async 命令跑在**主线程**，一次测试最坏阻塞 15s 会冻结窗口事件循环；带引用入参的 async 命令按 tauri 宏要求必须返回 `Result`（纪律见 floating-module-guide §12.3）。

## 10. 展示预算与解析上限

脚本返回值由 `parse_fields` 收口，形状与数量都夹死：

| 上限 | 值 | 为什么 |
|---|---|---|
| `MAX_FIELDS` | **4** | ⚠️ 与提示窗高度**绑定**（`tauri.conf.json` 的 260）。加字段数必须同步核算窗口尺寸，超出会被裁掉 —— 一屏预算（head + Endpoint + token + 4 字段 + 刷新标注 + 提问 1 行 ≈ 236px）的权威写法在 floating-module-guide §8.3 |
| `MAX_LABEL_CHARS` / `MAX_VALUE_CHARS` | 24 / 120 字符 | 超长会把行挤成一团；按字符切，不按字节 |
| `MAX_SCRIPT_CHARS` | 64 KB 字符 | 单份脚本正文上限，与 §3 的「别用 `snapshot()`」是同一笔开销的两端 |
| `color` | 非法色值整条丢弃 | 语义色只认信息卡那套 token，不接受脚本给任意 CSS 颜色 |

- 提问行靠 `--compact` 压在**一行**；⚠️ **不要再给它加行数**，否则刷新标注会被挤出可视区。
- 信息卡字段的展示顺序（provider/phase → Endpoint → token → **拓展字段 + 刷新标注** → 提问）由 `AgentStatusTooltip.vue` 决定，拓展字段紧跟 token 行、提问永远在最后，别插回提问下面。

## 11. rquickjs 与 panic 纪律

- ⚠️ **rquickjs 闭包两个坑（都踩过）**：
  1. 参数的生命周期必须写成外层**具名 `'js`**，不能写 `'_` —— 否则 E0106 / lifetime may not live long enough。
  2. 可选参数用 `prelude::Opt`，**不是** `Option` —— `Option` 没实现 `FromParam`，会被当成**必填**参数（`http.get(url)` 少传 opts 就当场报错）。
- ⚠️ **release 是 `panic = "abort"`**：`flow` 全模块的宿主闭包**不得 panic**（不 `unwrap` / `expect` / 裸索引 / `slice`）。失败一律变 `Err(String)` 或跳过本轮 —— 一次 panic 会带走整个 App，而脚本正文是用户任意写的东西，panic 概率远高于其他模块。护栏：`thrown_error_becomes_err_without_panic`（脚本里 `throw` 也只是 `Err`）、`infinite_loop_times_out_not_hangs`（死循环被中断超时收住，不是把 worker 挂死）。
- 通用那条 panic 红线见 floating-module-guide §12.4 与接入指南 §8；本篇只是它在沙盒侧的具体化。

## 12. 落点、命令与授权

| 层 | 位置 |
|---|---|
| 解析器契约 | `proxy/mod.rs`（`ProxyResolver` / `ResolverRegistry` / `build_registry` / `endpoint_identity` / `is_loopback_endpoint`） |
| cc-switch 实现 | `proxy/ccswitch.rs`（`upstream_from_settings` / `app_key` / `CACHE_TTL`） |
| 档案表 | `endpoint_profiles.rs`（`EndpointProfileState::with_profile` / `record_all` / `update_extensions` / `remove`） |
| 配置块 | `flow/config.rs`（`FlowConfig`、区间常量、`script_fingerprint`、`validate_secret_name`） |
| 沙盒 | `flow/runner.rs`（注入全局、`parse_fields`、内存帽与中断超时） |
| 宿主 API | `flow/api.rs`（`HttpFetch` / `validate_http_url` / `KeyringSecrets` / `CacheStore`） |
| 调度与命令 | `flow/mod.rs`（`FlowState::tick` / `apply_result` / `invalidate` / `spawn_worker` / `cleanup_secrets_for`） |
| 前端 | `panels/EndpointExtensionPanel.vue`、`panels/EndpointFlowEditor.vue`、`lib/flow.ts`、`lib/endpoint-profiles.ts`、`stores/flowResults.ts`、`stores/endpointProfiles.ts` |

命令与授权（注册三处的口径见 floating-module-guide §12.5）：

| 命令 | 授权 |
|---|---|
| `get_endpoint_profiles` / `get_flow_results` / `get_flow_snapshot` | 三个窗口（`capabilities/default.json` 的 `allow-get-flow-results` 等） |
| `save_endpoint_flow` / `run_flow_test` / `set_flow_secret` / `delete_flow_secret` / `add_endpoint_profile` / `delete_endpoint_profile` | **只主窗**（`capabilities/main-module-settings.json`） |

事件：`endpoint-profiles`（档案变化）、`flow-results`（脚本结果）。配置写回仍走外壳的 `updateModuleSettings('agent-status', …)`，不新增保存命令。

## 13. 红线汇总

| 做法 | 后果 |
|---|---|
| 给脚本加任何一条新的外界通道（fs / net / IPC / 模块加载） | ❌ 三条不变量当场破两条：地址策略无法集中、agent 全停不再等于零出网。必须收敛成带策略校验的宿主闭包（§5） |
| 让 `console` 进广播或进日志 | ❌ 脚本一句 `log(secrets.get('K'))` 就把凭据推给三个窗口（§4.1） |
| 在 `validate_http_url` 之后才判地址、或给 push/flow 共用一份校验 | ❌ 两边的地址来源性质不同，统一 = 要么砍掉局域网自建推送服务器，要么放开脚本打私网（§6） |
| 登记 loopback 地址当档案 | ❌ 本地代理不是可配置对象、端口随重启变，档案表会被垃圾行灌满（§3） |
| tick 用 `snapshot()` 读配置 | ❌ 每 500ms clone 整表含脚本正文（64KB × 64）。必须 `with_profile(id, …)`（§3） |
| 接受前端传入 `secret_names` / `script_hash` | ❌ 前端漏传就把凭据记账清空，凭据成孤儿（§8） |
| 删档案时不清 `secret_names` 对应的凭据 | ❌ Windows 凭据管理器无法按 service 列举，漏了就再也清不掉（§8） |
| 读 `settings_config` 里的 `auth` 段 / 任何 token 类 key | ❌ 只按名字取 base_url 一个 key 是本子系统的保密底线（§2.3） |
| `listen_port` 按 TEXT 读 | ❌ 整行 `InvalidType`，解析器静默空转（§2.1） |
| 只看 `proxy_enabled` 或只看 `enabled` | ❌ 「服务器开着但该 app 未接管」时误命中（§2.1） |
| 广播裸 `FlowField[]` | ❌ 信息卡失去「更新于几点」这个锚点，脚本挂掉时用户无法分辨屏上是新值还是旧值（§9） |
| 为 `upstream_url` / `tokens` 升 `SCHEMA_VERSION` | ❌ 新旧二进制互相丢事件且静默（接入指南 §11） |
| 给拓展字段加第 5 条、或给提问加行数 | ❌ 260px 的窗口装不下，标注会被挤出可视区（§10） |
| 宿主闭包里 `unwrap` / `expect` / 索引 / `slice` | ❌ release 是 `panic = "abort"`，一次 panic 带走整个 App（§11） |
| `run_flow_test` 写成非 async 命令 | ❌ 一次测试最坏冻结主线程 15s（§9） |
| 用 `window.alert` / `confirm` 做面板反馈 | ❌ 被 `tauri-plugin-prevent-default` 拦（接入指南 §11） |

## 14. 验收清单

**自动化**

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`，其中本篇的护栏命名测试至少覆盖：
  - 解析器：`normalize_host_port_keeps_ipv6_brackets_and_port`、`resolve_maps_local_proxy_to_current_upstream`、`resolve_returns_none_when_listen_port_mismatches`、`resolve_returns_none_when_proxy_disabled_for_app`、`resolve_returns_none_when_global_server_off_even_if_app_enabled`、`resolve_is_app_scoped_codex_proxy_disabled`、`claude_settings_read_only_env_base_url`、`codex_settings_read_base_url_from_embedded_toml`、`codex_settings_ignore_the_auth_block`；
  - 沙盒：`module_loader_unavailable`、`http_blocks_private_addresses`、`infinite_loop_times_out_not_hangs`、`thrown_error_becomes_err_without_panic`、`parse_fields_accepts_three_shapes`、`parse_fields_drops_illegal_color_and_caps_count`、`parse_fields_truncates_by_chars`、`ctx_session_and_endpoint_visible`、`secret_get_returns_value_missing_throws`；
  - 调度：`tick_enqueues_once_per_endpoint_not_per_session`、`tick_skips_when_already_running`、`tick_skips_before_interval_elapses`、`tick_drops_mapping_for_vanished_sessions`、`tick_ignores_loopback_and_missing_config`、`backoff_caps_at_max`、`invalidate_clears_state_and_cache`；
  - 配置与凭据：`script_fingerprint_changes_only_on_content`、`clamps_interval_and_timeout`、`remember_and_forget_secret_dedupe`、`validate_secret_name_rejects_bad`、`roundtrips_through_extensions`；
  - 档案：`records_non_local_endpoint_and_persists`、`skips_loopback_addresses`、`manual_add_rejects_loopback_and_blank`、`dedupes_by_identity_and_reports_no_change`（同一批 endpoint 每 500ms 进来也不重复写盘）、`caps_profile_count`、`corrupt_file_falls_back_to_empty`、`drops_legacy_entries_with_unknown_schema`、`ignores_blank_and_unparseable_values`；
  - 默认态：`default_new_config_is_disabled` —— 新建脚本配置**默认不启用**，用户写完脚本、点保存之前不应自动开始轮询出网。
- [ ] `npx vitest run`：`lib/flow.spec.ts`（`isFlowSnapshot` 对整份对象严格校验、缺新鲜度字段直接拒 / `formatFlowUpdatedAt`）、`lib/endpoint-profiles.spec.ts`、`panels/EndpointFlowEditor` 与档案面板的 store 契约。
- [ ] `npm run build`（`vue-tsc --noEmit` + vite）过。

**人工（真机）**

- [ ] 开 cc-switch 并接管 claude → 信息卡 Endpoint 显示**上游**而不是 `127.0.0.1:<port>`；切 provider 后 ≤5s 跟着变。
- [ ] 同一套设置下让 Codex 会话跑一轮 → Endpoint 也必须显示反查出的上游（回归：曾经的 bug 就是这里恒为 localhost）。
- [ ] 档案列表里**不应**出现 loopback 行；手动添加 `http://127.0.0.1:123` 要被拒。
- [ ] 写一段 `return [{label:'x',value:1,color:'#fff'}]` → 信息卡出现拓展字段与「更新于」；写 `throw new Error('x')` → 字段整块消失、App 不崩、连续失败后退避生效。
- [ ] 脚本里 `await http.get('http://127.0.0.1:8080/')` → 返回被拒的错误文案；`require('fs')` → 直接 ReferenceError。
- [ ] 全部 agent 停掉 → `flow-results` 不再更新、零出网（可用抓包或防火墙确认）。
- [ ] 配一个密钥 → 删档案 → 凭据管理器里那条 `com.tauri-app.my/flow/<endpoint_id>` 一起消失。
- [ ] 「运行测试」期间主窗不冻结（一次跑满 15s 也不卡）。

## 附录：常量与不变量速查

| 项 | 值 | 定义处 |
|---|---|---|
| QuickJS features | `std` / `parallel` / `classes` / `properties`（**无** `loader` / `dyn-load`） | `src-tauri/Cargo.toml` |
| 沙盒内存帽 | 2 MB | `flow/runner.rs` `MEMORY_LIMIT_BYTES` |
| 脚本正文上限 | 65536 字符 | `flow/runner.rs` `MAX_SCRIPT_CHARS` |
| 脚本超时 | 500ms–15000ms，默认 8000ms | `flow/config.rs` `MIN/MAX/DEFAULT_SCRIPT_TIMEOUT_MS` |
| 轮询间隔 | 10–3600s，默认 60s（下限是「低于它对上游就是骚扰」） | `flow/config.rs` `MIN/MAX/DEFAULT_INTERVAL_SEC` |
| HTTP 超时 / 响应体 | 500ms–15000ms / 256 KB | `flow/api.rs` `MIN/MAX_HTTP_TIMEOUT_MS`、`MAX_RESPONSE_BYTES` |
| 拓展字段数 | 4（`MAX_FIELDS`），label 24 / value 120 字符 | `flow/runner.rs` |
| console 缓冲 | 50 行 × 500 字符，仅测试回显 | `flow/runner.rs` `MAX_LOG_LINES` / `MAX_LOG_CHARS` |
| 失败退避 | `interval × 2^n`，封顶 600s | `flow/mod.rs` `MAX_BACKOFF_SEC`、`backoff_seconds` |
| 解析器缓存 TTL | 5s | `proxy/ccswitch.rs` `CACHE_TTL` |
| cc-switch 库打开方式 | `SQLITE_OPEN_READ_ONLY` + `busy_timeout` 200ms | `proxy/ccswitch.rs` |
| 档案上限 | 64 条 / URL 4096 字符 / 文件内 `SCHEMA_VERSION = 1` | `endpoint_profiles.rs` |
| 密钥名上限 | 128 字符 | `flow/config.rs` `MAX_SECRET_NAME_CHARS` |
| 凭据 service | `<APP_ID>/flow/<endpoint_id>` | `flow/api.rs` `KeyringSecrets` |
| 事件名 | `endpoint-profiles`、`flow-results` | `endpoint_profiles.rs`、`flow/mod.rs` |
| 提示窗高度 | 260px（与 `MAX_FIELDS` 绑定） | `src-tauri/tauri.conf.json` |

**相关文档**：`agent-status-session-data.md`（`base_url` / `upstream_url` / token 在快照与存档里的走法）· `../rule/agent-status-hooks.md` · `../guide/agent-status-platform-onboarding-guide.md` · `../guide/agent-status-push-guide.md` · `../guide/floating-module-guide.md` · `visual-style.md`
