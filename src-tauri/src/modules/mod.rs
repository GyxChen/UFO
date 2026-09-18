//! 悬浮窗组件在 Rust 侧的家。
//!
//! 一个组件一个目录，目录名 = 前端注册表里模块 id 的 Rust 写法
//! （`agent-status` → `agent_status`）：组件自己的状态、命令、设置、采集逻辑
//! 全部收在它自己的目录里，外壳（[`crate::floating`]）不认识任何组件。
//!
//! 加一个新组件 = 在这里加一个 `pub mod <id>;`，并按需在组合根（`lib.rs`）
//! 登记它的配置钩子与命令 —— 与前端「注册表里加一条」对称。

pub mod agent_status;
pub mod weather;
