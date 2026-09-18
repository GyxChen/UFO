//! Agent 状态灯组件（Rust 侧）。
//!
//! 与前端 `src/modules/agent-status/` 一一对应：一个 CLI 会话一盏灯，
//! 数据链路 = 平台 hook / Codex rollout 采集 → 兼容层归一化 → 内核维护灯态，
//! 另外带上信息卡拓展（流程脚本）与跨重启的会话存档。
//!
//! 组件自己的配置（保留时长 / 执行超时 / token 单位 / 代理解析器）由 [`settings`]
//! 解释 —— 外壳只存一块 JSON，不认字段名。

pub mod endpoint_profiles;
pub mod flow;
pub mod proxy;
pub mod session_store;
pub mod settings;
pub mod status;
