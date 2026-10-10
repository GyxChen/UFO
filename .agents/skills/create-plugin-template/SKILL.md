---
name: create-plugin-template
description: 验证宿主的组件包（插件）能力面，然后就地刷新 reference/plugin-template —— 升模板版本、重写 CAPABILITIES.md 里的可用能力与每项能力下的 API。用户说「/create-plugin-template」「刷新插件模板」「把模板跟宿主对齐」「模板是不是漂了」时使用。
---

# create-plugin-template

把「模板与宿主契约漂移」这件事收在这一条命令里。**这条命令不是脚本**：每一步都是 agent 读代码、核对、写文件。
产物是一份给人抄的模板目录，脚本式生成会把里面那些教学注释洗掉。

## 什么时候用

- 宿主加了或改了组件包的能力：新命名空间、新方法、新权限名、新挂载点、入口多读了一个字段。
- `npx vitest run src/lib/plugin-template-doc.spec.ts` 红了 —— 那条 spec 就是「这份文档该重生成」的旗标。
- 作者反馈「照着模板写却装不上 / 装上后某一面是空的」。

## 唯一事实来源（只读这四份，不抄文档）

| 文件 | 提供什么 |
| --- | --- |
| `src-tauri/src/plugin/paths.rs` | `PLUGIN_API_VERSION`、`MANIFEST_VERSION`，以及全部数字上限 |
| `src-tauri/src/plugin/capability.rs` | `PERMISSIONS` / `SURFACES` 两张表：`id` `label` `tone` `disclosure` |
| `src/lib/plugin-host.ts` | `PluginHost` 接口的命名空间与方法签名；`createPluginHost` 里每个块的 `require('…')` 给出「这一档要哪份权限」 |
| `src/lib/plugin-loader.ts` | 宿主从包的默认导出实际读哪些 `entry.x`，以及缺了哪一项会被记成降级 |

读的时候**先剥注释**：这几份文件的文档注释里满是「`host.http`」「`entry.tooltip`」这种写法，
拿它当契约证据就会假绿（同一个坑见 `src/lib/plugin-template.spec.ts` 的 `codeOf`）。

## 流程

### 第 1 步：验证宿主的插件功能

先跑契约那几档，红了就停 —— 契约自己没对齐的时候生成，只会把错固化成一份看起来很权威的文档：

```bash
npx vitest run src/lib/plugin-capabilities.spec.ts src/lib/plugin-limits.spec.ts \
  src/lib/plugin-types.spec.ts src/lib/plugin-host.spec.ts src/lib/plugin-loader.spec.ts \
  src/lib/plugin-acl.spec.ts src/lib/plugin-event-payload.spec.ts \
  src/lib/plugin-template.spec.ts src/lib/plugin-template-doc.spec.ts
```

再逐条源码对账，六条都是「不会有任何编译错误」的那种漂移：

1. `capability.rs` 的每一项能力，都能在 `plugin-host.ts` 里找到一个块 `require` 它。找不到 → 「登记了但宿主没实现」，作者清单写了也用不了。
2. 反向：`host` 里 `require` 的名字必须在表里登记过。没登记 → 作者把它写进清单会被直接拒装。
3. `PluginHost` 声明的每个命名空间，在 `createPluginHost` 的字面量里都有对应块。⚠️ 别名写法 `http: httpApi` 要跟着去找 `const httpApi` 那一段再数 `require`，否则会误报成缺实现。
4. 模板 `public/plugin.json` 的 `permissions` ⊆ 登记表，且每一项都在模板 `src/` 里真被引用（与 `plugin-template.spec.ts` 的「声明了就得用到」同方向）。
5. 宿主读的 `entry` 字段集合 == 模板 `src/index.d.ts` 里 `declare const entry` 那一段的字段集合。两个方向都要查：宿主多读了，作者侧编辑器没提示；`index.d.ts` 多写了，那份声明是假的。
6. 顺带确认 `plugin-limits.ts` 的 `PLUGIN_API_VERSION` 与 `paths.rs` 相等（`plugin-limits.spec.ts` 本来就在盯）。

任何一条不一致：**只报告，不生成**。要继续得用户点头，并且把不一致的条目原样留在交付说明里。

### 第 2 步：生成 / 刷新模板

**动这两个文件，别的不动：**

- `reference/plugin-template/CAPABILITIES.md` —— 整份重写，小节标题必须逐字照下面的「输出格式」。
- `reference/plugin-template/public/plugin.json` —— **只改三个值**：`apiVersion`、`manifestVersion` 抄 `paths.rs` 的当前数，`version` 递增。
  排版保持原样：`"tooltip": { "own": true, "size": [320, 160] }` 挤在一行是这份清单的读法，别用 `JSON.stringify` 重排。

**不许动的文件**：`src/*.vue`、`src/index.js`、`src/host.js`、`src/state.js`、`src/styles.css`、`vite.config.mjs`、`README.md`。
它们是被 `plugin-template.spec.ts` 逐条断言的机制件（`enforce: 'pre'`、`?inline`、`codeSplitting: false`、`fileName: 'main.js'`），
重写等于把护栏盯着的东西洗掉；这些文件要改是另一件事，改完跑那条 spec 才算数。

**版本递增规则**：能力名集合或 `host` 成员名集合变了 → minor（`1.0.1` → `1.1.0`）；只是文案、上限、重排 → patch。

**演示范围保持三项**：模板清单只声明 `storage` / `events:agent` / `fs`（无外设、不配密钥、不出网就能整包跑通）。
`http` / `secrets` / `frame` / `ble` / `serial` 不进清单，只在 `CAPABILITIES.md` 里给出接口原文 + 可直接抄的调用形状。
理由：抄模板的人会一路把用不到的权限带进真包，而每一份权限都是给用户看的一句披露。

### 第 3 步：复验

```bash
npx vitest run src/lib/plugin-template-doc.spec.ts src/lib/plugin-template.spec.ts src/lib/plugin-template-e2e.spec.ts
```

全绿再交。若怀疑某条断言是假的（尤其是新加的），做一次变异验证：改坏宿主一处（比如把某个方法改名），
应当**只有**对应那一条红，然后把宿主文件逐字节还原（`git diff` 对过再算还原）。

### 第 4 步：交付时要说清

- 文档里的签名是宿主接口原文，但 `ble` / `serial` 没上过真设备 —— 照文档写不等于跑得通。
- 要不要升宿主的 `PLUGIN_API_VERSION` 是用户的决定（它管的是「旧包还装不装得上」）。这条命令只把模板对齐到宿主当前值，不碰那个数。
- 不 commit、不 push，除非用户明说。

## 输出格式（`CAPABILITIES.md`）

`src/lib/plugin-template-doc.spec.ts` 是按标题取段的，改了标题护栏就瞎了 —— 下列标题逐字保留。

1. `# 这份宿主当前提供什么` —— 快照行：`API **vN**`、`清单格式 **vN**`、能力数、命名空间数、入口字段数。
   紧跟一段「每一节的判据来自哪份文件」，让读者知道这不是散文。
2. `## 清单 \`permissions\` 能写哪些名字` —— 四列表：`名字 | host 上的入口 | 档 | 本模板演示了`。
   第一列必须是反引号包住的能力名；第四列只写 `是` / `否`（那条 spec 拿它与清单的 `permissions` 对撞）。
3. `## 各能力的 API` —— 每档一节，标题 `### \`host.<命名空间>\` —— 需要 \`<权限名>\``；节内先一段 ```js 调用形状，
   再逐成员一行 `- \`签名原文\` —— 源码注释的第一句`。
4. `## 不占权限位的入口` —— `host.plugin`（这个包自己的身份，只读）、`host.tooltip.setSize`
   （授权来自清单的 `surfaces.tooltip.own`，所以不出徽标）、`host.log`（需求点名的特例，不声明也能打）。
5. `## 装前给用户看的披露原文` —— 每条 `- **\`id\`** label：disclosure`，原文照 `capability.rs`，一个字都不许改写。
6. `## 入口：宿主会从你的默认导出里读这些名字` —— 两列表：`字段 | 缺了会怎样`。
7. `## 挂载点` —— 两列表：`登记名 | label —— disclosure`。
8. `## 上限（宿主会夹的数）` —— `paths.rs` 里全部数字常量，两个版本常量不在这里重复列。
   单位按名字后缀换可读写法：`_BYTES` → KiB/MiB/GiB，`_MS` → `N ms`，`_CHARS` → `N 字符`，其余是「个数」，别把 1024 个键印成 `1 KiB`。

硬性要求：**不许出现宿主没有的名字**，也**不许用「等等 / 之类 / 类似」省略** —— 那条 spec 是双向集合断言，
少写一项和多写一项都会红。

## 不受这条命令管的

- `reference/doc/guide/plugin-guide.md` §4 那张表是人写的散文，里面有取舍理由（「为什么不为组件建窗」），
  命令不重写它。能力变了要顺手改那张表的对应行，这是人力活。
- `plugin.json` 的 `surfaces` 键怎么映射到入口字段，判据在 `src/lib/plugin-loader.ts`，文档只列字段名，不复制那份规则。
- 模板的 `dist/` 产物（gitignore）与作者自己的包：命令不构建、不安装，验证靠那几档 spec 与真机。
