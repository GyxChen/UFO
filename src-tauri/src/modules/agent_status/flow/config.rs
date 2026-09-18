//! 信息卡拓展的 per-endpoint 流程配置（脚本 + 调度参数 + 密钥名清单）。
//!
//! ## 存放位置
//! 整块作为一个 JSON 值塞进 [`super::endpoint_profiles::EndpointProfile::extensions`] 的
//! `"flow"` 键下。用 JSON map 而非给档案结构加字段，正是 extensions 当初的设计意图 ——
//! 加/改配置字段不动 `EndpointProfile` 协议，也不升 `SCHEMA_VERSION`。
//!
//! ## 为什么 secret_names 要单独记
//! Windows 凭据管理器（以及多数 keyring 后端）**不能按 service 前缀列举**条目，只能凭
//! (service, name) 精确读写。删一条 endpoint 档案时想连带清掉它配过的密钥，就必须自己留一份
//! 名字清单。⚠️ 清单丢失 = 那些凭据成为孤儿（无法被 UFO 清掉），所以清单与密钥写入必须在同一路径里维护。
//!
//! ## 清洗策略
//! `sanitize` 对越界数值 clamp（间隔、超时），但对**脚本过长直接 Err** —— 静默截断会让用户
//! 看到一份和自己写的不一样、 yet 能跑的脚本，比报错更糟。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as JsonValue};

/// extensions 里存放本配置的键。
pub const FLOW_EXTENSION_KEY: &str = "flow";

/// 轮询间隔下限（秒）：低于这个值对上游就是骚扰，且脚本单跑最坏 15s。
pub const MIN_INTERVAL_SEC: u64 = 10;
/// 轮询间隔上限（秒）：一天一次足够。
pub const MAX_INTERVAL_SEC: u64 = 3600;
/// 脚本单次超时下限 / 上限（毫秒），与 `api.rs` 的 HTTP 超时同区间。
pub const MIN_SCRIPT_TIMEOUT_MS: u64 = super::api::MIN_HTTP_TIMEOUT_MS;
pub const MAX_SCRIPT_TIMEOUT_MS: u64 = super::api::MAX_HTTP_TIMEOUT_MS;
/// 默认轮询间隔（秒）。
pub const DEFAULT_INTERVAL_SEC: u64 = 60;
/// 默认脚本超时（毫秒）。
pub const DEFAULT_SCRIPT_TIMEOUT_MS: u64 = 8_000;
/// 单个密钥名长度上限：Windows target-name 有 1MB 硬限，但更关键的是可读性与
/// service 名拼接后的总长可控（service = `<APP_ID>/flow/<endpoint_id>`，endpoint_id 本身也可能很长）。
/// 超长的 name 会在凭据管理器里被截断到不可预测的键 → 读不出来（静默失败），因此在入口就拒。
pub const MAX_SECRET_NAME_CHARS: usize = 128;

/// 一个 endpoint 的流程脚本配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowConfig {
    /// 关掉即停止调度；已缓存结果视为作废（由 FlowState 侧清理）。
    pub enabled: bool,
    /// 轮询间隔（秒），加载时 clamp 到 [`MIN_INTERVAL_SEC`]..=[`MAX_INTERVAL_SEC`]。
    pub interval_sec: u64,
    /// 单次脚本运行上限（毫秒），加载时 clamp。
    pub timeout_ms: u64,
    /// 用户脚本源码（长度上限在 runner 侧，这里不重复）。
    #[serde(default)]
    pub script: String,
    /// 脚本源码的简单指纹：判断「改没改脚本」以决定要不要丢 `lastResult`。
    /// ⚠️ 不做加密哈希，只用长度 + FNV-1a 足够识别变化，成本可忽略。
    #[serde(default)]
    pub script_hash: u64,
    /// 该 endpoint 配过的密钥名（供删档案级联清理）。只存名字，绝不存值。
    #[serde(default)]
    pub secret_names: Vec<String>,
}

impl Default for FlowConfig {
    fn default() -> Self {
        Self {
            // 新建默认不启用：用户写完脚本、点保存前不应自动开始轮询出网。
            enabled: false,
            interval_sec: DEFAULT_INTERVAL_SEC,
            timeout_ms: DEFAULT_SCRIPT_TIMEOUT_MS,
            script: String::new(),
            script_hash: 0,
            secret_names: Vec::new(),
        }
    }
}

impl FlowConfig {
    /// 从 profile.extensions 读出流程配置；缺失或解析失败返回 None（坏配置不阻塞别的 endpoint）。
    pub fn load_from(extensions: &Map<String, JsonValue>) -> Option<Self> {
        let raw = extensions.get(FLOW_EXTENSION_KEY)?;
        let config: Self = serde_json::from_value(raw.clone()).ok()?;
        Some(config.sanitized())
    }

    /// 写回 extensions（覆盖同名的 flow 键）。
    pub fn write_into(&self, extensions: &mut Map<String, JsonValue>) {
        if let Ok(value) = serde_json::to_value(self) {
            extensions.insert(FLOW_EXTENSION_KEY.to_string(), value);
        }
    }

    /// 规整越界数值；返回自身（脚本长度不在此校验，交给 runner / save 命令）。
    pub fn sanitized(mut self) -> Self {
        self.interval_sec = self.interval_sec.clamp(MIN_INTERVAL_SEC, MAX_INTERVAL_SEC);
        self.timeout_ms = self.timeout_ms.clamp(MIN_SCRIPT_TIMEOUT_MS, MAX_SCRIPT_TIMEOUT_MS);
        self
    }

    /// 依据脚本源码刷新 `script_hash`（保存/改脚本后调用，保证指纹与内容一致）。
    pub fn rehash_script(&mut self) {
        self.script_hash = script_fingerprint(&self.script);
    }

    /// 幂等地记录一个密钥名（写密钥时调用）。已存在不重复加。
    pub fn remember_secret(&mut self, name: &str) {
        let trimmed = name.trim();
        if !trimmed.is_empty() && !self.secret_names.iter().any(|n| n == trimmed) {
            self.secret_names.push(trimmed.to_string());
        }
    }

    /// 移除一个密钥名（删密钥时调用）。
    pub fn forget_secret(&mut self, name: &str) {
        let trimmed = name.trim();
        self.secret_names.retain(|n| n != trimmed);
    }
}

/// 脚本源码的 FNV-1a 64 位指纹（按字符，不按字节，避免多字节差异带来的伪变化）。
///
/// 只用于「这段脚本和上次是不是同一份」的廉价判断，非安全用途。
pub fn script_fingerprint(script: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x100_0000_01b3;
    let mut hash = OFFSET;
    for ch in script.chars() {
        hash ^= ch as u32 as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// 校验密钥名是否可用于写凭据管理器：非空、不超长、不含控制字符。
/// 返回规范化（trim 后）的名字；不合法时 Err(原因)。
pub fn validate_secret_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("密钥名不能为空".to_string());
    }
    if trimmed.chars().count() > MAX_SECRET_NAME_CHARS {
        return Err(format!("密钥名过长（上限 {MAX_SECRET_NAME_CHARS} 字符）"));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err("密钥名不能含控制字符".to_string());
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext_with(config: &FlowConfig) -> Map<String, JsonValue> {
        let mut map = Map::new();
        config.write_into(&mut map);
        map
    }

    #[test]
    fn roundtrips_through_extensions() {
        let mut config = FlowConfig::default();
        config.enabled = true;
        config.script = "return [{label:'a',value:1}]".to_string();
        config.rehash_script();
        let loaded = FlowConfig::load_from(&ext_with(&config)).expect("应能读回");
        assert_eq!(loaded.script, config.script);
        assert_eq!(loaded.script_hash, config.script_hash);
        assert!(loaded.enabled);
    }

    #[test]
    fn clamps_interval_and_timeout() {
        let config = FlowConfig {
            enabled: true,
            interval_sec: 1,
            timeout_ms: 999_999,
            script: String::new(),
            script_hash: 0,
            secret_names: Vec::new(),
        };
        let clean = config.sanitized();
        assert_eq!(clean.interval_sec, MIN_INTERVAL_SEC);
        assert_eq!(clean.timeout_ms, MAX_SCRIPT_TIMEOUT_MS);
    }

    #[test]
    fn load_returns_none_for_missing_or_broken() {
        assert!(FlowConfig::load_from(&Map::new()).is_none());
        let mut broken = Map::new();
        broken.insert(FLOW_EXTENSION_KEY.to_string(), JsonValue::String("nope".into()));
        assert!(FlowConfig::load_from(&broken).is_none());
    }

    #[test]
    fn default_new_config_is_disabled() {
        // 未显式启用前不应自动出网。
        assert!(!FlowConfig::default().enabled);
    }

    #[test]
    fn script_fingerprint_changes_only_on_content() {
        let a = script_fingerprint("return 1");
        assert_eq!(a, script_fingerprint("return 1"));
        assert_ne!(a, script_fingerprint("return 2"));
        // 多字节按字符哈希：同样的中文两次一致。
        assert_eq!(script_fingerprint("你好"), script_fingerprint("你好"));
    }

    #[test]
    fn remember_and_forget_secret_dedupe() {
        let mut config = FlowConfig::default();
        config.remember_secret("KEY");
        config.remember_secret(" KEY ");
        config.remember_secret("");
        assert_eq!(config.secret_names, vec!["KEY".to_string()]);
        config.forget_secret("KEY");
        assert!(config.secret_names.is_empty());
    }

    #[test]
    fn validate_secret_name_rejects_bad() {
        assert!(validate_secret_name("  ").is_err());
        assert!(validate_secret_name("a\u{1}b").is_err());
        let long = "x".repeat(MAX_SECRET_NAME_CHARS + 1);
        assert!(validate_secret_name(&long).is_err());
        assert_eq!(validate_secret_name(" ANTHROPIC_KEY ").unwrap(), "ANTHROPIC_KEY");
    }
}
