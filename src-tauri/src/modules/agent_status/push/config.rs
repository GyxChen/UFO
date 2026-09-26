//! 推送配置块的解析与判定纯函数。
//!
//! 这块 JSON 存在 `module_settings['agent-status'].push` 里，外壳只按 id 整块透传、不解释任何字段；
//! 本文件是它的唯一解释者（前端对应实现见 `src/modules/agent-status/lib/push.ts`，两边口径必须一致）。
//!
//! 与 `../settings.rs` 同一条纪律：**逐字段宽松取值 + 夹取**，不用 serde 整块反序列化 ——
//! 手改配置写坏一个值只该回落那一个字段，不该让整块设置消失。
//! ⚠️ 新增字段一律 optional + 默认值，**不升任何 schema 版本号**（旧文件要能被新程序原样读出）。
//!
//! ## 形状为什么这么小
//! 一个服务只有一个可配字段（服务器地址），一个收件方只有「名字 + 主题」两列 ——
//! 没有静默时段、没有节流、没有标题前缀、没有投递记录，也**没有密钥**：
//! 主题串按用户要求明文摆在配置里（已知代价见 `reference/doc/agent-status-push-guide.md` §12）。

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use super::message::{PushContent, PushEvent};

/// 总开关默认关：推送会往公网发内容，默认开着等于替用户做了他没同意的决定。
const DEFAULT_ENABLED: bool = false;
/// 提醒内容里每项的文本上限：提问可能很长，推送正文要收得住。
pub const MAX_CONTENT_CHARS: usize = 300;
/// 主题串（ntfy topic / Bark 设备密钥）长度上限，防手改配置塞进超长串。
pub const MAX_TOPIC_CHARS: usize = 256;
/// 服务器地址长度上限，与 `providers::normalize_server` 同一档。
pub const MAX_SERVER_CHARS: usize = 4096;
/// 收件方名字上限（进列表与通知标题，太长会把行挤没）。
pub const MAX_NAME_CHARS: usize = 48;
/// 最多配多少个主题（防配置文件被手改出上千条，每轮都要遍历一遍）。
const MAX_CHANNELS: usize = 32;
/// id / provider 的形状上限。
const MAX_ID_CHARS: usize = 64;
const MAX_PROVIDER_CHARS: usize = 32;
/// 退避封顶（秒）：与流程脚本同一档，挂了也别每秒打网。
pub const MAX_BACKOFF_SEC: u64 = 600;

/// 一个服务级配置：只有开关与服务器地址（空串 = 用 provider 自带的官方默认地址）。
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderConfig {
    /// 该服务是否启用（服务关掉时它下面所有主题一条都不发）。
    pub enabled: bool,
    /// 服务器地址原样值。
    pub server: String,
}

/// 一个主题 = 一个收件地址（ntfy 的主题 / Bark 的设备密钥）。
///
/// ⚠️ 主题**不是密钥**：它随配置明文落盘，这是本次精简的已知代价，不要在这里再加脱敏逻辑。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChannelConfig {
    /// 稳定随机 id：只做行标识与增删定位（凭据 service 名已随密钥一起删除）。
    pub id: String,
    /// 所属 provider id。
    pub provider: String,
    /// 用户起的名字，显示在列表左侧。
    pub name: String,
    /// 主题串，显示在列表右侧。
    pub topic: String,
}

/// 整份推送配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PushConfig {
    /// 总开关。
    pub enabled: bool,
    /// 勾选的事件（线协议字符串，解析时已过滤非法值）。
    pub events: Vec<PushEvent>,
    /// 正文额外带哪些内容（agent 状态固定发，这里只列「还要带什么」）。
    ///
    /// 默认**空**：endpoint / 提问 / 脚本 / 会话 id 都可能敏感，而 ntfy 主题是公开可订阅的。
    pub content: Vec<PushContent>,
    /// 各服务的配置，key = provider id。
    pub providers: BTreeMap<String, ProviderConfig>,
    /// 主题清单（顺序即 UI 顺序）。
    pub channels: Vec<ChannelConfig>,
}

impl Default for PushConfig {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_ENABLED,
            // 默认勾前三类「该看手机」的，会话结束不勾（信息量低、噪音高）。
            events: vec![
                PushEvent::TurnFinished,
                PushEvent::WaitingForUser,
                PushEvent::Failed,
            ],
            content: Vec::new(),
            providers: BTreeMap::new(),
            channels: Vec::new(),
        }
    }
}

impl PushConfig {
    /// 从 `module_settings['agent-status']` 里取 `push` 子块并解析（缺块 = 全默认）。
    pub fn from_block(module_block: Option<&Value>) -> Self {
        let Some(raw) = module_block.and_then(|block| block.get("push")) else {
            return Self::default();
        };
        let defaults = Self::default();
        Self {
            enabled: read_bool(raw.get("enabled"), defaults.enabled),
            // 事件清单：**字段存在就照它走**（包括空数组 —— 那是用户把所有勾都去掉的明确意图）；
            // 只有字段缺失或类型不对才回落默认勾选。
            events: raw
                .get("events")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .filter_map(PushEvent::parse)
                        .collect()
                })
                .unwrap_or(defaults.events),
            // 提醒内容与事件同理：字段在就权威（全不勾 = 只发固定那一行）。
            content: raw
                .get("content")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .filter_map(PushContent::parse)
                        .collect()
                })
                .unwrap_or(defaults.content),
            providers: read_providers(raw.get("providers")),
            channels: read_channels(raw.get("channels")),
        }
    }

    /// 该事件是否要推（两层开关的第一层：总开关 ∧ 事件勾选）。
    pub fn wants(&self, event: PushEvent) -> bool {
        self.enabled && self.events.contains(&event)
    }
}

/// 解析服务表：只认对象形态，单个服务坏了不影响别的服务。
fn read_providers(raw: Option<&Value>) -> BTreeMap<String, ProviderConfig> {
    let mut out = BTreeMap::new();
    let Some(map) = raw.and_then(Value::as_object) else {
        return out;
    };
    for (id, value) in map {
        out.insert(
            id.clone(),
            ProviderConfig {
                enabled: read_bool(value.get("enabled"), false),
                // 旧配置把地址存在 `fields.server` 里（那时服务字段是通用 map）。
                server: read_string(value.get("server"))
                    .or_else(|| read_legacy(value, "server"))
                    .map(|server| server.chars().take(MAX_SERVER_CHARS).collect())
                    .unwrap_or_default(),
            },
        );
    }
    out
}

/// 解析主题清单：跳过没有 id / 没有 provider 的行（那种行连该发给谁都不知道）。
fn read_channels(raw: Option<&Value>) -> Vec<ChannelConfig> {
    let Some(items) = raw.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        let Some(id) = read_string(item.get("id")).filter(|id| id.len() <= MAX_ID_CHARS) else {
            continue;
        };
        let Some(provider) = read_string(item.get("provider"))
            .filter(|provider| provider.len() <= MAX_PROVIDER_CHARS)
        else {
            continue;
        };
        let name = read_string(item.get("name"))
            .map(|name| name.chars().take(MAX_NAME_CHARS).collect())
            .unwrap_or_else(|| provider.clone());
        out.push(ChannelConfig {
            topic: read_string(item.get("topic"))
                .or_else(|| read_legacy(item, "topic"))
                .map(|topic| topic.chars().take(MAX_TOPIC_CHARS).collect())
                .unwrap_or_default(),
            id,
            provider,
            name,
        });
        if out.len() >= MAX_CHANNELS {
            break;
        }
    }
    out
}

/// 读旧版存在 `fields` 里的一个值（`{ server, topic }` 当年都是通用字段）。
fn read_legacy(value: &Value, key: &str) -> Option<String> {
    read_string(value.get("fields")?.get(key))
}

/// 只认真正的布尔值；写错类型（如字符串 "false"）回落默认，不做隐式转换。
fn read_bool(value: Option<&Value>, fallback: bool) -> bool {
    value.and_then(Value::as_bool).unwrap_or(fallback)
}

/// 取一个非空字符串（自动 trim）。
fn read_string(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// 连续失败后的退避秒数：`base × 2^n`，封顶 [`MAX_BACKOFF_SEC`]。
///
/// 与流程脚本同一口径 —— 一个失效的设备密钥不该每秒重试一次。
pub fn backoff_seconds(base_sec: u64, fail_streak: u32) -> u64 {
    if fail_streak == 0 {
        return base_sec;
    }
    let factor = 1u64 << fail_streak.min(5);
    base_sec.saturating_mul(factor).min(MAX_BACKOFF_SEC)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn module(push: Value) -> Value {
        json!({ "push": push })
    }

    #[test]
    fn missing_block_is_all_defaults_and_disabled() {
        let config = PushConfig::from_block(None);
        assert!(!config.enabled, "默认关：不能替用户决定往公网发东西");
        assert_eq!(config.events.len(), 3);
        assert!(config.content.is_empty(), "提醒内容默认全不勾：出网内容要用户显式选");
        assert!(config.channels.is_empty());
        assert!(config.providers.is_empty());
    }

    #[test]
    fn one_bad_field_only_falls_back_itself() {
        // 事件写成字符串而不是数组：其余字段照常生效。
        let config = PushConfig::from_block(Some(&module(json!({
            "enabled": true,
            "events": "failed",
            "content": ["prompt"],
        }))));
        assert!(config.enabled);
        assert_eq!(config.events.len(), 3, "类型不对才回落默认");
        assert_eq!(config.content, vec![PushContent::Prompt]);
    }

    #[test]
    fn unknown_events_are_dropped_but_explicit_empty_list_is_kept() {
        let config = PushConfig::from_block(Some(&module(json!({"events": ["nope", "failed"]}))));
        assert_eq!(config.events, vec![PushEvent::Failed]);
        let config = PushConfig::from_block(Some(&module(json!({"events": []}))));
        assert!(config.events.is_empty(), "全勾掉是用户的明确意图，不回落默认");
        let config = PushConfig::from_block(Some(&module(json!({"content": ["nope"]}))));
        assert!(config.content.is_empty(), "没登记的内容项直接丢掉");
    }

    #[test]
    fn wants_needs_the_master_switch_and_the_event() {
        let mut config = PushConfig::default();
        assert!(!config.wants(PushEvent::Failed), "总开关关着就什么都不发");
        config.enabled = true;
        assert!(config.wants(PushEvent::Failed));
        assert!(!config.wants(PushEvent::SessionFinished), "没勾的事件不推");
    }

    #[test]
    fn channels_without_id_or_provider_are_dropped_and_names_fall_back_to_provider() {
        let config = PushConfig::from_block(Some(&module(json!({
            "channels": [
                { "id": "c1", "provider": "bark", "name": "iPhone", "topic": "k9abc" },
                { "provider": "bark" },
                { "id": "c3" },
                { "id": "c4", "provider": "ntfy", "topic": "ufo" },
            ]
        }))));
        assert_eq!(config.channels.len(), 2);
        assert_eq!(config.channels[0].topic, "k9abc");
        // 没起名字的行回落成服务名：列表左侧不能是空的。
        assert_eq!(config.channels[1].name, "ntfy");
    }

    #[test]
    fn legacy_field_maps_still_parse_into_server_and_topic() {
        // 旧版本把地址与主题存在通用 `fields` 里；漏了这条回落会**静默丢光**用户已有的 ntfy 主题。
        let config = PushConfig::from_block(Some(&module(json!({
            "providers": { "ntfy": { "enabled": true, "fields": { "server": "http://192.168.1.20:8080" } } },
            "channels": [{ "id": "c1", "provider": "ntfy", "name": "家里", "fields": { "topic": "ufo" } }],
        }))));
        assert_eq!(
            config.providers.get("ntfy").map(|item| item.server.as_str()),
            Some("http://192.168.1.20:8080")
        );
        assert_eq!(config.channels[0].topic, "ufo");
        // 新键优先于旧键（用户改过就不该被历史值盖回去）。
        let config = PushConfig::from_block(Some(&module(json!({
            "channels": [{ "id": "c1", "provider": "ntfy", "topic": "new", "fields": { "topic": "old" } }],
        }))));
        assert_eq!(config.channels[0].topic, "new");
    }

    #[test]
    fn topic_and_server_are_clipped() {
        let config = PushConfig::from_block(Some(&module(json!({
            "providers": { "ntfy": { "enabled": true, "server": "h".repeat(5000) } },
            "channels": [{
                "id": "c1",
                "provider": "ntfy",
                "name": "很".repeat(60),
                "topic": "t".repeat(500),
            }],
        }))));
        assert!(config.providers["ntfy"].server.chars().count() <= MAX_SERVER_CHARS);
        assert_eq!(config.channels[0].topic.len(), MAX_TOPIC_CHARS);
        assert_eq!(config.channels[0].name.chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn too_many_channels_are_capped() {
        let items: Vec<_> = (0..60)
            .map(|index| json!({ "id": format!("c{index}"), "provider": "ntfy", "topic": "ufo" }))
            .collect();
        let config = PushConfig::from_block(Some(&module(json!({ "channels": items }))));
        assert_eq!(config.channels.len(), MAX_CHANNELS, "手改出的超长清单要截断");
    }

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(backoff_seconds(60, 0), 60);
        assert_eq!(backoff_seconds(60, 1), 120);
        assert_eq!(backoff_seconds(60, 3), 480);
        // 封顶 600s，且连击很多次也不能溢出成 0。
        assert_eq!(backoff_seconds(60, 30), MAX_BACKOFF_SEC);
        assert_eq!(backoff_seconds(u64::MAX, 3), MAX_BACKOFF_SEC);
    }
}
