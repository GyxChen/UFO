//! cc-switch 本地代理解析器。
//!
//! cc-switch 在 `~/.cc-switch/` 下维护 `cc-switch.db`（SQLite）：
//! - `proxy_config`：代理服务器与各 app 的接管状态。命中解析需**两列同时为 1**：
//!   - `proxy_enabled`：全局代理服务器开关（UPDATE 无 WHERE，三行同值镜像，服务器起来才为 1）。
//!   - `enabled`：该 app 是否被代理接管（每 app 独立；`takeover_single_proxy` 只翻本 app 行）。
//!   另含监听地址 `listen_address:listen_port`（`listen_port` 是 INTEGER，非 TEXT）。
//! - `providers`：每个 provider 的 `settings_config` JSON，`env` 块内含 `ANTHROPIC_BASE_URL`
//!   （真实上游）—— 同一块里通常还有 `ANTHROPIC_AUTH_TOKEN`。
//!
//! 反查链路：会话 base_url 的 `host:port` 命中某 app 已接管（`proxy_enabled=1` 且 `enabled=1`）
//! 的 `proxy_config` 监听地址时，返回该 app 下 `is_current=1` 的 provider 的上游
//! `ANTHROPIC_BASE_URL`。命中条件只看 db 里这两列与会话实际 base_url：cc-switch 关掉代理
//! 时会把 `ANTHROPIC_BASE_URL` 改回真实上游，会话 base_url 便不再匹配本地监听地址，
//! 自然回落不解析，无需再读 `settings.json`。
//!
//! 只读、短连接、带 TTL 缓存；`settings_config` 整块 JSON 会进内存解析，但**只提取
//! `env.ANTHROPIC_BASE_URL` 这一个 key**，token / api key 一律不读取、不缓存、不上报。

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use rusqlite::Connection;
use serde_json::Value;

use super::{matches_listen_address, AgentApp, ProxyResolver};

/// 缓存有效期：cc-switch 里手动切 provider 后，最迟这么久反映到悬浮窗。
/// 也让 emit 快照的热路径不必每次都开 SQLite。
const CACHE_TTL: Duration = Duration::from_secs(5);

/// 某 app 在 cc-switch 里的代理状态快照（与具体会话的 base_url 无关，可安全缓存）。
///
/// 缓存这份「代理长什么样」而非「某地址解析成什么」—— 因为同一 app 下不同会话可能连
/// 不同 base_url（有的经代理、有的直连），匹配必须每次按各自 base_url 现算，
/// 但代理监听地址与当前上游是全局唯一的，缓存它才对任意 base_url 都成立。
#[derive(Clone)]
struct AppProxyInfo {
    /// 已启用代理时的监听 `host:port`；未启用 / 读不到时 None（该 app 不解析）。
    listen_addr: Option<String>,
    /// 当前 provider 的上游 endpoint；读不到时 None。
    upstream: Option<String>,
}

struct CacheEntry {
    info: AppProxyInfo,
    fetched_at: Instant,
}

pub struct CcSwitchResolver {
    /// `~/.cc-switch` 目录；None 表示未探测到（无 db），此时恒不解析。
    root: Option<PathBuf>,
    cache: Mutex<HashMap<&'static str, CacheEntry>>,
}

impl CcSwitchResolver {
    /// home 为 `None` 时按当前进程环境探测用户主目录。
    pub fn new(home: Option<PathBuf>) -> Self {
        let root = home
            .or_else(crate::status::home_dir)
            .map(|h| h.join(".cc-switch"))
            .filter(|path: &PathBuf| path.join("cc-switch.db").is_file());
        Self {
            root,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// 取该 app 的代理状态；过期 / 无缓存时读库刷新并写回。
    fn proxy_info(&self, db: &Path, app: AgentApp) -> AppProxyInfo {
        let key = app_key(app);
        if let Ok(cache) = self.cache.lock() {
            if let Some(entry) = cache.get(key) {
                if entry.fetched_at.elapsed() < CACHE_TTL {
                    return entry.info.clone();
                }
            }
        }
        let info = self.read_proxy_info(db, app);
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(
                key,
                CacheEntry {
                    info: info.clone(),
                    fetched_at: Instant::now(),
                },
            );
        }
        info
    }

    /// 从 SQLite 读取该 app 的监听地址与当前上游；读不到一律 None 字段。
    fn read_proxy_info(&self, db: &Path, app: AgentApp) -> AppProxyInfo {
        // 只读：cc-switch 应用持有该库，用 READ_ONLY 避免争写锁。
        let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY;
        let Ok(conn) = Connection::open_with_flags(db, flags) else {
            return AppProxyInfo { listen_addr: None, upstream: None };
        };
        // 快速失败：库被占时不长时间阻塞热路径。
        let _ = conn.busy_timeout(Duration::from_millis(200));

        let app_type = app_key(app);
        // ① 该 app 的代理是否启用、监听在哪个 host:port。
        // 语义来自 cc-switch 源码（`src-tauri/src/database/schema.rs` + `services/proxy.rs`）：
        //   - `proxy_enabled`：**全局服务器开关**（三行同值，服务器起来才为 1）。
        //   - `enabled`：**该 app 是否被代理接管**（每 app 独立）。
        // 会话经代理 ⟺ 两列同时为 1；只看 `proxy_enabled` 会在「全局开但此 app 未接管」时误命中。
        // ⚠️ `listen_port` 是 `INTEGER`（DEFAULT 15721），必须按整数读：rusqlite 的 String
        // 只接受 Text，读 INTEGER 列会 InvalidType 整行失败，导致功能空转（历史 bug）。
        let listen: Option<(String, i64, i64, i64)> = conn
            .query_row(
                "SELECT listen_address, listen_port, proxy_enabled, enabled \
                 FROM proxy_config WHERE app_type = ?1",
                [app_type],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .ok();
        let proxy_on = listen
            .as_ref()
            .is_some_and(|(_, _, global, app_enabled)| *global == 1 && *app_enabled == 1);
        let listen_addr = listen
            .filter(|_| proxy_on)
            .map(|(address, port, _, _)| format!("{address}:{port}"));

        // 代理没开则无需再查上游。
        if listen_addr.is_none() {
            return AppProxyInfo { listen_addr: None, upstream: None };
        }

        // ② 当前 provider 的上游：只取 env.ANTHROPIC_BASE_URL 一个 key。
        let settings: Option<String> = conn
            .query_row(
                "SELECT settings_config FROM providers WHERE app_type = ?1 AND is_current = 1 LIMIT 1",
                [app_type],
                |row| row.get(0),
            )
            .ok();
        let upstream = settings.as_deref().and_then(base_url_from_settings);
        AppProxyInfo { listen_addr, upstream }
    }
}

/// app 在 cc-switch 库里的 `app_type` 取值 / 缓存键。
fn app_key(app: AgentApp) -> &'static str {
    match app {
        AgentApp::Claude => "claude",
        AgentApp::Codex => "codex",
    }
}

/// 从 provider 的 settings_config JSON 里取 `env.ANTHROPIC_BASE_URL`。
///
/// 只按名字取这一个 key —— settings_config 里同时存有 token，绝不整体读取或上报。
/// 缺失 / 非字符串 / 空白一律视为未命中。
fn base_url_from_settings(raw: &str) -> Option<String> {
    let value: Value = serde_json::from_str(raw).ok()?;
    value
        .get("env")?
        .get("ANTHROPIC_BASE_URL")?
        .as_str()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(ToString::to_string)
}

impl ProxyResolver for CcSwitchResolver {
    fn id(&self) -> &'static str {
        "cc-switch"
    }

    fn label(&self) -> &'static str {
        "cc-switch"
    }

    fn resolve_upstream(&self, app: AgentApp, local_base_url: &str) -> Option<String> {
        let root = self.root.as_ref()?;
        let info = self.proxy_info(&root.join("cc-switch.db"), app);
        // 会话连的必须是该 app 代理的监听地址，否则不是经本代理转发（如直连场景）。
        let listen_addr = info.listen_addr?;
        if !matches_listen_address(local_base_url, &listen_addr) {
            return None;
        }
        info.upstream
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::test_support::TempDir;
    use rusqlite::Connection;

    #[test]
    fn base_url_from_settings_reads_only_env_base_url() {
        let raw = r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"secret","ANTHROPIC_BASE_URL":"  https://api.b.ai  "}}"#;
        assert_eq!(
            base_url_from_settings(raw).as_deref(),
            Some("https://api.b.ai")
        );
    }

    #[test]
    fn base_url_from_settings_rejects_missing_or_blank_or_bad_json() {
        assert_eq!(base_url_from_settings("{}"), None);
        assert_eq!(base_url_from_settings(r#"{"env":{}}"#), None);
        assert_eq!(
            base_url_from_settings(r#"{"env":{"ANTHROPIC_BASE_URL":"   "}}"#),
            None
        );
        assert_eq!(base_url_from_settings("not json"), None);
        // token 在，但没有 base_url：不能误返回任何东西。
        assert_eq!(
            base_url_from_settings(r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"secret"}}"#),
            None
        );
    }

    /// 在临时 `.cc-switch` 目录里造一份最小可用的 cc-switch.db。
    ///
    /// 参数只作用于 **claude 行**：`listen_port` 监听端口、`global_proxy_on` 全局服务器开关
    /// （`proxy_enabled`）、`claude_taken_over` 本 app 是否被接管（`enabled`）；当前 provider
    /// 上游写成 `upstream`。**codex 行两开关都给 0**，用于验证按 app 分流。
    ///
    /// ⚠️ `listen_port` 用 `INTEGER` —— 与 cc-switch 真实 schema（`listen_port INTEGER DEFAULT 15721`）
    /// 一致；早先误用 TEXT 掩盖了「按 String 读 INTEGER 列会整行失败」的回归，改回 TEXT 会让测试失去保护。
    /// ⚠️ `proxy_enabled`（全局）与 `enabled`（本 app 接管）两列都得建，解析要求两者同时为 1；
    /// 缺任一列会让整行查询失败——真实库里这两列都存在。
    fn seed_db(
        temp: &TempDir,
        listen_port: i64,
        global_proxy_on: bool,
        claude_taken_over: bool,
        upstream: &str,
    ) -> PathBuf {
        let root = temp.path().join(".cc-switch");
        std::fs::create_dir_all(&root).expect("create root");
        let db = root.join("cc-switch.db");
        let conn = Connection::open(&db).expect("create db");
        // 全局开关：cc-switch 用无 WHERE 的 UPDATE 镜像三行，这里按同一取值写入。
        let global = i64::from(global_proxy_on);
        conn.execute_batch(
            r#"
            CREATE TABLE proxy_config (
                app_type TEXT PRIMARY KEY, listen_address TEXT, listen_port INTEGER,
                proxy_enabled INTEGER, enabled INTEGER);
            CREATE TABLE providers (id TEXT, app_type TEXT, is_current INTEGER, settings_config TEXT);
            "#,
        )
        .expect("create schema");
        // claude：接管状态由参数决定；codex：两开关都关。
        conn.execute(
            "INSERT INTO proxy_config VALUES ('claude','127.0.0.1',?1,?2,?3)",
            rusqlite::params![listen_port, global, i64::from(claude_taken_over)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO proxy_config VALUES ('codex','127.0.0.1',?1,0,0)",
            rusqlite::params![listen_port],
        )
        .unwrap();
        let settings = format!(
            r#"{{"env":{{"ANTHROPIC_AUTH_TOKEN":"secret","ANTHROPIC_BASE_URL":"{upstream}"}}}}"#
        );
        conn.execute(
            "INSERT INTO providers VALUES ('cur','claude',1,?1)",
            [settings],
        )
        .expect("seed provider");
        db
    }

    /// 常用夹具：claude 全局开 + 已接管、监听 15721、上游 `upstream`。
    fn seed_taken_over(temp: &TempDir, upstream: &str) -> PathBuf {
        seed_db(temp, 15721, true, true, upstream)
    }

    fn resolver_for(temp: &TempDir) -> CcSwitchResolver {
        // seed 之后再建解析器，root 探测到 cc-switch.db。
        CcSwitchResolver::new(Some(temp.path().to_path_buf()))
    }

    #[test]
    fn resolve_maps_local_proxy_to_current_upstream() {
        let temp = TempDir::new("ccswitch-ok");
        seed_taken_over(&temp, "https://api.b.ai");
        let resolver = resolver_for(&temp);
        assert_eq!(
            resolver
                .resolve_upstream(AgentApp::Claude, "http://127.0.0.1:15721")
                .as_deref(),
            Some("https://api.b.ai")
        );
    }

    #[test]
    fn resolve_returns_none_when_listen_port_mismatches() {
        // 会话连的是 9999，但 claude 代理监听 15721：不是经本代理，不解析。
        let temp = TempDir::new("ccswitch-port");
        seed_taken_over(&temp, "https://api.b.ai");
        let resolver = resolver_for(&temp);
        assert_eq!(
            resolver.resolve_upstream(AgentApp::Claude, "http://127.0.0.1:9999"),
            None
        );
    }

    #[test]
    fn resolve_returns_none_when_proxy_disabled_for_app() {
        // 该 app 未被接管（enabled=0），即使全局服务器开着也不解析。
        let temp = TempDir::new("ccswitch-off");
        seed_db(&temp, 15721, true, false, "https://api.b.ai");
        let resolver = resolver_for(&temp);
        assert_eq!(
            resolver.resolve_upstream(AgentApp::Claude, "http://127.0.0.1:15721"),
            None
        );
    }

    #[test]
    fn resolve_returns_none_when_global_server_off_even_if_app_enabled() {
        // 全局服务器关（proxy_enabled=0）但本 app 残留 enabled=1：服务器没在转发，不解析。
        let temp = TempDir::new("ccswitch-global-off");
        seed_db(&temp, 15721, false, true, "https://api.b.ai");
        let resolver = resolver_for(&temp);
        assert_eq!(
            resolver.resolve_upstream(AgentApp::Claude, "http://127.0.0.1:15721"),
            None
        );
    }

    #[test]
    fn resolve_is_app_scoped_codex_proxy_disabled() {
        let temp = TempDir::new("ccswitch-codex");
        seed_taken_over(&temp, "https://api.b.ai");
        let resolver = resolver_for(&temp);
        // codex 的两开关都是 0，同样地址不解析（按 app 分流）。
        assert_eq!(
            resolver.resolve_upstream(AgentApp::Codex, "http://127.0.0.1:15721"),
            None
        );
    }

    #[test]
    fn cache_holds_stale_value_within_ttl() {
        // 命中后结果被缓存：改库内 provider 上游，TTL 内仍返回旧值。
        let temp = TempDir::new("ccswitch-cache");
        let db = seed_taken_over(&temp, "https://first.example");
        let resolver = resolver_for(&temp);
        assert_eq!(
            resolver
                .resolve_upstream(AgentApp::Claude, "http://127.0.0.1:15721")
                .as_deref(),
            Some("https://first.example")
        );
        // 直连库改上游（模拟 cc-switch 里切了 provider）。
        let conn = Connection::open(&db).unwrap();
        conn.execute(
            "UPDATE providers SET settings_config='{\"env\":{\"ANTHROPIC_BASE_URL\":\"https://second.example\"}}'",
            [],
        )
        .unwrap();
        drop(conn);
        // 仍在 5s TTL 内：返回缓存的旧值，而不是读新库。
        assert_eq!(
            resolver
                .resolve_upstream(AgentApp::Claude, "http://127.0.0.1:15721")
                .as_deref(),
            Some("https://first.example")
        );
    }

    #[test]
    fn new_without_db_disables_resolution() {
        // 指向一个不存在 cc-switch.db 的目录，root 应为 None，resolve 恒 None。
        let resolver = CcSwitchResolver::new(Some(PathBuf::from("Z:/definitely-not-here")));
        assert!(resolver.root.is_none());
        assert_eq!(
            resolver.resolve_upstream(AgentApp::Claude, "http://127.0.0.1:15721"),
            None
        );
    }
}
