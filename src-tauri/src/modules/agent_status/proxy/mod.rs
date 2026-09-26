//! 代理解析：把 agent 会话连接的**本地代理地址**翻译成它真正转发的**上游 endpoint**。
//!
//! ## 为什么需要
//! 用 cc-switch 这类本地代理时，Claude 配置里的 `ANTHROPIC_BASE_URL` 指向的是
//! `http://127.0.0.1:<port>`（代理监听地址），而真实上游（如 `https://api.b.ai`）
//! 只写在代理自己的配置里。悬浮窗只展示 `127.0.0.1` 没有信息量，需要反查出上游。
//!
//! ## 可扩展性（面向未来多代理）
//! 每种代理工具实现一次 [`ProxyResolver`]，在 [`build_registry`] 里注册一行即可：
//! 主窗口的「代理解析器」下拉会自动出现新选项，`SessionSnapshot.upstream_url`
//! 的填充逻辑无需改动。下拉选中的解析器 id 存在
//! [`crate::floating::module_config`]（字段 `proxy_resolver`，`none` = 不解析）。

mod ccswitch;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use serde::Serialize;

use crate::modules::agent_status::status::Provider;

/// 特殊解析器 id：不做任何解析，tooltip 直接显示会话原本的 base_url（通常是真实上游或本地地址）。
pub const NONE_RESOLVER: &str = "none";

/// 本地代理 host 的等价写法：loopback 的 `localhost` 与 `127.0.0.1` / `::1` 视为同一。
const LOOPBACK_HOSTS: &[&str] = &["127.0.0.1", "localhost", "::1", "[::1]"];

/// 解析器可选用的规范主机名（比较时统一按这个）。
const LOOPBACK_CANONICAL: &str = "127.0.0.1";

/// 把一个 agent provider 归一成解析器可用的 app 标识。
///
/// 用独立枚举而非复用 [`Provider`]，方便解析器按需支持更多 agent app 而不改状态协议。
/// ⚠️ 「有这一路」≠「一定被某款代理接管」：如 Qoder 由自己的云网关路由，cc-switch 里
/// 没有它的行，解析器对它只能返回 None（见 `ccswitch::app_key`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentApp {
    Claude,
    Codex,
    Qoder,
}

impl AgentApp {
    fn from_provider(provider: Provider) -> Self {
        match provider {
            Provider::Claude => Self::Claude,
            Provider::Codex => Self::Codex,
            Provider::Qoder => Self::Qoder,
        }
    }
}

/// 一个可选代理解析器的下拉项（id 存配置，label 供 UI 展示）。
#[derive(Debug, Clone, Serialize)]
pub struct ProxyResolverInfo {
    pub id: String,
    pub label: String,
}

/// 代理解析器接口：每种本地代理工具实现一次。
///
/// 实现必须**只读**、**快速**、**不 panic**：它在 emit 快照的热路径上被调用，
/// 且绝不能读取 / 上报任何 token、api key。内部可自带缓存控制读盘频率。
pub trait ProxyResolver: Send + Sync {
    /// 稳定 id，存进用户配置、并作下拉的 value。
    fn id(&self) -> &'static str;

    /// 人类可读名，作下拉的显示文本。
    fn label(&self) -> &'static str;

    /// 本地代理地址命中时返回其转发到的上游 endpoint；
    /// 未配置代理 / 地址不是本工具的监听端口 / 读不到时，一律返回 None（调用方回落原值）。
    fn resolve_upstream(&self, app: AgentApp, local_base_url: &str) -> Option<String>;
}

/// 所有已注册解析器的集合；顺序即下拉顺序。
pub struct ResolverRegistry {
    resolvers: Vec<Box<dyn ProxyResolver>>,
}

impl ResolverRegistry {
    /// 供 UI 展示的选项（`none` 恒在最前）。
    pub fn options(&self) -> Vec<ProxyResolverInfo> {
        let mut options = vec![ProxyResolverInfo {
            id: NONE_RESOLVER.to_string(),
            label: "不解析（直连）".to_string(),
        }];
        options.extend(self.resolvers.iter().map(|resolver| ProxyResolverInfo {
            id: resolver.id().to_string(),
            label: resolver.label().to_string(),
        }));
        options
    }

    /// 按选中的 id 找解析器；`none` / 未知 id 返回 None（即不解析）。
    fn get(&self, id: &str) -> Option<&dyn ProxyResolver> {
        self.resolvers
            .iter()
            .find(|resolver| resolver.id() == id)
            .map(AsRef::as_ref)
    }
}

/// 构造解析器注册表。
///
/// **接入新代理工具只需在这里多 push 一行** —— 下拉、upstream 填充、持久化都自动适配。
/// 参数化 home 目录，便于测试注入样例配置。
pub fn build_registry(home: Option<PathBuf>) -> ResolverRegistry {
    ResolverRegistry {
        resolvers: vec![Box::new(ccswitch::CcSwitchResolver::new(home))],
    }
}

/// 跨窗口共享的解析器选择状态。
///
/// 真正的选择值持久化在 [`crate::floating::module_config`] 的 `proxy_resolver` 字段，
/// 这里缓存一份供 `status` 侧热路径快速读取（避免每次 emit 都去锁配置）。
#[derive(Clone)]
pub struct ProxyState {
    selected: Arc<Mutex<String>>,
    registry: Arc<ResolverRegistry>,
}

impl ProxyState {
    pub fn new(home: Option<PathBuf>) -> Self {
        Self {
            selected: Arc::new(Mutex::new(NONE_RESOLVER.to_string())),
            registry: Arc::new(build_registry(home)),
        }
    }

    /// 主窗口下拉可选项。
    pub fn options(&self) -> Vec<ProxyResolverInfo> {
        self.registry.options()
    }

    pub fn selected(&self) -> String {
        self.selected
            .lock()
            .map(|value| value.clone())
            .unwrap_or_else(|_| NONE_RESOLVER.to_string())
    }

    /// 记录用户选择的解析器 id（由 module_config 写入路径同步调用）。
    pub fn set_selected(&self, id: &str) {
        if let Ok(mut selected) = self.selected.lock() {
            *selected = id.to_string();
        }
    }

    /// 解析会话 base_url 对应的上游 endpoint；不解析 / 未命中时返回 None。
    pub fn resolve_upstream(&self, provider: Provider, base_url: &str) -> Option<String> {
        let selected = self.selected();
        if selected == NONE_RESOLVER {
            return None;
        }
        let app = AgentApp::from_provider(provider);
        self.registry.get(&selected)?.resolve_upstream(app, base_url)
    }
}

/// 从 http(s) URL 中取出「规范化的 host:port」。
///
/// 只做足够反查代理端口用的粗解析（scheme://[userinfo@]host[:port]/path），不引 url crate：
/// - 去掉 scheme、userinfo、path/query/fragment；
/// - loopback 的各种写法统一成 `127.0.0.1`；
/// - 省略默认端口（http→80、https→443），与代理配置里 `15721` 这种显式端口对齐比较。
/// 解析不出 host 时返回 None。
pub fn normalize_host_port(raw: &str) -> Option<String> {
    let after_scheme = raw.split_once("://").map(|(_, rest)| rest).unwrap_or(raw);
    // 截断到第一个分隔符，剥掉 path / query / fragment。
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    // 去掉 userinfo（最后一个 `@` 之后才是 host）。
    let host_port = authority.rsplit_once('@').map(|(_, hp)| hp).unwrap_or(authority);
    if host_port.is_empty() {
        return None;
    }

    // IPv6 形如 `[::1]:15721`；其余按最后一个 `:` 分 host/port（端口段必须是全数字）。
    let (host, port) = split_host_port(host_port);
    let host = normalize_host(host);
    let port = match port {
        Some(value) if value.chars().all(|c| c.is_ascii_digit()) && !value.is_empty() => {
            value.to_string()
        }
        Some(_) => return None,
        None => default_port_for(raw).to_string(),
    };
    Some(format!("{host}:{port}"))
}

/// 拆 host 与 port；无端口或 IPv6 无端口时 port 为 None。
fn split_host_port(authority: &str) -> (&str, Option<&str>) {
    if let Some(rest) = authority.strip_prefix('[') {
        // [::1] 或 [::1]:port
        if let Some(close) = rest.find(']') {
            // close 是 rest（已去掉 `[`）里的下标，切原串时要 +1 才把 `]` 包含进来。
            let host = &authority[..=close + 1];
            return match rest[close + 1..].strip_prefix(':') {
                Some(port) => (host, Some(port)),
                None => (host, None),
            };
        }
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    }
}

/// 把 loopback 主机名统一成规范写法，其它主机名原样（小写）。
fn normalize_host(host: &str) -> String {
    let lowered = host.trim().to_ascii_lowercase();
    if LOOPBACK_HOSTS.contains(&lowered.as_str()) {
        return LOOPBACK_CANONICAL.to_string();
    }
    lowered
}

fn default_port_for(raw: &str) -> u16 {
    if raw.to_ascii_lowercase().starts_with("https") {
        443
    } else {
        80
    }
}

/// 把一个 endpoint 归一成**稳定去重键**：`host:port`（缺省端口按 scheme 补全，大小写归一）。
///
/// 与 [`normalize_host_port`] 同一套规则；`endpoint_profiles` 用它给档案去重 ——
/// 同一上游写成带不带 path、带不带尾斜杠都不影响，端口也统一带上，`host:port` 即用户理解的 endpoint。
/// 解析不出 host 时返回 None（调用方跳过该 endpoint）。
pub fn endpoint_identity(raw: &str) -> Option<String> {
    normalize_host_port(raw)
}

/// 该归一键是否指向本机（loopback）。本地代理地址不作为可配置的 endpoint 登记。
pub fn is_loopback_endpoint(identity: &str) -> bool {
    // identity 形如 `127.0.0.1:15721` 或 `[::1]:8080`；只看 host 段。
    let (host, _) = split_host_port(identity);
    normalize_host(host) == LOOPBACK_CANONICAL
}

/// 判断某个 base_url 是否等价于给定监听地址（`host:port`），比较前双方都走 [`normalize_host_port`]。
pub fn matches_listen_address(base_url: &str, listen_host_port: &str) -> bool {
    match (
        normalize_host_port(base_url),
        normalize_host_port(listen_host_port),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_host_port_handles_common_forms() {
        assert_eq!(
            normalize_host_port("http://127.0.0.1:15721").as_deref(),
            Some("127.0.0.1:15721")
        );
        // localhost 与默认端口归一化。
        assert_eq!(
            normalize_host_port("http://localhost:15721/v1").as_deref(),
            Some("127.0.0.1:15721")
        );
        assert_eq!(
            normalize_host_port("https://api.b.ai").as_deref(),
            Some("api.b.ai:443")
        );
        assert_eq!(
            normalize_host_port("http://example.com").as_deref(),
            Some("example.com:80")
        );
        // userinfo / query 剥离。
        assert_eq!(
            normalize_host_port("http://u:p@127.0.0.1:15721/x?y=1").as_deref(),
            Some("127.0.0.1:15721")
        );
    }

    #[test]
    fn normalize_host_port_rejects_host_with_nonnumeric_port() {
        assert_eq!(normalize_host_port("http://127.0.0.1:abc"), None);
        assert_eq!(normalize_host_port(""), None);
    }

    #[test]
    fn normalize_host_port_keeps_ipv6_brackets_and_port() {
        // 回归：早期实现把 `[::1]:8080` 切成 `[::1:8080`（漏掉 `]`），
        // 会让 loopback 判定失效、本地代理地址被当成可配置 endpoint 登记。
        assert_eq!(
            normalize_host_port("http://[::1]:8080").as_deref(),
            Some("127.0.0.1:8080")
        );
        assert_eq!(normalize_host_port("http://[::1]").as_deref(), Some("127.0.0.1:80"));
        assert!(is_loopback_endpoint("127.0.0.1:8080"));
        assert!(!is_loopback_endpoint("api.b.ai:443"));
        // 裸 host（无 scheme、无端口）也按 loopback 认。
        assert!(is_loopback_endpoint("localhost:80"));
    }

    #[test]
    fn matches_listen_address_compares_normalized() {
        assert!(matches_listen_address(
            "http://localhost:15721",
            "127.0.0.1:15721"
        ));
        assert!(!matches_listen_address(
            "http://localhost:15721",
            "127.0.0.1:8080"
        ));
        // 上游真实地址不匹配本地监听。
        assert!(!matches_listen_address(
            "https://api.b.ai",
            "127.0.0.1:15721"
        ));
    }

    #[test]
    fn options_list_none_first_then_resolvers() {
        let registry = build_registry(Some(PathBuf::from("nonexistent-home")));
        let options = registry.options();
        assert_eq!(options[0].id, NONE_RESOLVER);
        // 至少注册了 cc-switch。
        assert!(options.iter().any(|option| option.id == "cc-switch"));
    }
}
