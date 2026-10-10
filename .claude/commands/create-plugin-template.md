---
description: 验证宿主的组件包能力面，然后刷新 reference/plugin-template（升模板版本 + 重写 CAPABILITIES.md）
allowed-tools: Bash(npx vitest run *), Bash(rg *), Bash(git diff *)
---

验证宿主的插件功能，并就地刷新作者侧模板 `$ARGUMENTS`。

> 规则与 `.agents/skills/create-plugin-template/SKILL.md` 是同一份，改流程要两边一起改。
> 这条命令由 agent 逐步执行，不落脚本：模板目录里的教学注释与排版是产物的一部分。

## 唯一事实来源（只读这四份）

- `src-tauri/src/plugin/paths.rs` —— `PLUGIN_API_VERSION` / `MANIFEST_VERSION` + 全部数字上限
- `src-tauri/src/plugin/capability.rs` —— `PERMISSIONS` / `SURFACES` 两张表（`id` `label` `tone` `disclosure`）
- `src/lib/plugin-host.ts` —— `PluginHost` 的命名空间与方法签名；`createPluginHost` 里各块的 `require('…')` 给出这一档要哪份权限
- `src/lib/plugin-loader.ts` —— 宿主从包的默认导出实际读哪些 `entry.x`

解析前先剥注释：这些文件的注释里满是 `host.http` / `entry.tooltip` 这种写法，拿它当证据就会假绿。

## 流程

1. **跑契约**（红了就停，不要带着错生成）：
   `npx vitest run` 上 `plugin-capabilities` / `plugin-limits` / `plugin-types` / `plugin-host` /
   `plugin-loader` / `plugin-acl` / `plugin-event-payload` / `plugin-template` / `plugin-template-doc` 这几档 spec。
2. **六条源码对账**（细节见 SKILL.md）：能力表 ↔ `host` 入口两个方向、命名空间实现齐全
   （`http: httpApi` 这种别名要跟着找到 `const httpApi` 再数 `require`）、模板清单声明的能力都被自己用到、
   入口字段集合与 `src/index.d.ts` 双向一致、`plugin-limits.ts` 的 API 版本与 `paths.rs` 相等。
   有不一致就只报告，要强行生成需用户点头。
3. **写两个文件**：
   - `reference/plugin-template/CAPABILITIES.md` 整份重写，八个固定小节标题逐字保留（那条 doc spec 按标题取段）。
   - `reference/plugin-template/public/plugin.json` 只改 `apiVersion` / `manifestVersion`（抄 `paths.rs`）与 `version`
     （能力面变了走 minor，否则走 patch）。排版保持原样，别用 `JSON.stringify` 重排。
4. **别动** `src/*.vue`、`src/index.js`、`src/host.js`、`src/styles.css`、`vite.config.mjs`、`README.md` ——
   它们被 `plugin-template.spec.ts` 逐条断言，重写等于把护栏盯着的东西洗掉。
5. **复验**：`npx vitest run src/lib/plugin-template-doc.spec.ts src/lib/plugin-template.spec.ts src/lib/plugin-template-e2e.spec.ts` 全绿再交。
6. **交付说明**里写清：`ble` / `serial` 没上过真设备；要不要升宿主的 `PLUGIN_API_VERSION` 是用户的决定；不 commit 不 push。

## 文档的硬性要求

不许出现宿主没有的名字，也不许用「等等 / 之类」省略 —— `plugin-template-doc.spec.ts` 是双向集合断言，
少写一项和多写一项都会红。披露原文照 `capability.rs` 一个字都不改；上限表按名字后缀换算可读单位
（`_BYTES` → KiB/MiB/GiB、`_MS` → `N ms`、`_CHARS` → `N 字符`，其余是个数）。
