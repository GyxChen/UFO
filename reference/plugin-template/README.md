# UFO 组件包模板

改三个地方就能得到一个能装的包：`public/plugin.json` 的 `id` / `name`、`src/*.vue` 的界面、
`src/index.js` 的导出。宿主侧的完整约束与能力边界写在
`reference/doc/guide/plugin-guide.md`（这份只讲怎么跑起来）。

**这一版宿主到底提供什么**（清单 `permissions` 能写哪些名字、每个名字对应 `host` 上哪些方法、
入口要导出哪些字段、上限是多少）全在 `CAPABILITIES.md` —— 那份是 `/create-plugin-template`
从宿主源码整理出来的，别手改，宿主能力变了它自己的 spec 会红。

## 跑起来

```bash
yarn install
yarn dev            # 监听构建：改一次 .vue 就重出 dist/my-plugin/
```

然后打开 UFO：齿轮 → 下面的插件管理区 → 选目录 → 选本仓库的 `reference/plugin-template/dist/my-plugin`
（**不是** `plugin-template` 本身，也不是 `dist`）→ 确认安装。

装完把详情卡里的「热重载」开关打开：之后只要 `yarn dev` 重出产物，悬浮窗 / 信息卡 / 设置页
里的这一块就会自己换掉，不用重启、也不用再点安装。

## 目录

```
public/plugin.json      清单（构建时整份拷进产物，宿主读的就是它）
CAPABILITIES.md         这版宿主的能力与 API（生成物，见上面那段）
src/index.js            入口：默认导出四个挂载点的组件 + activate(host)，样式也在这里挂
src/Cell.vue            悬浮窗格子里的那一块
src/TooltipCard.vue     悬停自己格子时的独立信息卡
src/ExtensionBlock.vue  插进「状态灯」信息卡尾部的一块
src/SettingsPanel.vue   主窗一级页签里自己的设置页
src/styles.css          全部样式（由 JS 以 ?inline 带进产物 —— 看下面第 2 条）
src/host.js             useHost()：在 setup 里取这个包的宿主能力面
src/state.js            四个挂载点共享的一份 reactive 状态
vite.config.mjs         单文件产出 + 把 vue/pinia 换成宿主注入的那一份 + 拷清单
```

## 四条会让「跑不起来」的错法

1. **把 Vue 打进包里**。`vite.config.mjs` 里的 `ufo-host-globals` 插件把 `from 'vue'` /
   `from 'pinia'` 换成「读 `globalThis.__UFO_VUE__`」的生成模块，所以它必须 `enforce: 'pre'` —
   晚一步就被 vite 的核心解析器抢先，把 `node_modules` 里的 Vue 打进来。两个 Vue 实例的坏法是
   `provide/inject` 与响应式静默错乱：界面照常渲染，数据不通，而构建与类型检查全绿。
   自检一句话：产物里要能 `grep` 到 `__UFO_VUE__`，且不能出现 `@vue/shared`。
2. **把样式写在 `.vue` 的 `<style>` 里**。清单只有一个 `entry`（一个 JS 产物），
   构建额外产出的 `.css` 没人去取，表现是「组件挂上了但完全没样式」。样式集中在
   `src/styles.css`，由入口以 `import styles from './styles.css?inline'` 带进来，在**模块顶层**
   挂一个带 id 的 `<style>`（热重载只重新 import，不会重跑同一个包的 `activate()`，
   挂在 `activate()` 里会表现为「类名对了、样子没变」）。
3. **在模块顶层 `inject`**。顶层只有 `activate(host)` 拿到的那一份是准的；
   组件里要 host 就在 `<script setup>` 里 `useHost()`。
4. **写死格子尺寸**。槽位高度随贴边形态变（方形未贴边时一格可能只有十几像素高），
   只用 `100%` 和 `cqw` / `cqh`。

## 只留一个挂载点

清单里删掉不要的那一句，`src/index.js` 里删掉对应的导出，`vite.config.mjs` 不用改。
清单没声明的挂载点，宿主不会去取，多导出也不会出现。
