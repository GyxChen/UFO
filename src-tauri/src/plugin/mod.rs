//! 插件（组件包）在 Rust 侧的家：与前端 `src/lib/plugin-*.ts` 一一对应。
//!
//! 分工：
//! - [`paths`]：包目录 / id / 包内路径的规则，以及所有体积上限（唯一落点）。
//! - [`manifest`]：`plugin.json` 的形状与校验（结构性错误拒装、数字夹取 + 告警）。
//! - [`capability`]：宿主能力与挂载点的注册表（披露文案、徽标图标、配色档）。
//! - [`store`] / [`commands`]：台账与 IPC（能力闸在这里，按 pluginId 查清单）。
//! - [`protocol`]：`ufo-plugin://` 产物供给（含 `__pick` / `__fs` 两条保留入口）。
//! - [`log`] / [`events`] / [`storage`] / [`net`] / [`secrets`] / [`fs`]：六项宿主能力的实装。
//!
//! ⚠️ 这个模块**不认识任何具体插件**，也不该出现某个包的 id；
//! 具体包的信息只从台账与清单读（护栏见前端 `plugin-shell-purity.spec.ts`）。

pub mod capability;
pub mod commands;
pub mod events;
pub mod ble;
pub mod fs;
pub mod handle;
pub mod log;
pub mod manifest;
pub mod net;
pub mod paths;
pub mod protocol;
pub mod secrets;
pub mod serial;
pub mod storage;
pub mod store;
