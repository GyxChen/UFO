//! 信息卡拓展配置：按 endpoint 持久化的「端点档案」。
//!
//! ## 为什么需要
//! 会话快照里的 `base_url` / `upstream_url` 只在会话存活期间存在，会话被清理后就没了，
//! 而 tooltip 的拓展字段是**跟着 endpoint 走**的（同一上游地址共用一套配置）。
//! 因此需要一份以 endpoint 为主键的持久化登记表：UFO 一旦从会话里看到一个**非本地代理**
//! 的 endpoint，就把它落盘，主窗口「信息卡拓展」tab 才能列出可配置的对象。
//!
//! ## 采集口径
//! 有效 endpoint = `upstream_url ?? base_url`（与前端 `displayEndpoint` 同一口径）：
//! - 命中代理解析时登记**反查出的上游**，而不是 `127.0.0.1:15721` 这种代理监听地址；
//! - 未启用解析时登记 `base_url` 本身，但**loopback（127.0.0.1 / localhost / ::1）一律跳过** ——
//!   本地代理地址不是用户要配置的对象，且端口每次重启可能变化，登记下来只会攒垃圾。
//!
//! 采集发生在常驻 monitor 线程（每 500ms 一轮），不在 hook 进程里做：
//! hook 是短命进程、必须快进快出，且不该碰 AppData 配置。

use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::proxy::{endpoint_identity, is_loopback_endpoint};
/// 配置协议版本；不匹配时整份回落默认（与 `module_config` 同一策略）。
const SCHEMA_VERSION: u16 = 1;
/// 登记条数上限：endpoint 数量级很小（个人机器上通常 < 10），
/// 设上限只为防住异常数据把配置撑爆。
const MAX_PROFILES: usize = 64;
/// 单个 endpoint 原始串的字符上限，与会话侧 `base_url` 的截断口径一致。
const MAX_URL_CHARS: usize = 4096;

/// 配置变更时广播给前端的事件名（payload 为 `EndpointProfileList`）。
pub const ENDPOINT_PROFILES_EVENT: &str = "endpoint-profiles";

/// 一条 endpoint 档案。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct EndpointProfile {
    /// 去重键：由 `proxy::endpoint_identity` 归一成 `host:port`（缺省端口按 scheme 补全，
    /// host 小写）。同一 host 写成带不带 path、带不带尾斜杠都会合并到同一条。
    pub id: String,
    /// 首次登记时的原始地址，供 UI 展示（不做归一化，保留用户看得懂的写法）。
    pub url: String,
    /// 首次发现时间（毫秒时间戳）。
    pub first_seen_at_ms: i64,
    /// 拓展字段容器：tooltip 的 per-endpoint 配置（下一步接入）。
    /// 现阶段恒为空对象；用 JSON map 以免每加一个字段就改一次协议。
    #[serde(default)]
    pub extensions: serde_json::Map<String, serde_json::Value>,
}

/// 档案集合的持久化形态。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EndpointProfileList {
    pub schema_version: u16,
    /// 每次写入递增，供前端丢弃乱序事件。
    pub revision: u64,
    pub profiles: Vec<EndpointProfile>,
}

impl Default for EndpointProfileList {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            revision: 0,
            profiles: Vec::new(),
        }
    }
}

impl EndpointProfileList {
    /// 按「首次发现时间」升序排序，同刻按 id 兜底。
    ///
    /// 用发现时间而非任何「最近使用」类字段，是为了让左侧列表顺序稳定 ——
    /// 否则某 endpoint 一有会话就会重排，用户的鼠标会跟着跳。
    fn sort_profiles(&mut self) {
        self.profiles.sort_by(|left, right| {
            left.first_seen_at_ms
                .cmp(&right.first_seen_at_ms)
                .then_with(|| left.id.cmp(&right.id))
        });
    }

    /// 校验并修正加载结果：未知 schema 直接空集合，条目去重 + 丢弃非法项。
    fn sanitize(mut self) -> Self {
        if self.schema_version != SCHEMA_VERSION {
            return Self::default();
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        self.profiles.retain(|profile| {
            // id / url 为空的条目没有意义；重复 id 只保留第一条（已按发现顺序排列）。
            !profile.id.is_empty()
                && !profile.url.trim().is_empty()
                && seen.insert(profile.id.clone())
        });
        self.profiles.truncate(MAX_PROFILES);
        self.sort_profiles();
        self
    }
}

/// 跨窗口共享的档案状态。
#[derive(Clone)]
pub struct EndpointProfileState {
    list: Arc<Mutex<EndpointProfileList>>,
    path: PathBuf,
}

impl EndpointProfileState {
    /// 从磁盘加载；文件缺失 / JSON 非法 / schema 不符都回落空集合（不让一个坏文件卡住启动）。
    pub fn load(path: PathBuf) -> Self {
        let list = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<EndpointProfileList>(&bytes).ok())
            .map(EndpointProfileList::sanitize)
            .unwrap_or_default();
        Self {
            list: Arc::new(Mutex::new(list)),
            path,
        }
    }

    pub fn snapshot(&self) -> EndpointProfileList {
        self.list
            .lock()
            .map(|list| list.clone())
            .unwrap_or_default()
    }

    /// 按 id 读一条档案的拷贝（含 url 与 extensions）；未登记时 None。
    pub fn profile(&self, id: &str) -> Option<EndpointProfile> {
        self.list
            .lock()
            .ok()?
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
    }

    /// 按 id 就地读取该档案的 extensions（只 clone 那一个 map，**不整表 clone**）。
    ///
    /// monitor 每 500ms 的 tick 要用它读流程配置 —— `snapshot()` 会 clone 全部档案含脚本正文，
    /// 单份脚本上限 64KB × 档案上限 64 条，每轮整表 clone 是纯浪费。找不到返回 None。
    pub fn with_profile<T>(
        &self,
        id: &str,
        read: impl FnOnce(&serde_json::Map<String, serde_json::Value>) -> T,
    ) -> Option<T> {
        let guard = self.list.lock().ok()?;
        let profile = guard.profiles.iter().find(|profile| profile.id == id)?;
        Some(read(&profile.extensions))
    }

    /// 按 id 就地修改 extensions；有该档案时写盘并返回新列表，否则 Err。
    ///
    /// 供主窗口「信息卡拓展」保存流程配置、增删密钥名清单用。revision 递增让前端即时同步。
    pub fn update_extensions(
        &self,
        id: &str,
        edit: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>),
    ) -> Result<EndpointProfileList, String> {
        let updated = {
            let mut list = self
                .list
                .lock()
                .map_err(|_| "endpoint profile lock poisoned".to_string())?;
            match list
                .profiles
                .iter_mut()
                .find(|profile| profile.id == id.trim())
            {
                Some(profile) => {
                    edit(&mut profile.extensions);
                    list.revision = list.revision.saturating_add(1);
                    true
                }
                None => false,
            }
        };
        if !updated {
            return Err("该 endpoint 未登记".to_string());
        }
        self.persist()?;
        Ok(self.snapshot())
    }

    /// 已登记条目的 id 集合，供采集路径做「是否需要落盘」的廉价判断。
    fn known_ids(&self) -> BTreeSet<String> {
        self.list
            .lock()
            .map(|list| list.profiles.iter().map(|profile| profile.id.clone()).collect())
            .unwrap_or_default()
    }

    /// 批量登记若干 endpoint（monitor 每轮调用）；有新增时写盘并返回 true。
    ///
    /// 只在真的冒出未见过的 id 时才读盘写文件 —— 同一批 endpoint 每 500ms 都会进来，
    /// 无脑落盘会让配置文件写入次数高到没意义。
    pub fn record_all(&self, urls: &[String], now_ms: i64) -> bool {
        let fresh: Vec<(String, String)> = urls
            .iter()
            .filter_map(|raw| endpoint_key(raw))
            .filter(|(id, _)| !is_loopback_endpoint(id))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if fresh.is_empty() {
            return false;
        }
        let known = self.known_ids();
        let pending: Vec<(String, String)> = fresh
            .into_iter()
            .filter(|(id, _)| !known.contains(id))
            .collect();
        if pending.is_empty() {
            return false;
        }
        self.insert_many(&pending, now_ms)
    }

    /// 登记单条（主窗口手动添加）；已存在时不写盘，返回当前列表。
    pub fn record_one(&self, raw_url: &str, now_ms: i64) -> Result<EndpointProfileList, String> {
        let (id, url) = endpoint_key(raw_url).ok_or_else(|| "invalid endpoint".to_string())?;
        if is_loopback_endpoint(&id) {
            return Err("本地代理地址无需登记".to_string());
        }
        let exists = self
            .list
            .lock()
            .map(|list| list.profiles.iter().any(|profile| profile.id == id))
            .unwrap_or(false);
        if !exists {
            self.insert_many(&[(id, url)], now_ms);
        }
        Ok(self.snapshot())
    }

    /// 删除一条档案；id 不存在时返回 Err（前端据此回滚 UI）。
    pub fn remove(&self, id: &str) -> Result<EndpointProfileList, String> {
        let target = id.trim();
        if target.is_empty() {
            return Err("endpoint id 为空".to_string());
        }
        let deleted = {
            let mut list = self
                .list
                .lock()
                .map_err(|_| "endpoint profile lock poisoned".to_string())?;
            let before = list.profiles.len();
            list.profiles.retain(|profile| profile.id != target);
            let deleted = list.profiles.len() != before;
            if deleted {
                list.revision = list.revision.saturating_add(1);
            }
            deleted
        };
        if !deleted {
            return Err("该 endpoint 未登记".to_string());
        }
        self.persist()?;
        Ok(self.snapshot())
    }

    /// 写入若干新条目并落盘；返回是否真的有变化。
    fn insert_many(&self, pending: &[(String, String)], now_ms: i64) -> bool {
        let changed = {
            let mut list = match self.list.lock() {
                Ok(list) => list,
                Err(_) => return false,
            };
            let before = list.profiles.len();
            for (id, url) in pending {
                if list.profiles.len() >= MAX_PROFILES {
                    break;
                }
                if list.profiles.iter().any(|profile| &profile.id == id) {
                    continue;
                }
                list.profiles.push(EndpointProfile {
                    id: id.clone(),
                    url: url.clone(),
                    first_seen_at_ms: now_ms,
                    extensions: serde_json::Map::new(),
                });
            }
            if list.profiles.len() == before {
                false
            } else {
                list.revision = list.revision.saturating_add(1);
                list.sort_profiles();
                true
            }
        };
        if !changed {
            return false;
        }
        if let Err(error) = self.persist() {
            eprintln!("failed to persist endpoint profiles: {error}");
        }
        true
    }

    /// 原子写盘：先写 `.tmp`，删旧文件，rename（Windows 下 rename 不覆盖已存在文件）。
    fn persist(&self) -> Result<(), String> {
        let snapshot = self.snapshot();
        let encoded = serde_json::to_vec_pretty(&snapshot).map_err(|error| error.to_string())?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let temporary = self.path.with_extension("tmp");
        fs::write(&temporary, encoded).map_err(|error| error.to_string())?;
        if self.path.exists() {
            fs::remove_file(&self.path).map_err(|error| error.to_string())?;
        }
        fs::rename(&temporary, &self.path).map_err(|error| error.to_string())
    }
}

/// 归一化一条 endpoint：`(去重键, 展示原串)`；非法或为空时 None（跳过登记）。
fn endpoint_key(raw: &str) -> Option<(String, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_URL_CHARS {
        return None;
    }
    let id = endpoint_identity(trimmed)?;
    Some((id, trimmed.to_string()))
}

#[tauri::command]
pub fn get_endpoint_profiles(state: State<'_, EndpointProfileState>) -> EndpointProfileList {
    state.snapshot()
}

/// 手动登记一条 endpoint（主窗口）。写盘后广播，其它窗口即时同步。
#[tauri::command]
pub fn add_endpoint_profile(
    app: AppHandle,
    state: State<'_, EndpointProfileState>,
    url: String,
) -> Result<EndpointProfileList, String> {
    let list = state.record_one(&url, crate::status::now_ms())?;
    let _ = app.emit(ENDPOINT_PROFILES_EVENT, &list);
    Ok(list)
}

/// 删除一条 endpoint 档案（主窗口）。
///
/// ⚠️ 级联清理流程凭据必须在 `remove` **之前**做：密钥名清单存在该档案的 extensions 里，
/// 删了行就读不到了。清理只在 flow 侧 warn 失败，不阻断删除。
#[tauri::command]
pub fn delete_endpoint_profile(
    app: AppHandle,
    state: State<'_, EndpointProfileState>,
    flow: State<'_, crate::flow::FlowState>,
    id: String,
) -> Result<EndpointProfileList, String> {
    flow.cleanup_for_profile(&state, id.trim());
    let list = state.remove(&id)?;
    let _ = app.emit(ENDPOINT_PROFILES_EVENT, &list);
    // 再广播一次流程结果：该 endpoint 的字段已被清掉，tooltip 得立刻收回去。
    let _ = app.emit(crate::flow::FLOW_RESULTS_EVENT, flow.snapshot());
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::test_support::TempDir;

    fn state(tag: &str) -> (TempDir, EndpointProfileState) {
        let dir = TempDir::new(tag);
        let state = EndpointProfileState::load(dir.path().join("endpoint-profiles.json"));
        (dir, state)
    }

    #[test]
    fn loads_empty_when_file_missing() {
        let (_dir, state) = state("missing");
        let list = state.snapshot();
        assert_eq!(list.schema_version, SCHEMA_VERSION);
        assert!(list.profiles.is_empty());
    }

    #[test]
    fn records_non_local_endpoint_and_persists() {
        let (dir, state) = state("record");
        let urls = vec!["https://api.example.com".to_string()];
        assert!(state.record_all(&urls, 1_000));
        let list = state.snapshot();
        assert_eq!(list.profiles.len(), 1);
        assert_eq!(list.profiles[0].id, "api.example.com:443");
        assert_eq!(list.profiles[0].url, "https://api.example.com");
        assert_eq!(list.profiles[0].first_seen_at_ms, 1_000);
        // 真的写盘了，且重读一致。
        assert!(dir.path().join("endpoint-profiles.json").exists());
        let reloaded = EndpointProfileState::load(dir.path().join("endpoint-profiles.json"));
        assert_eq!(reloaded.snapshot().profiles, list.profiles);
    }

    #[test]
    fn skips_loopback_addresses() {
        let (_dir, state) = state("loopback");
        let urls = [
            "http://127.0.0.1:15721".to_string(),
            "http://localhost:15721".to_string(),
            "http://[::1]:8080".to_string(),
        ];
        assert!(!state.record_all(&urls, 1_000));
        assert!(state.snapshot().profiles.is_empty());
    }

    #[test]
    fn dedupes_by_identity_and_reports_no_change() {
        let (_dir, state) = state("dedupe");
        assert!(state.record_all(&["https://api.example.com/v1".to_string()], 1_000));
        // 同 host 不同 path：归一键相同 → 不重复登记、也不写盘。
        assert!(!state.record_all(&["https://api.example.com".to_string()], 2_000));
        let list = state.snapshot();
        assert_eq!(list.profiles.len(), 1);
        assert_eq!(list.profiles[0].url, "https://api.example.com/v1");
    }

    #[test]
    fn ignores_blank_and_unparseable_values() {
        let (_dir, state) = state("blank");
        let urls = vec![String::new(), "   ".to_string(), "://".to_string()];
        assert!(!state.record_all(&urls, 1_000));
        assert!(state.snapshot().profiles.is_empty());
    }

    #[test]
    fn orders_by_first_seen_then_id() {
        let (_dir, state) = state("order");
        assert!(state.record_all(&["https://b.example.com".to_string()], 2_000));
        assert!(state.record_all(&["https://a.example.com".to_string()], 1_000));
        let loaded = state.snapshot();
        let ids: Vec<_> = loaded
            .profiles
            .iter()
            .map(|profile| profile.id.as_str())
            .collect();
        // 先发现的排前面，不看字母序。
        assert_eq!(ids, vec!["a.example.com:443", "b.example.com:443"]);
    }

    #[test]
    fn caps_profile_count() {
        let (_dir, state) = state("cap");
        let urls: Vec<String> = (0..(MAX_PROFILES + 10))
            .map(|index| format!("https://host{index}.example.com"))
            .collect();
        assert!(state.record_all(&urls, 1_000));
        assert_eq!(state.snapshot().profiles.len(), MAX_PROFILES);
    }

    #[test]
    fn removes_by_id() {
        let (_dir, state) = state("remove");
        assert!(state.record_all(&["https://api.example.com".to_string()], 1_000));
        let list = state.remove("api.example.com:443").unwrap();
        assert!(list.profiles.is_empty());
        assert_eq!(list.revision, 2);
    }

    #[test]
    fn manual_add_rejects_loopback_and_blank() {
        let (_dir, state) = state("manual");
        assert!(state.record_one("http://127.0.0.1:15721", 1_000).is_err());
        assert!(state.record_one("   ", 1_000).is_err());
        assert!(state.record_one("https://ok.example.com", 1_000).is_ok());
        // 重复添加：幂等，不报错也不新增。
        let again = state.record_one("https://ok.example.com", 2_000).unwrap();
        assert_eq!(again.profiles.len(), 1);
    }

    #[test]
    fn corrupt_file_falls_back_to_empty() {
        let (dir, _) = state("corrupt");
        let path = dir.path().join("endpoint-profiles.json");
        fs::write(&path, b"not json").expect("write junk");
        let loaded = EndpointProfileState::load(path);
        assert!(loaded.snapshot().profiles.is_empty());
    }

    #[test]
    fn drops_legacy_entries_with_unknown_schema() {
        // schema 不符时整份清空，而不是把无法解释的结构继续用下去。
        let (dir, _) = state("schema");
        let path = dir.path().join("endpoint-profiles.json");
        fs::write(
            &path,
            br#"{"schema_version":99,"revision":3,"profiles":[{"id":"a","url":"a","first_seen_at_ms":1,"extensions":{}}]}"#,
        )
        .expect("write v99");
        assert!(EndpointProfileState::load(path).snapshot().profiles.is_empty());
    }

    #[test]
    fn sanitize_dedupes_and_clamps_loaded_list() {
        // 手改配置塞重复 id / 空 id：加载时清掉，长度按上限截断。
        let list = EndpointProfileList {
            schema_version: SCHEMA_VERSION,
            revision: 5,
            profiles: {
                let mut profiles = vec![
                    EndpointProfile {
                        id: "dup".to_string(),
                        url: "https://dup".to_string(),
                        first_seen_at_ms: 1,
                        extensions: serde_json::Map::new(),
                    },
                    EndpointProfile {
                        id: "dup".to_string(),
                        url: "https://dup-2".to_string(),
                        first_seen_at_ms: 2,
                        extensions: serde_json::Map::new(),
                    },
                    EndpointProfile {
                        id: String::new(),
                        url: "https://no-id".to_string(),
                        first_seen_at_ms: 3,
                        extensions: serde_json::Map::new(),
                    },
                ];
                profiles.extend((0..(MAX_PROFILES + 5)).map(|index| EndpointProfile {
                    id: format!("host{index}"),
                    url: format!("https://host{index}"),
                    first_seen_at_ms: 10 + index as i64,
                    extensions: serde_json::Map::new(),
                }));
                profiles
            },
        };
        let clean = list.sanitize();
        assert_eq!(clean.profiles.len(), MAX_PROFILES);
        assert_eq!(clean.profiles.iter().filter(|p| p.id == "dup").count(), 1);
        assert!(!clean.profiles.iter().any(|p| p.id.is_empty()));
    }

    #[test]
    fn extensions_serialize_as_empty_object_by_default() {
        let json = serde_json::to_string(&EndpointProfile {
            id: "api.example.com:443".to_string(),
            url: "https://api.example.com".to_string(),
            first_seen_at_ms: 1,
            extensions: serde_json::Map::new(),
        })
        .unwrap();
        assert!(json.contains("\"extensions\":{}"));
    }
}
