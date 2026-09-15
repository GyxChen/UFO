# Agent 代理的使用细节（cc-switch 上游反查）

> 本文描述 UFO **当前实现**如何把「被本地代理接管的 agent 会话」的 `base_url` 反查成真实上游 endpoint，
> 以及在 tooltip 里怎么展示。所有结论都对照了 cc-switch **v3.20.3** 的源码（本机克隆于
> `D:\workspace\tmp\cc-switch-main`）与本机真实 `~/.cc-switch/cc-switch.db` 实测，不是设计意图。
>
> 相关：架构决策见 `悬浮窗Agent状态监测与模块化实施方案.md`；本文属于实现细节。

## 1. 要解决的问题

cc-switch 的「本地代理」模式会改写 `~/.claude/settings.json`，把 `env.ANTHROPIC_BASE_URL` 指向自己监听的
回环地址，真实上游只存在 cc-switch 自己的 SQLite 里。实测本机：

```
~/.claude/settings.json:
  "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721"
  "ANTHROPIC_AUTH_TOKEN": "PROXY_MANAGED"      ← token 被托管成占位符，真 token 在 cc-switch 库里
```

于是 UFO 的 tooltip 只显示 `127.0.0.1:15721`，没有任何信息量 —— 看不出这个会话到底打到了哪家模型服务。
**「代理解析器」就是把本地地址反查成上游**，且全程只读、绝不外泄 token。

## 2. 端到端数据流

```
① hook 采集会话 base_url（配置文件优先，环境变量兜底）
   ~/.claude/settings.json → env.ANTHROPIC_BASE_URL = http://127.0.0.1:15721
                 ↓ AgentEvent.base_url
② reducer 存进 SessionSnapshot.base_url（同轮继承，新事件没带不抹掉）
                 ↓ snapshot()
③ StatusState::attach_upstreams —— 按「当前选中的解析器」逐会话反查
                 ↓
   CcSwitchResolver.resolve_upstream(app, base_url)
     ├─ proxy_config：本 app 是否被代理接管、监听在哪个 host:port
     └─ providers  ：is_current=1 的 settings_config → env.ANTHROPIC_BASE_URL（真实上游）
                 ↓ SessionSnapshot.upstream_url
④ 前端 displayEndpoint(session) = upstream_url ?? base_url（上游优先、回落本地）
```

| 环节 | 位置 |
|---|---|
| ① | `src-tauri/src/status/hook.rs` → `provider_base_url` |
| ② | `src-tauri/src/status/reducer.rs` |
| ③ | `src-tauri/src/status/mod.rs` → `attach_upstreams` / `upstream_signature` |
| ③ 解析器 | `src-tauri/src/proxy/mod.rs`、`src-tauri/src/proxy/ccswitch.rs` |
| ④ | `src/lib/agent-status.ts` → `displayEndpoint`；`src/components/TooltipWindow.vue` |

## 3. cc-switch 侧的真实结构（读它才能反查）

### 3.1 `proxy_config`：两个不同层级的开关

表是**按 app 三~四行**结构（`app_type TEXT PRIMARY KEY CHECK (app_type IN ('claude','codex','gemini','grokbuild'))`）。
关键是 `proxy_enabled` 与 `enabled` **语义完全不同**（源码 `src/proxy/types.rs`、`src/database/dao/proxy.rs`、
`src/services/proxy.rs`）：

| 列 | 类型 | 对应结构 | 语义 | 写入方 |
|---|---|---|---|---|
| `proxy_enabled` | INTEGER | `GlobalProxyConfig.proxy_enabled` | **代理服务器总开关**，服务器起来才为 1；`stop()` 时置 0 | `update_global_proxy_config` 用**无 WHERE 的 UPDATE**，故三行同值镜像 |
| `enabled` | INTEGER | `AppProxyConfig.enabled` | **该 app 是否被代理接管**，每 app 独立 | `takeover_single_proxy` 只翻本 app 行；`stop_with_restore` 逐 app 清零 |
| `listen_address` | TEXT | — | 监听地址，默认 `127.0.0.1` | 全局配置 |
| `listen_port` | **INTEGER** | — | 监听端口，默认 `15721` | 全局配置 |

> `live_takeover_active` 是旧版兼容列，源码注释明确「不再使用，由 `enabled` 替代」。

### 3.2 `providers`：真实上游在哪

```
SELECT settings_config FROM providers WHERE app_type = ?1 AND is_current = 1 LIMIT 1
```

`settings_config` 是一整块 JSON，Claude 走 `env` 块：

```json
{ "env": {
    "ANTHROPIC_BASE_URL": "https://api.b.ai",     ← 我们只取这一个 key
    "ANTHROPIC_AUTH_TOKEN": "***",          ← 绝不读取 / 缓存 / 上报
    "..." : "..."
} }
```

### 3.3 为什么不需要读 `settings.json` 的 `enableLocalProxy`

`~/.cc-switch/settings.json` 里有个 `enableLocalProxy`（前端 `App.tsx` 用的 UI 门控），UFO **不读它**。
原因：cc-switch 关闭代理接管时会把 `ANTHROPIC_BASE_URL` 改回真实上游，会话的 `base_url` 便不再匹配
`proxy_config` 的本地监听地址，反查自然不命中 —— 判据已经隐含在「会话实际连哪儿」里，少读一个文件、少一处不一致。

## 4. UFO 侧实现

### 4.1 抽象：接新代理工具只需注册一行

```rust
pub trait ProxyResolver: Send + Sync {
    fn id(&self) -> &'static str;          // 存进配置、作下拉 value
    fn label(&self) -> &'static str;       // 下拉显示文本
    fn resolve_upstream(&self, app: AgentApp, local_base_url: &str) -> Option<String>;
}
```

`build_registry(home)` 里 `push` 一行即接入：主窗口下拉、`upstream_url` 填充、持久化**全部自动适配，前端零改动**。
`AgentApp`（Claude/Codex）独立于 `Provider`，将来解析器要支持更多 agent app 不改状态协议。

实现约束（写在 trait 文档注释里）：**只读、快速、不 panic** —— 它在生成快照的热路径上被调用。

### 4.2 命中判据

```
命中 = (proxy_enabled == 1) AND (enabled == 1)              ← 两个开关都要开
       AND normalize_host_port(base_url) == normalize_host_port(listen_address:listen_port)
```

取两列之**与**是本轮依据 cc-switch 源码修正的点：只看 `proxy_enabled` 会在「服务器开着但该 app 未接管」时误命中。

### 4.3 `normalize_host_port`：地址等价判定

反查要比较「会话连的地址」和「代理监听地址」，不能字符串直接相等，所以做了归一化（`proxy/mod.rs`）：

- 剥离 scheme、userinfo、path/query/fragment。
- IPv6 `[::1]:port` 与普通 `host:port` 都处理。
- loopback 等价写法（`localhost` / `127.0.0.1` / `::1` / `[::1]`）统一成 `127.0.0.1`。
- 省略默认端口（http→80、https→443）。
- host 带非数字端口 → `None`（判定为不匹配，不解析）。

未引 `url` crate：只需要「够比端口」的粗解析，少一个依赖。

### 4.4 缓存：缓存「代理长什么样」，不是「某地址解析成什么」

`AppProxyInfo { listen_addr, upstream }` 按 `app_type` 缓存，TTL **5 秒**，结果（含 None）也写缓存。

这个区分是有意的：同一 app 下不同会话可能连不同 `base_url`（有的经代理、有的直连），
**匹配必须每次按各自 base_url 现算**；但代理的监听地址与当前上游是全局唯一的，缓存它才对任意 base_url 成立。
缓存整份解析结果会让「直连会话」错误复用「代理会话」的上游。

5 秒的取值权衡：cc-switch 里手动切 provider 后，最迟 5s 反映到悬浮窗；同时让 monitor 每 500ms 的热路径
不必每次都开 SQLite。

### 4.5 SQLite 访问姿势

| 约束 | 原因 |
|---|---|
| `SQLITE_OPEN_READ_ONLY` | 库被 cc-switch 应用持有，只读避免争写锁 |
| `busy_timeout(200ms)` | 库被占时快速失败，不长时间阻塞热路径；失败即回落 None |
| 每次短连接 | 无常驻句柄，不与 cc-switch 的生命周期耦合 |
| 只 SELECT 需要的列 | 见 4.6 |

### 4.6 密钥边界

**如实描述**：代码 `SELECT settings_config` 会把**整块 JSON 读进内存**（含 `ANTHROPIC_AUTH_TOKEN`），
但解析时 `value.get("env")?.get("ANTHROPIC_BASE_URL")?` **只按名字提取这一个 key**，
token 不进缓存、不进快照、不发给前端、不写日志。

之所以不写成「绝不读取 token」：字面上不精确，会被误解成 SQL 层做了列内过滤（SQLite 侧没法只取 JSON 的一个 key）。

### 4.7 上游变化的推送：`upstream_signature`

用户在 cc-switch 里切 provider **不产生 hook 事件**，光靠 `changed` 标志推不动快照。
因此 monitor 每 500ms 在启用解析器时算一份「key → upstream」有序签名，与上轮不同就 `emit_snapshot`。

⚠️ 这里有个已修的锁纪律问题：`registry.lock()` 的 guard 若存活贯穿整个 `.collect()`，
就变成**持有 registry 锁去读 SQLite**（最坏 200ms），会阻塞 `apply_event`。
现实现是：锁内只拷出 `(key, provider, base_url)` 三元组随即释放，锁外逐会话反查。

### 4.8 快照字段与协议兼容

- `SessionSnapshot.upstream_url: Option<String>` + `#[serde(default)]`，**不是事件派生字段**，由 `snapshot()` 时现算填入。
- `reducer.rs` 在构造 `SessionSnapshot` 时恒置 `upstream_url: None`（注释指向上面这句）。
- ⚠️ **不得升 `SCHEMA_VERSION`**：一旦升版，旧 hook 二进制写出的 inbox 事件会被静默丢弃。

## 5. 配置与交互

| 项 | 值 / 行为 |
|---|---|
| 配置字段 | `FloatingModuleConfig.proxy_resolver: String`，默认 `none`（`NONE_RESOLVER`） |
| 旧配置兼容 | 缺字段或空串（trim 后为空）回落 `none`，与加该设置前行为一致 |
| 下拉内容 | 来自 `get_proxy_resolvers` → `ResolverRegistry::options()`，`不解析（直连）` 恒在首位 |
| 下拉选的是 | **解析器工具**，不是代理内部的 provider（provider 由 cc-switch 的 `is_current` 决定） |
| 生效时机 | `set_floating_module_config` 写入路径同步调 `proxy.set_selected(...)`，**切换即时生效、无需重启** |
| 重启后 | `lib.rs` setup 里 `ProxyState::new(None)` 后 `set_selected(&module_state.proxy_resolver())`，不会回落 `none` |
| 未装 cc-switch | `~/.cc-switch/cc-switch.db` 不存在 → `root = None` → 恒不解析，回落本地地址，不报错 |
| 权限 | 新增 command `get_proxy_resolvers`，permission `allow-get-proxy-resolvers`，已加入 `capabilities/default.json`（main + floating + tooltip） |

### tooltip 的 Endpoint 行

```
主行：displayEndpoint = upstream_url ?? base_url（两者都无显示 —）
title：命中上游且与原值不同 → 「经本地代理 <本地地址>」；否则显示完整 base_url
```

即：**上游优先、回落本地**，同时保留「它其实走了本地代理」这条信息在悬浮提示里。

## 6. 已验证的真实行为与已知限制

### 6.1 本机实测（修 bug 时写的临时探针，跑完即删，不留在仓库）

```
REALHOME claude = Some("https://api.b.ai")
REALHOME codex  = None
```

配合 `netstat`：`127.0.0.1:15721 LISTENING`（cc-switch 进程）。数据库里 claude 与 codex 行的
`proxy_enabled=1`、`enabled=1` 均为真。

### 6.2 已知限制（有意为之，非缺陷）

| 限制 | 说明 / 后果 |
|---|---|
| **只反查 Claude，不反查 Codex** | Codex 的 base_url 不在 `env.ANTHROPIC_BASE_URL`，而在 `settings_config.config` 的**内嵌 TOML** 里。当前解析器不碰 TOML，Codex 会话命中代理后 `upstream = None`，tooltip 回落显示本地地址 —— 优雅降级，不会误显示 |
| **不覆盖 gemini / grokbuild** | cc-switch 的 `app_type` 有 4 个取值，但 UFO 的状态协议目前只有 claude / codex 两路 provider。要支持得先扩 `Provider` |
| **切换上游最迟 5s 可见** | TTL 决定；期间改库不重读 |
| **endpoint 明文入 inbox** | `upstream_url` 由 status 侧现算、**不落盘**；但 `base_url` / `prompt` 是明文写 `%APPDATA%/<id>/status/inbox/*.json`（约 500ms 内消费删除）。这是既有已知代价，与本文无关但同属 Endpoint 链路 |
| **依赖 cc-switch 的文件布局** | `~/.cc-switch/cc-switch.db` 与两张表的列名/类型是硬约定。cc-switch 若改 schema，反查会静默回落（读不到就 None），不影响其它状态显示 |

### 6.3 踩过的两个坑（回归风险最高，改动时先看这里）

1. **`listen_port` 必须按整数读**。真实 schema 是 `INTEGER`，rusqlite 的 `String::column_result` 只接受 Text，
   读 INTEGER 列返回 `InvalidType` → `query_row` **整行失败** → `listen` 恒 None → 功能整体空转，
   而且**不报错、看起来只是"没解析出来"**。
   更糟的是测试夹具当时把 `listen_port` 建成了 `TEXT`，那 8 个 ccswitch 测试全绿也照样发现不了。
   → 现在夹具建表用 `listen_port INTEGER`（值按整数插入），与真实库的 `INTEGER NOT NULL DEFAULT 15721` 同型，
   注释里写明「改回 TEXT 测试就失去保护」。

2. **两个开关不能只看一个**。`proxy_enabled` 是全局服务器开关、`enabled` 是本 app 接管开关，
   见 3.1。测试分别覆盖「全局开、本 app 未接管」与「全局关、本 app 残留开」两种组合都**必须不解析**。

## 7. 测试覆盖

**Rust（proxy 13 个：`proxy::tests` 4 + `proxy::ccswitch::tests` 9）**

`proxy::tests`
- `normalize_host_port_handles_common_forms` — scheme/localhost/默认端口/userinfo/query 归一
- `normalize_host_port_rejects_host_with_nonnumeric_port`
- `matches_listen_address_compares_normalized` — 匹配、端口不匹配、真实上游不匹配本地
- `options_list_none_first_then_resolvers` — `none` 恒在首位且 cc-switch 已注册

`proxy::ccswitch::tests`
- `base_url_from_settings_reads_only_env_base_url` — 只取 base_url、顺带 trim
- `base_url_from_settings_rejects_missing_or_blank_or_bad_json` — token 在场但缺 base_url 时**不得**误返回
- `resolve_maps_local_proxy_to_current_upstream` — 端到端：INTEGER 端口 + 双开关为 1 → 解析出上游
- `resolve_returns_none_when_listen_port_mismatches`
- `resolve_returns_none_when_proxy_disabled_for_app` — `enabled=0` 不解析
- `resolve_returns_none_when_global_server_off_even_if_app_enabled` — `proxy_enabled=0` 不解析
- `resolve_is_app_scoped_codex_proxy_disabled` — 按 app 分流
- `cache_holds_stale_value_within_ttl` — 改库后 TTL 内仍返回旧值
- `new_without_db_disables_resolution` — 无 db 时 `root=None`、恒不解析

夹具：`seed_db(temp, listen_port, global_proxy_on, claude_taken_over, upstream)`（真实列类型 + 两列开关），
`seed_taken_over(temp, upstream)` 为常用「双开 + 15721」快捷方式。

**TypeScript（3 个，`src/lib/agent-status.spec.ts` 的 `describe('displayEndpoint')`）**
- 上游优先于本地 `base_url`
- 无上游时回落 `base_url`
- 两者都缺返回 `null`

全量：Rust **143 passed** / TS **76 passed**（6 files）。

## 8. 接入新代理工具的清单

1. 在 `src-tauri/src/proxy/` 下新建模块，实现 `ProxyResolver`（id / label / `resolve_upstream`）。
2. 在 `build_registry` 里多 `push` 一行。
3. 只读、短超时、TTL 缓存；失败一律回落 `None`。
4. 明确"只提取 endpoint 类 key"，把凭据类 key 挡在缓存与快照之外。
5. 前端与持久化不用改；下拉会自动出现新选项。
6. 回到本文补一节：该工具的配置在哪、哪个字段表示"已接管"、真实列类型是什么。
