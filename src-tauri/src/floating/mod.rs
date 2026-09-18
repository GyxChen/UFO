//! 悬浮窗外壳（Rust 侧）：窗口/组件的配置存取与转发。
//!
//! 外壳**不认识任何具体组件**：它只按 id 存一块 JSON（`module_settings`），
//! 组件自己的字段由组件自己解释（见 [`crate::modules`]）。

pub mod module_config;
