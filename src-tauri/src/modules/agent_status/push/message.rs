//! 推送的通用文案模型：provider 只看到「一条带事件语义的标题 + 正文」，看不到灯态实现。
//!
//! 事件取值直接由内核的 `AgentEventKind` 投影（见 [`PushEvent::from_kind`]），
//! ⚠️ 推送层**不得自己判定灯态** —— 否则手机上收到的会和悬浮窗上的灯不一致。

use serde::Serialize;

use crate::modules::agent_status::status::AgentEventKind;

/// 可推送的四种事件语义（线协议字符串：存配置 `events[]`、进广播）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PushEvent {
    /// 一轮跑完、等你输入。
    TurnFinished,
    /// 在等你授权 / 回答。
    WaitingForUser,
    /// 本轮出错。
    Failed,
    /// 会话结束。
    SessionFinished,
}

/// 通知标题前缀：**写死**而不是可配 —— 多设备 / 多人共用一个主题时，靠「提醒内容」里的
/// endpoint 与会话行去区分来源，不需要再给用户提供一个会进每条通知的自由文本框。
pub const PUSH_TITLE_PREFIX: &str = "UFO";

impl PushEvent {
    /// 内核语义 → 推送事件；不推送的语义（Running / 子代理 / Unknown）返回 None。
    ///
    /// ⚠️ 子代理事件**永不**单独推送：它与主会话共用 session_id，推出来只会是重复噪音。
    pub fn from_kind(kind: AgentEventKind) -> Option<Self> {
        match kind {
            AgentEventKind::TurnFinished => Some(Self::TurnFinished),
            AgentEventKind::WaitingForUser => Some(Self::WaitingForUser),
            AgentEventKind::Failed => Some(Self::Failed),
            AgentEventKind::SessionFinished => Some(Self::SessionFinished),
            _ => None,
        }
    }

    /// 解析配置里的一个事件名；认不出返回 None（坏字段只丢这一项，不影响其它）。
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "turn_finished" => Some(Self::TurnFinished),
            "waiting_for_user" => Some(Self::WaitingForUser),
            "failed" => Some(Self::Failed),
            "session_finished" => Some(Self::SessionFinished),
            _ => None,
        }
    }

    /// 标题里那句人话。
    pub fn label(self) -> &'static str {
        match self {
            Self::TurnFinished => "一轮跑完",
            Self::WaitingForUser => "在等你",
            Self::Failed => "本轮出错",
            Self::SessionFinished => "会话结束",
        }
    }

    /// 通知标题：`UFO · 一轮跑完` 这样的一句（两个 provider 与测试消息都用它）。
    pub fn title(self) -> String {
        format!("{PUSH_TITLE_PREFIX} · {}", self.label())
    }
}

/// 提醒内容里**可选**的四项（线协议字符串：存配置 `content[]`、进广播）。
///
/// ⚠️ agent 状态（哪个平台的哪类事件）是**固定发**的，不在这一列里 —— 没有可勾的开关，
/// 因为一条不含状态的推送没有意义。这四项都是「额外带什么」，默认全不勾（少外泄一寸是一寸）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PushContent {
    /// 会话连的 endpoint（`upstream_url ?? base_url`，与 tooltip 同一口径）。
    Endpoint,
    /// 用户最近一次提问原文。
    Prompt,
    /// 流程脚本产出的拓展字段（信息卡上那几行）。
    Script,
    /// 会话标识（`provider:session_id` 全量）。
    SessionId,
}

impl PushContent {
    /// 解析配置里的一项；认不出返回 None（坏值只丢这一项）。
    ///
    /// 反向的「写成字符串」不需要手写：serde 的 `rename_all = "snake_case"` 就是线协议写法。
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "endpoint" => Some(Self::Endpoint),
            "prompt" => Some(Self::Prompt),
            "script" => Some(Self::Script),
            "session_id" => Some(Self::SessionId),
            _ => None,
        }
    }

    /// 正文里这一行的标签。
    pub fn label(self) -> &'static str {
        match self {
            Self::Endpoint => "endpoint",
            Self::Prompt => "提问",
            Self::Script => "脚本",
            Self::SessionId => "会话",
        }
    }
}

/// 一条待投递的消息（provider 无关）。
#[derive(Debug, Clone)]
pub struct PushMessage {
    /// 标题（已带用户配置的前缀 + 事件名）。
    pub title: String,
    /// 正文。⚠️ 默认不含用户提问原文 —— ntfy 主题公开可订阅，提问可能含密钥（见规范 §3）。
    pub body: String,
}
