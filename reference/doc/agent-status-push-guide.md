# Agent 状态灯 · 消息推送（Push）规范

> **适用范围**：给「Agent 状态灯」模块加一条**把灯态变化推到手机**的链路 —— ntfy、Bark，以及将来更多的投递服务。
> **一句话版本**：一个服务只有一个可配字段（服务器地址）+ 若干行「名字 / 主题」；灯态只从内核已有的四种语义取，不自造判定；加一个 provider = Rust 一个文件 + 两处各一行登记，面板不写新组件。
> **与其它文档的关系**：灯态语义见 `agent-status-hooks.md`；模块/外壳边界与两级 tab 见 `AGENTS.md`；本篇只讲推送这一件事的契约与红线。
> **版面**：设置页采用 `reference/mock/push/push-a-dual-pane.html`（方案 A · 双栏主从），与「信息卡拓展」同构。

## 目录

1. [模型与开关层级](#1-模型与开关层级)
2. [配置块结构](#2-配置块结构)
3. [什么时候发、发什么](#3-什么时候发发什么)
4. [Provider 契约](#4-provider-契约)
5. [加一个 provider](#5-加一个-provider)
6. [主题不是密钥](#6-主题不是密钥)
7. [出网纪律：退避与零出网](#7-出网纪律退避与零出网)
8. [统一测试发送](#8-统一测试发送)
9. [前端分区](#9-前端分区)
10. [实现落点](#10-实现落点)
11. [红线与验收](#11-红线与验收)
12. [已知代价与孤儿凭据](#12-已知代价与孤儿凭据)

---

## 1. 模型与开关层级

```
push（总开关 + 事件勾选 + 提醒内容勾选）
 ├── provider: ntfy  { enabled, server }
 │    └── channels[] { id, name, topic }     ← 一行 = 一个收件地址
 └── provider: bark  { enabled, server }
      └── channels[] { id, name, topic }     ← Bark 的 topic 就是设备密钥串
```

- **provider** = 一种投递服务，代码里注册，用户不能增删类型。一个服务**只有服务器地址**这一个配置。
- **channel** = 该服务下的一行**主题**（名字 + 主题串），用户可任意增删。Bark「一串密钥一台设备」与 ntfy「可订阅多个主题」都落在这同一层，因此**不需要为任何服务特判**，UI 也只有一套列表。
- 两层开关是**与**关系：`push.enabled` ∧ `provider.enabled` 才发。主题行**没有自己的开关**（曾经有，按「精简」要求删掉了 —— 一行不想发就删掉那一行）。
- 一次灯态变化 = 给该服务下**每一行主题各发一条**（不是挑一条发）。

⚠️ 主题行的 `id` 必须是**稳定随机串**（`ch_` + 8 位十六进制，`newChannelId()`）：名字可改、顺序会变，增删定位与退避账都按 id 走。

## 2. 配置块结构

整体存在本模块自己的配置块里：`module_settings['agent-status'].push`。外壳（`floating/module_config.rs`、`stores/floatingModules.ts`）只按 id 整块透传，**不解释任何字段**；解析与夹取在 Rust 的 `modules/agent_status/push/config.rs` 与前端的 `lib/push.ts` 各写一份，字段名一致。

```jsonc
{
  "enabled": false,
  "events": ["turn_finished", "waiting_for_user", "failed"],   // 哪些状态要推，见 §3
  "content": [],                                               // 提醒内容，见 §3；默认一条额外内容都不带
  "providers": {
    "ntfy": { "enabled": true, "server": "https://ntfy.sh" },
    "bark": { "enabled": true, "server": "" }                   // 空 = 用 provider 自带的官方地址
  },
  "channels": [
    { "id": "ch_7f3a9c01", "provider": "ntfy", "name": "家里电脑", "topic": "ufo-gyx-home" }
  ]
}
```

- **新增字段一律 optional + 默认值**，缺字段 = 用默认，坏字段只影响它自己（逐字段宽松取值）。⚠️ 这条是本项目硬规则：**不升任何 schema 版本号**，旧配置文件要能被新程序原样读出来。
- **旧键的读取回落**：`providers[].server` 与 `channels[].topic` 都还接受老版本存在通用 `fields` map 里的写法（`read_legacy` / `readLegacy`），漏了这条会**静默丢光**用户已有的 ntfy 主题 —— 两边各有一条命名测试钉住它。新键优先于旧键。
- **写入是整块替换**：`pushPatch` 每次把整份 `push` 交回外壳浅合并落盘，所以被删掉的功能（节流 / 静默 / 标题前缀 / 收件方开关 / `secret_names`）在用户第一次改动任何推送设置时就从文件里消失了，**不需要迁移代码**。
- 长度上限：名字 48 字符、主题 256、地址 4096、主题行数 32；全部按**字符**截断（按字节会把中文截成半个字）。

## 3. 什么时候发、发什么

触发源固定为内核已有的四种语义，**推送层不得自己判定灯态**（否则会和悬浮窗的灯不一致）：

| 内核语义 | 推送事件名 | 默认 | 标题 | 正文固定那一行 |
|---|---|---|---|---|
| `TurnFinished` | `turn_finished` | ✅ 开 | `UFO · 一轮跑完` | `claude 一轮跑完` |
| `WaitingForUser` | `waiting_for_user` | ✅ 开 | `UFO · 在等你` | `claude 在等你` |
| `Failed` | `failed` | ✅ 开 | `UFO · 本轮出错` | `claude 本轮出错` |
| `SessionFinished` | `session_finished` | ❌ 关 | `UFO · 会话结束` | `claude 会话结束` |

标题前缀 `UFO` 是 `message.rs` 里的常量 `PUSH_TITLE_PREFIX`，**不再可配**：多设备 / 多人共用一个主题时，靠下面勾选的 endpoint 与会话行区分来源。

**正文 = 固定一行 + 勾选的若干行**（`config.content`，每项一行 `标签: 值`，取不到值就整行不出现）：

| 取值 | 正文标签 | 内容 |
|---|---|---|
| `endpoint` | `endpoint:` | `upstream_url ?? base_url`（与 tooltip 的 `displayEndpoint` 同一口径） |
| `prompt` | `提问:` | 最近一次用户提问原文（压成一行、截到 300 字） |
| `script` | `脚本:` | 信息卡拓展那几行的当前值（`余额=¥12.30、用量=80%`） |
| `session_id` | `会话:` | 完整会话 key `provider:session_id` |

⚠️ **「哪个平台的哪类状态」是固定发的**，没有开关 —— 一条不含状态的推送没有意义。
⚠️ 四项**默认全不勾**：勾了才外发。取脚本内容时才读 `FlowState::session_fields(key)`（只查这一个 key，别用 `snapshot()` 整表 clone）。

- 事件名是**线协议字符串**（存配置、进广播），取值集合只能增不能改语义。
- ⚠️ **默认只发状态，不带任何内容项**。ntfy 的主题是公开可订阅的：任何知道主题串的人都能收到并长期留存；提问可能含密钥与私有代码，而出网后不受我们控制。因此「带上提问」这类勾选必须在 UI 上写清风险（`PUSH_CONTENT_OPTIONS` 的 hint 就是干这个的），并且**永远不要把它做成默认值**。
`waiting_reason`（等待理由）刻意不在四项里：它由平台注入、比用户输入更容易含敏感上下文，需要时再单独评估。
- provider 自己决定怎么落地这份通用文案（ntfy 把标题放 `Title` 头、正文放 body；Bark 用 URL 路径段 + JSON 的 `title`/`body`），**通用模型里没有 provider 专属字段**。

## 4. Provider 契约

Rust 侧 `modules/agent_status/push/providers/<id>.rs` 实现一次：

```rust
pub struct ServerSpec { pub label: &'static str, pub default: &'static str, pub help: &'static str }
pub struct SendRequest<'a> { pub server: &'a str, pub topic: &'a str, pub message: &'a PushMessage }

pub trait PushProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    /// 服务器地址那一栏的文案与官方默认地址。
    fn server(&self) -> ServerSpec;
    /// 真正发一条，成功返回 HTTP 状态码；失败返回 Err(可行动原因)，**绝不 panic**。
    fn send(&self, req: &SendRequest<'_>, http: &dyn HttpFetch) -> Result<u16, String>;
}
```

- **没有通用字段表**（曾有 `FieldSpec` / `FieldKind` / `sanitize`，服务只剩一个地址后就是空转的抽象，已整体删除）。服务级地址由 `ServerSpec` 给文案与默认值，收件方固定是「名字 + 主题」两列 —— 加一个服务不需要写新组件，但也**不需要**再声明字段。
- `server` 传的是**用户填的原样值**（可能为空串）；回落到 `ServerSpec::default` 与校验都由 provider 调 `normalize_server` 完成。
- ⚠️ `send` 里不得 `unwrap` / `expect` / 裸索引：release 是 `panic = "abort"`，一次 panic 带走整个 App。失败一律 `Err(String)`。
- 超时与响应体上限沿用 flow 的量级（`PUSH_TIMEOUT_MS = 10s`、`HttpFetch` 那侧封顶），provider 不得自己起线程。
- provider **不读配置、不查库**：它只接 `SendRequest`。拼主题列表、判开关、排队、退避全在 `push/mod.rs` 的调度层 —— 这样 provider 可纯单测（喂假 request 断言 URL / 请求体）。

## 5. 加一个 provider

| # | 位置 | 改什么 |
|---|---|---|
| 1 | `push/providers/<id>.rs` | 实现 `PushProvider`（`server()` + `send()`）+ 单测 |
| 2 | `push/providers/mod.rs` | `mod <id>;` 一行 |
| 3 | `push/mod.rs` 的 `build_providers()` | 注册一行（顺序 = 前端服务列表顺序） |
| 4 | `src/modules/agent-status/lib/push-providers.ts` | 展示名 / 一句说明 / 主题量词一行；**地址与字段都不在这里抄** |

第 4 步配一条 `push-providers.spec.ts`：像 `provider-registry.spec.ts` 那样用 `?raw` 读 Rust 源，解析出 provider id 与 `ServerSpec::default`，断言与前端注册表逐个对应 —— 漏登记时 CI 红，而不是页面上少个字段。该文件还断言 provider 源码里**不出现** `FieldSpec` / `secrets` / `Bearer`（见 §6）。

✅ 新增 provider **不需要**改：分区组件、面板、store、命令、capability、配置 schema。

## 6. 主题不是密钥

用户明确要求：主题串要能在列表里直接看见、能命名，因此**整套凭据路径已删除**（`set_push_secret` / `delete_push_secret` 命令、`SecretView`、按 spec 推导密钥清单、级联清凭据、`Inner.notice`）。

- 主题串（含 Bark 的设备密钥）明文写在 `module_settings['agent-status'].push.channels[].topic`，也明文显示在面板输入框里。
- `flow` 模块**仍然**用自己的 `SecretSource` / `KeyringSecrets`（流程脚本的 `secrets.get`），两边互不影响；别把 push 的删除当成「flow 也不需要密钥」。
- ⚠️ 因此 **ntfy 开了访问控制的自建服务器不再支持**（没有 `Authorization` 头可发）。这一条写进 `server()` 的 help 文案，不要靠用户踩出来。

## 7. 出网纪律：退避与零出网

- **连续失败指数退避**（`BACKOFF_BASE_SEC × 2^n`，封顶 600s），与流程脚本同一口径；否则一个失效的 Bark 密钥会每秒打网。退避按**主题行**记账（`FailState`，键 = channel id），只影响失败的那一行，同服务下别的行照发。`configure()` 按存活行裁剪这本账。
  节流（`throttle_sec`）与静默时段曾压在这条链上，按「精简」要求删掉了 —— 现在的口径是「灯真的变了才发一条」，去重靠内核的 `changed` 判定（见 §3 的触发源）。
- ⚠️ **所有 agent 全停 = 零出网**：调度只看存活会话（`tick` 里按 `StatusSnapshot.sessions` 找得到 key 才入队），空闲时不产生任何 HTTP 请求。这条不变量与 flow 模块共享，破坏它的改动不予接受。
- 出站只走**一条**通道（`flow::HttpFetch`：统一超时、响应体上限、错误脱敏）。
  ⚠️ 地址策略与 flow **刻意不同**：flow 拦 loopback / 私网，因为那里发的是用户自写脚本拼出的任意地址；push 的地址是用户在设置页显式填的固定字段，而自建 ntfy 常落在局域网（`http://192.168.x.x:8080`），挡掉等于砍功能。因此 `providers::normalize_server` 只校 `http(s)://` scheme、形状与长度，**允许私网**；明文 `http://` 由字段 help 提示风险。
- 发送跑在**专用 worker 线程 + 串行队列**：一次网络抖动不能拖住 500ms 的状态循环，也不能让两个主题并发写同一份状态。

## 8. 统一测试发送

- 「发一条测试」按钮在**主题卡片头的右上角**，一次点完给**当前服务下的每个主题各发一条**（串行、不进队列、不写退避、不看服务与总开关 —— 测试要回答的是「这条地址发得出去吗」，与当前是否启用无关）。
- 命令 `test_push_provider(provider_id) -> Vec<PushTestOutcome>`，`PushTestOutcome { name, ok, status, error }`：面板把逐主题的成败拼成一行行内提示（`家里电脑 ✓ · 公司群 ✗ ntfy 返回 404：…`）。**一条坏地址不该把其它行的结果一起吞掉**，所以不返回「第一个错误就失败」。
- 该服务一行主题都没有时返回 `Err("该服务还没有主题，先加一行")`，按钮本身也在 `activeTopics` 为空时禁用。
- ⚠️ 必须是 `async` 命令 + `spawn_blocking`：几条请求串起来最坏 10s × N，非 async 命令跑在主线程会冻结主窗事件循环。
- **投递记录（「最近发送」列表）已删除**：两次测试之间投递失败不留痕，只有下一次点测试时才可见。这是刻意取舍 —— 有统一测试这个诊断入口，就不值得为一张只活在内存里、重启即清的环形表养一套 `DeliveryRecord`。

## 9. 前端分区

- 注册：`settings.sections` 追加 `{ id: 'push', label: '消息推送', component: PushSection, fill: true }`（放在「会话记录」之后）。`fill: true` 是必须的 —— 面板要占满内容区高度、左右两列各自滚，因此 `.main-window--wide → .module-page--fill → .module-page__section--fill → .push-panel` 这条高度链一环都不能缺。
- ⚠️ 分区**不再重复渲染左上角标题与说明**：二级 tabbar 已经写明这是哪一区，标题只是把面板往上顶。分区根节点用 `aria-label` 保住可读的名字（`ExtensionsSection` / `RecordsSection` / `PushSection` 三处同此）。
- **版面比例**：`.push-panel` 两行 = `auto`（总开关 + 两组勾选，按内容高）+ `minmax(0, 1fr)`（双栏）。上半区不再固定占 40% —— 节流 / 静默 / 前缀三项控件删掉后它撑不满那一档，写死会在中间留一大片空白。
- ⚠️ **卡片撑满列宽要靠选择器权重，不能靠加载顺序**：`.ufo-card` 自带 `max-w-md`，而模块 CSS 打包时排在 `main.css` 之前，所以放开限宽必须写成两个类权重（`.push-panel > .push-master` / `.push-split > .ufo-card`，同 `.ufo-card.light-sound`）。写成单类 `.push-master` 会被基类压回去 —— 实测就是「下半区两栏正常、上半区缩成一条 448px」。护栏：`agent-status-css.spec.ts` 扫全部 `max-w-none` 覆盖的选择器权重。
- **主题行的形状是硬约定**：`grid-template-columns: minmax(0, 1fr) minmax(0, 1.4fr) auto` = 名字 | 主题串 | 移除，且 `.push-topic__value` 用等宽字体（用户要求「左边名字、右边主题值」，顺序反了就错；等宽才看得清粘进的空格）。行本身**不是 `<button>`** —— 里面两个输入框加一个删除按钮，交互控件不能互相嵌套（同服务列表行的写法）。
- 移除复用「信息卡拓展」那套两步确认样式（`.ep-list__remove` / `--armed`，第一次点变「确认」），不另写一份。
- 文件归属：分区 `sections/PushSection.vue`、面板 `panels/PushChannelPanel.vue`、纯逻辑 `lib/push.ts` + `lib/push-providers.ts`、store `stores/pushStatus.ts`、样式 `agent-status.css`（模块自己的类一律 `.push-*`）。
- 配置读写一律走外壳 store 的 `moduleSettings('agent-status')` / `updateModuleSettings('agent-status', patch)`；**改一行主题要整块 patch**（浅合并语义），不要指望局部深合并。每次写后 `refresh()` 拉权威快照（后端做过截断与回落）。
- ⚠️ 反馈**不能用 `window.alert` / `confirm`**（装了 `tauri-plugin-prevent-default` 会被拦）：一律 `role="status"` 行内提示；删除走两步确认。
- 写命令（`test_push_provider`）只在主窗授权；`get_push_status` 同样只给主窗（面板是唯一读者，悬浮窗与提示窗都不订阅推送广播）。

## 10. 实现落点

| 层 | 位置 | 说明 |
|---|---|---|
| 调度 | `src-tauri/src/modules/agent_status/push/mod.rs` | 两层开关、每主题一条、退避、串行 worker、广播、2 个命令 |
| 配置 | `push/config.rs` | `push` 子块解析 + 夹取 + 旧 `fields` 回落；`backoff_seconds` 是纯函数 |
| 文案 | `push/message.rs` | `PushEvent`（内核语义 → 事件名，`title()` 出 `UFO · 一轮跑完`）与 `PushMessage`（只有标题 + 正文） |
| 契约 | `push/providers/mod.rs` | `PushProvider` trait、`ServerSpec`、`SendRequest`、`normalize_server`、`encode_segment` |
| 服务 | `push/providers/{ntfy,bark}.rs` | 各一份 `server()` 与 `send()` |
| 触发 | `status/mod.rs::apply_status_event` | **灯真的变了**才 `push.note_change(...)`；每轮 `push.tick(&snapshot, &flow, now)` |
| 前端 | `lib/push.ts` · `lib/push-providers.ts` · `stores/pushStatus.ts` · `sections/PushSection.vue` · `panels/PushChannelPanel.vue` | 类型与校验 / 展示元信息 / 面板数据 / 分区与面板 |

命令只剩两个：`get_push_status`（读快照）与 `test_push_provider`（统一测试），都在 `permissions/status.toml` 的 `allow-manage-push` 里、只授予 `main` 窗口。

## 11. 红线与验收

**红线**

| 做法 | 后果 |
|---|---|
| 在推送层自己判定灯态 / 读 inbox 事件 | ❌ 会与悬浮窗的灯不一致。只消费内核投影出的语义 |
| 把 `prompt` 默认塞进推送正文 | ❌ 公开主题长期留存用户私有内容。必须显式开关 + 风险提示 |
| 给主题串加脱敏 / 挪回凭据管理器 | ❌ 用户明确要的是「看得见、能命名」（§6）；要改回去先问 |
| 为 Bark 特判「一台设备」 | ❌ 用主题行表达，服务差异不进控制流 |
| 加 provider 时顺手改配置块结构 | ❌ 一律 optional + 默认值，不升版本号 |
| 推送失败时 `panic!` / `expect` | ❌ release 是 `panic = "abort"`，会带走整个 App |

**自动化**

- [ ] `cargo test`：provider 单测（喂 `SendRequest` 断言 URL / 请求体 / 头）、两层开关、每主题一条、退避序列、`test_provider` 逐主题回报、旧 `fields` 回落
- [ ] `npx vitest run`：`push-providers.spec.ts` 与 Rust 同源断言（含「不出现 `FieldSpec` / `secrets` / `Bearer`」）、配置归一化（缺字段 / 坏字段 / 越界截断 / 旧键回落）、`pushPatch` 整块替换不留旧键、面板 → store 契约
- [ ] `npm run build`（`vue-tsc --noEmit` + vite）过
- [ ] 落地检查：`rg -n "bark|ntfy" src/modules/agent-status/sections src/modules/agent-status/panels` 只应命中注册表与文案，不应命中控制流分支
- [ ] 已删功能不得复活：`rg -niE "throttle|quiet|title_prefix|FieldSpec|muted_pending|records" src-tauri/src/modules/agent_status/push src/modules/agent-status` 只应命中测试里那几句「断言它不存在」的话

**人工（真机）**

- [ ] 消息推送 tab：改服务器地址 → ＋添加两行具名主题 → 点右上角「发一条测试」→ 行内提示逐主题报成败
- [ ] 只留一行空主题：不发（provider 给出「主题为空」的原因），其它行照发
- [ ] 关掉服务开关后让 agent 跑一轮：该服务零出网；关掉总开关后两个服务都零出网
- [ ] 填一个失效的 Bark 密钥：连续失败后退避生效（不再每轮重试），面板不打转
- [ ] 重启应用：配置、主题名字与主题串都还在；旧键（`throttle_sec` / `quiet` / `title_prefix` / `channels[].enabled` / `fields` / `secret_names`）在第一次改动后消失

## 12. 已知代价与孤儿凭据

1. **需要访问令牌的自建 ntfy 不再支持**（`Authorization` 那条路随密钥一起删了）。
2. **主题串明文驻留配置**：`%APPDATA%/<identifier>` 下的模块配置 JSON 里能看到 Bark 的设备密钥。与「提问明文落 `session-records.db`」「提问明文落 inbox」同一条已知代价级别 —— 拿到这台机器的人即可拿到这些串。
3. **旧版本写进 Windows 凭据管理器的条目成了孤儿**：`com.tauri-app.my/push/<channel_id>` 那批值删不掉了（`keyring` 无法按 service 列举，而删除路径已随功能一起移除）。只在这里写明手工清理路径 —— **控制面板 → 凭据管理器 → Windows 凭据 → 删除 `com.tauri-app.my/push/…`**。曾考虑的「保留一次性清理命令」被否掉：为清几行凭据养着一整套密钥后端，代价大于收益。
4. 因此老配置里的 **Bark 行为空主题**：密钥取不回来，用户要在主题栏重新粘一次（面板的空态文案已写明这一栏填什么）。
