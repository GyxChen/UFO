//! 会话记录持久化（SQLite）。
//!
//! ## 为什么需要
//! Agent 会话状态原本完全活在内存里（`StatusRegistry` 的 HashMap），进程重启即丢失，
//! `Completed` 会话还会按 retention 被清掉。后果是同一个 `session_id`（如 `claude --resume`
//! 继续旧对话）在 UFO 眼里是全新会话：累计 token 归零、`started_at_ms` 变成「现在」（灯跳位）、
//! `prompt` / `base_url` 要等下一次事件才有值。本模块把每个会话「跨重启仍有意义」的字段落 SQLite，
//! 收到事件时按 `provider:session_id` 命中存档 → 回填这些字段，让灯位与统计连续。
//!
//! ## 只 GUI 侧持有
//! 只有常驻 GUI 进程读写这个库；hook 二进制（`status::hook`）必须继续快进快出、**不打开它**
//! （每次事件都启动一次，绝不能碰 AppData 的 SQLite）。
//!
//! ## 版本策略（两套版本，别混）
//! - [`SCHEMA_VERSION`]：**线协议**版本（`SessionRecord` ↔ 前端）。按项目硬规则**永不递增** ——
//!   加字段一律 `Option` + `#[serde(default)]`，否则旧数据被静默丢弃。
//! - `PRAGMA user_version`：**库内布局**版本。将来加列时递增它并配一次 `ALTER TABLE`，不动线协议。
//!
//! ## 锁纪律
//! `rusqlite::Connection` 是 `Send` 而非 `Sync`，必须 `Mutex` 包住才能跨线程共享
//! （monitor 线程刷盘 + 主窗口命令读写）。加锁顺序恒为 **inner → seeds**（`inner` 里做 DB I/O，
//! 出来再同步 `seeds` 缓存）；`seed_for` 只碰 `seeds`、不碰 `inner`，因此热路径回填零 I/O。

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::status::{now_ms, Provider, SessionSnapshot, StoredSeed, TokenTotals};

/// 线协议版本（前后端之间）；**不得递增**，见模块头「版本策略」。
const SCHEMA_VERSION: u16 = 1;
/// 库内布局版本，存 `PRAGMA user_version`；加列时递增并补一次迁移。
const SCHEMA_USER_VERSION: i32 = 1;
/// 存档行数上限：超出时按 `last_seen` 最旧优先淘汰，当前存活会话豁免。
const MAX_RECORDS: usize = 200;
/// 启动期一次性清理：`last_seen` 超过 30 天的行直接删。
const MAX_RECORD_AGE_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// 库被占时的等待上限：仅防住「两个实例同时开着」这种意外，正常只此一处连接。
const BUSY_TIMEOUT: Duration = Duration::from_millis(2_000);

/// 存档变更时广播给前端的事件名（payload 为 [`SessionRecordList`]）。
pub const SESSION_RECORDS_EVENT: &str = "session-records";

/// 一行会话存档。字段全部取自 [`SessionSnapshot`] 里**跨重启仍有意义**的那些。
///
/// 不存：`phase` / `is_flashing` / `remove_at_ms` / `completed_at_ms` / `pid` /
/// `error_code` / `last_sequence` / `source` / `confidence` —— 都是存活态字段，
/// 灯色必须由本次事件决定；旧 `pid` 回填进来会让存活检查立刻判死。
///
/// reducer 只看到 [`StoredSeed`]（回填所需四字段），不依赖本存储类型 —— 这里是持久化行
/// 到领域边界的转换点。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionRecord {
    /// 主键 = `session_key(provider, session_id)`，即 `"claude:<uuid>"`。
    pub key: String,
    /// 来源 provider（`"claude"` / `"codex"`）。
    pub provider: Provider,
    /// 原始会话标识（不含 provider 前缀）。
    pub session_id: String,
    /// 用户最近一次提问原文（明文长期驻留磁盘，见 CLAUDE.md ⚠️）。
    pub prompt: Option<String>,
    /// 会话实际使用的 API endpoint。存 `base_url`，**不存**反查出的 `upstream_url`
    /// （后者是当前解析器现算的，跨重启可能已失效）。
    pub base_url: Option<String>,
    /// 会话最早创建时刻：resume 时回填它，让指示灯排序位置不跳。
    pub started_at_ms: i64,
    /// 最后一次活动时刻：淘汰与 30 天过期都按它算。
    pub last_seen_at_ms: i64,
    /// 累计 token；四项全 0 时序列化为 `null`，与快照「未统计到」的表现一致。
    #[serde(default)]
    pub tokens: Option<TokenTotals>,
}

/// 存档集合的线协议形态；与 `EndpointProfileList` 同构，前端丢乱序事件的逻辑可直接照抄。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionRecordList {
    pub schema_version: u16,
    /// 每次内容真实变化时递增；前端据此丢弃乱序到达的事件。
    pub revision: u64,
    pub records: Vec<SessionRecord>,
}

impl Default for SessionRecordList {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            revision: 0,
            records: Vec::new(),
        }
    }
}

/// SQLite 连接 + 列表版本号，共用一把锁。
///
/// 二者同锁可一次锁定完成「写 + 读回 + 递增 revision」，天然避免
/// 「revision 已升高但 payload 还是旧的」被前端丢弃。
struct StoreInner {
    conn: Connection,
    revision: u64,
}

/// 跨线程共享的会话存档句柄。
///
/// `inner` 为 `None` 表示库彻底打不开（降级：读返回空、写返回 Err），不阻断启动。
#[derive(Clone)]
pub struct SessionStoreState {
    inner: Option<Arc<Mutex<StoreInner>>>,
    /// 回填缓存：key → 存档行。启动读入一次，之后由刷盘/删除/清空同步维护。
    ///
    /// 存在的理由不是性能而是功能：一个会话在本次运行中途从内存消失、稍后又带同一
    /// `session_id` 回来（`--resume`）时，若缓存已随会话清理而丢失就取不到 started/prompt。
    /// 因此刷盘必回写此表，remove/clear 必同步清它 —— 它是「库里现有什么」的内存镜像。
    seeds: Arc<Mutex<HashMap<String, Arc<SessionRecord>>>>,
    #[allow(dead_code)]
    path: PathBuf,
}

/// 一次 [`SessionStoreState::record_flush`] 的结果。
#[derive(Debug, Default, Clone)]
pub struct FlushOutcome {
    /// 真实写入（内容确有变化）的行数。生产路径只看 `should_broadcast`，该字段供测试
    /// 断言节流（第二次刷同一批会话必须为 0），故在非测试构建里未被读取。
    #[allow(dead_code)]
    pub written: usize,
    /// 库里此前没有、本次新出现的 key；monitor 据此广播事件让面板即时长出。
    pub new_keys: Vec<String>,
}

impl FlushOutcome {
    /// 是否有需要通知前端的变化（出现了新行）。
    pub fn should_broadcast(&self) -> bool {
        !self.new_keys.is_empty()
    }
}

impl SessionStoreState {
    /// 打开（或创建）存档库，读入回填缓存，并做一次 30 天过期清理。
    ///
    /// 容错对齐 `EndpointProfileState::load`（绝不让坏文件卡住启动），但 SQLite 坏法更多：
    /// 先原地试开 → 失败则把坏库改名 `<path>.broken-<ms>` 重建 → 再失败则降级为
    /// 「只读空存档」（写命令返回 Err）。全程不 panic、绝不让启动失败。
    pub fn load(path: PathBuf, now_ms: i64) -> Self {
        let state = match Self::open(&path) {
            Ok(state) => state,
            Err(error) => {
                eprintln!("session store unusable ({error}), moving {path:?} aside");
                move_aside(&path);
                match Self::open(&path) {
                    Ok(state) => state,
                    Err(error) => {
                        eprintln!("session store disabled ({error})");
                        // 降级：无连接，读空、写 Err。seeds 空表照常可用。
                        Self {
                            inner: None,
                            seeds: Arc::new(Mutex::new(HashMap::new())),
                            path,
                        }
                    }
                }
            }
        };
        // 启动期一次性过期清理 + 读入全表进缓存（顺序：先删过期，缓存里就不留陈旧行）。
        state.prune_expired(now_ms);
        state.reload_seeds();
        state
    }

    /// 打开连接并跑迁移；任一步失败即 Err（触发上层的挪开重建）。
    fn open(path: &PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
        let conn = Connection::open_with_flags(path, flags).map_err(|error| error.to_string())?;
        conn
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(|error| error.to_string())?;
        migrate(&conn).map_err(|error| error.to_string())?;
        Ok(Self {
            inner: Some(Arc::new(Mutex::new(StoreInner { conn, revision: 0 }))),
            seeds: Arc::new(Mutex::new(HashMap::new())),
            path: path.clone(),
        })
    }

    /// 存档列表（按 `last_seen_at_ms` 降序，最新活动在最前）。降级态 / 无连接返回空集合。
    pub fn list(&self) -> SessionRecordList {
        let inner = match self.inner.as_ref() {
            Some(inner) => inner,
            None => return SessionRecordList::default(),
        };
        let Ok(guard) = inner.lock() else {
            return SessionRecordList::default();
        };
        let records = select_all(&guard.conn);
        SessionRecordList {
            schema_version: SCHEMA_VERSION,
            revision: guard.revision,
            records,
        }
    }

    /// 回填摘要单键查询：O(1)、**不做任何 I/O**（读内存镜像，不碰连接）。
    ///
    /// **必须在 registry 锁之外调用**，返回值直接喂给 `apply_event_with_seed`。
    /// 这里做一次 `SessionRecord → StoredSeed` 的字段裁剪（clone 两个 Option<String>），
    /// 让存储类型不外泄给 reducer —— 事件频率很低，这点拷贝可忽略。
    pub fn seed_for(&self, key: &str) -> Option<StoredSeed> {
        let record = self.seeds.lock().ok()?.get(key).cloned()?;
        Some(StoredSeed {
            started_at_ms: record.started_at_ms,
            prompt: record.prompt.clone(),
            base_url: record.base_url.clone(),
            tokens: record.tokens.unwrap_or_default(),
        })
    }

    /// 批量刷入存活会话。**只写内容真的变化的行**（与缓存里的上一份对比），
    /// 因此 monitor 每 500ms 调一次也基本零 I/O（对齐 endpoint_profiles 的「无脑落盘会写爆」教训）。
    ///
    /// 顺序要紧：本函数应在 monitor 的 `cleanup` **之前**调用 —— 会话被清理前必须留档一次。
    /// 全程不持 registry 锁（输入是已克隆的 owned `Vec<SessionSnapshot>`）。
    pub fn record_flush(&self, sessions: &[SessionSnapshot]) -> FlushOutcome {
        let Some(inner) = self.inner.as_ref() else {
            return FlushOutcome::default();
        };

        // 1. 锁 inner：先扫脏行（锁内查 seeds，顺序恒为 inner → seeds），再开事务写。
        //    扫描放事务之前是为了让**空闲轮次完全不碰磁盘**（不必每 500ms 白开一次 BEGIN/ROLLBACK）。
        let (outcome, cache_delta) = {
            let Ok(mut guard) = inner.lock() else {
                return FlushOutcome::default();
            };

            // 待写脏行，连同「库里此前没有该 key」的标记（扫描时顺带算出，不再多查一次缓存）。
            let mut pending: Vec<(SessionRecord, bool)> = Vec::new();
            for session in sessions {
                let record = record_from_snapshot(session);
                // 脏行判定：与回填缓存里的旧值整体比较，缓存没有的 key 必然是脏行（且是新增）。
                // 逐键查而非克隆整表 —— 一轮最多「存活会话数」次查询（悬浮窗上限 4–8），
                // 而每 500ms 复制一份 ≤200 行的表纯属浪费。
                let previous = self
                    .seeds
                    .lock()
                    .ok()
                    .and_then(|seeds| seeds.get(&record.key).cloned());
                if previous.as_deref() != Some(&record) {
                    pending.push((record, previous.is_none()));
                }
            }
            if pending.is_empty() {
                return FlushOutcome::default();
            }

            let Ok(tx) = guard.conn.unchecked_transaction() else {
                eprintln!("session flush begin failed");
                return FlushOutcome::default();
            };

            let mut new_keys: Vec<String> = Vec::new();
            let mut updates: Vec<(String, Arc<SessionRecord>)> = Vec::new();
            for (record, is_fresh) in pending {
                if let Err(error) = upsert(&tx, &record) {
                    eprintln!("session flush upsert failed for {}: {error}", record.key);
                    continue;
                }
                if is_fresh {
                    new_keys.push(record.key.clone());
                }
                updates.push((record.key.clone(), Arc::new(record)));
            }

            if let Err(error) = tx.commit() {
                eprintln!("session flush commit failed: {error}");
                return FlushOutcome::default();
            }
            guard.revision = guard.revision.saturating_add(1);

            // 淘汰溢出：live = 本次输入的全量存活 key（豁免）。返回被删 key 以同步缓存。
            let live: Vec<&str> = sessions.iter().map(|session| session.key.as_str()).collect();
            let evicted = evict_overflow(&guard.conn, MAX_RECORDS, &live);

            // 缓存增量：先并入脏行新值，再剔除被淘汰的 key（保持「缓存 = 库内容」）。
            let written = updates.len();
            let mut delta: Vec<(String, Option<Arc<SessionRecord>>)> = updates
                .into_iter()
                .map(|(key, record)| (key, Some(record)))
                .collect();
            delta.extend(evicted.into_iter().map(|key| (key, None)));
            (FlushOutcome { written, new_keys }, delta)
        };

        // 2. 出 inner 锁后同步缓存。
        if let Ok(mut seeds) = self.seeds.lock() {
            for (key, record) in cache_delta {
                match record {
                    Some(record) => {
                        seeds.insert(key, record);
                    }
                    None => {
                        seeds.remove(&key);
                    }
                }
            }
        }

        outcome
    }

    /// 删一条。key 不存在 → Err（前端据此回滚 UI，与 endpoint_profiles 一致）。
    pub fn remove(&self, key: &str) -> Result<SessionRecordList, String> {
        let target = key.trim();
        if target.is_empty() {
            return Err("会话 key 为空".to_string());
        }
        let inner = self.inner.as_ref().ok_or("会话存档不可用")?;
        let deleted = {
            let mut guard = inner.lock().map_err(|_| "session store lock poisoned".to_string())?;
            let affected = guard
                .conn
                .execute("DELETE FROM session_records WHERE key = ?1", [target])
                .map_err(|error| error.to_string())?;
            if affected > 0 {
                guard.revision = guard.revision.saturating_add(1);
            }
            affected > 0
        };
        if deleted {
            if let Ok(mut seeds) = self.seeds.lock() {
                seeds.remove(target);
            }
            Ok(self.list())
        } else {
            Err("该会话未登记".to_string())
        }
    }

    /// 清空存档表 + 回填缓存。
    ///
    /// ⚠️ **不动内存里的存活会话**（需求决策）：因此仍在运行的会话会在下一次 `record_flush`
    /// 时把行重新写回来 —— 这是设计行为，不是 bug。
    pub fn clear_all(&self) -> Result<SessionRecordList, String> {
        let inner = self.inner.as_ref().ok_or("会话存档不可用")?;
        {
            let mut guard = inner.lock().map_err(|_| "session store lock poisoned".to_string())?;
            guard
                .conn
                .execute("DELETE FROM session_records", [])
                .map_err(|error| error.to_string())?;
            guard.revision = guard.revision.saturating_add(1);
        }
        if let Ok(mut seeds) = self.seeds.lock() {
            seeds.clear();
        }
        Ok(self.list())
    }

    /// 删除 `last_seen` 早于 `now_ms - MAX_RECORD_AGE_MS` 的行（启动期调用一次）。
    fn prune_expired(&self, now_ms: i64) {
        let cutoff = now_ms.saturating_sub(MAX_RECORD_AGE_MS);
        let inner = match self.inner.as_ref() {
            Some(inner) => inner,
            None => return,
        };
        if let Ok(mut guard) = inner.lock() {
            if let Ok(deleted) = guard
                .conn
                .execute("DELETE FROM session_records WHERE last_seen_at_ms < ?1", [cutoff])
            {
                if deleted > 0 {
                    guard.revision = guard.revision.saturating_add(1);
                }
            }
        }
    }

    /// 把全表读进回填缓存（启动清理之后调一次）。
    fn reload_seeds(&self) {
        let Some(inner) = self.inner.as_ref() else { return };
        let Ok(guard) = inner.lock() else { return };
        let fresh: HashMap<String, Arc<SessionRecord>> = select_all(&guard.conn)
            .into_iter()
            .map(|record| (record.key.clone(), Arc::new(record)))
            .collect();
        if let Ok(mut seeds) = self.seeds.lock() {
            *seeds = fresh;
        }
    }
}

/// 由存活快照折出一行存档。`tokens` 直接取快照里已合并的值（monitor 传的是带 token 的快照）。
fn record_from_snapshot(session: &SessionSnapshot) -> SessionRecord {
    SessionRecord {
        key: session.key.clone(),
        provider: session.provider,
        session_id: session.session_id.clone(),
        prompt: session.prompt.clone(),
        base_url: session.base_url.clone(),
        started_at_ms: session.started_at_ms,
        last_seen_at_ms: session.last_seen_at_ms,
        tokens: session.tokens,
    }
}

/// 建表 + 迁移。迁移策略保守：读不懂的库整表重建（存档是缓存，不是账本）。
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap_or(0);
    if version > SCHEMA_USER_VERSION {
        // 来自更新版本程序的库：不猜它的结构，重建。
        conn.execute_batch("DROP TABLE IF EXISTS session_records;")?;
    }
    create_tables(conn)?;
    // version==0 且表已建好、或列缺失（手改 / 早期版本）→ 已在 create_tables 用 IF NOT EXISTS 处理；
    // 这里只需确保 user_version 落定。
    conn.pragma_update(None, "user_version", SCHEMA_USER_VERSION)?;
    Ok(())
}

/// 建表与索引（幂等）。⚠️ `prompt` 明文长期驻留磁盘（原 inbox 文件只活约 500ms）。
fn create_tables(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS session_records (
            key                          TEXT    PRIMARY KEY NOT NULL,
            provider                     TEXT    NOT NULL,
            session_id                   TEXT    NOT NULL,
            prompt                       TEXT,
            base_url                     TEXT,
            started_at_ms                INTEGER NOT NULL,
            last_seen_at_ms              INTEGER NOT NULL,
            input_tokens                 INTEGER NOT NULL DEFAULT 0,
            cache_creation_input_tokens  INTEGER NOT NULL DEFAULT 0,
            cache_read_input_tokens      INTEGER NOT NULL DEFAULT 0,
            output_tokens                INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_session_records_last_seen
            ON session_records(last_seen_at_ms DESC);
        "#,
    )
}

/// 写一行（存在则更新）。除 `started_at_ms` 取 MIN 外一律用 `excluded.*` 覆盖 ——
/// 传进来的值已由 reducer 算完继承与 token 下限，SQL 里再算一遍会出现两套会漂移的合并规则。
/// `started_at_ms` 是「这轮对话最早的时刻」，任何情况都不该变晚，MIN 兜住写入路径顺序颠倒。
const UPSERT_SQL: &str = r#"
INSERT INTO session_records
    (key, provider, session_id, prompt, base_url, started_at_ms, last_seen_at_ms,
     input_tokens, cache_creation_input_tokens, cache_read_input_tokens, output_tokens)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
ON CONFLICT(key) DO UPDATE SET
    prompt = excluded.prompt,
    base_url = excluded.base_url,
    started_at_ms = MIN(excluded.started_at_ms, session_records.started_at_ms),
    last_seen_at_ms = excluded.last_seen_at_ms,
    input_tokens = excluded.input_tokens,
    cache_creation_input_tokens = excluded.cache_creation_input_tokens,
    cache_read_input_tokens = excluded.cache_read_input_tokens,
    output_tokens = excluded.output_tokens
"#;

/// 在事务里 UPSERT 一行。
fn upsert(tx: &rusqlite::Transaction<'_>, record: &SessionRecord) -> rusqlite::Result<()> {
    let tokens = record.tokens.unwrap_or_default();
    tx.execute(
        UPSERT_SQL,
        rusqlite::params![
            record.key,
            record.provider.as_str(),
            record.session_id,
            record.prompt,
            record.base_url,
            record.started_at_ms,
            record.last_seen_at_ms,
            tokens.input_tokens as i64,
            tokens.cache_creation_input_tokens as i64,
            tokens.cache_read_input_tokens as i64,
            tokens.output_tokens as i64,
        ],
    )?;
    Ok(())
}

/// 读全表，按 `last_seen_at_ms` 降序。provider 解析失败的行丢弃（手改 / 脏数据）。
fn select_all(conn: &Connection) -> Vec<SessionRecord> {
    let mut statement = match conn.prepare(
        "SELECT key, provider, session_id, prompt, base_url, started_at_ms, last_seen_at_ms,
                input_tokens, cache_creation_input_tokens, cache_read_input_tokens, output_tokens
         FROM session_records
         ORDER BY last_seen_at_ms DESC, key ASC",
    ) {
        Ok(statement) => statement,
        Err(error) => {
            eprintln!("session store select failed: {error}");
            return Vec::new();
        }
    };
    let rows = match statement.query_map([], read_row) {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("session store query failed: {error}");
            return Vec::new();
        }
    };
    rows.filter_map(Result::ok).collect()
}

/// 从一行结果集还原 `SessionRecord`。四项 token 全 0 时 `tokens` 记为 None。
fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRecord> {
    let provider_raw: String = row.get(1)?;
    let provider = Provider::parse(&provider_raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::other(format!("unknown provider {provider_raw}"))),
        )
    })?;
    let input = row.get::<_, i64>(7)?.max(0) as u64;
    let cache_write = row.get::<_, i64>(8)?.max(0) as u64;
    let cache_read = row.get::<_, i64>(9)?.max(0) as u64;
    let output = row.get::<_, i64>(10)?.max(0) as u64;
    let tokens = if input == 0 && cache_write == 0 && cache_read == 0 && output == 0 {
        None
    } else {
        Some(TokenTotals {
            input_tokens: input,
            cache_creation_input_tokens: cache_write,
            cache_read_input_tokens: cache_read,
            output_tokens: output,
        })
    };
    Ok(SessionRecord {
        key: row.get(0)?,
        provider,
        session_id: row.get(2)?,
        prompt: row.get(3)?,
        base_url: row.get(4)?,
        started_at_ms: row.get(5)?,
        last_seen_at_ms: row.get(6)?,
        tokens,
    })
}

/// 超过 `MAX_RECORDS` 时按 `last_seen` 升序淘汰最旧的行；`live`（当前存活会话）豁免。
///
/// 返回被删除的 key，供调用方同步回填缓存（缓存必须始终等于库内容，否则脏行判定与
/// `new_keys` 判定都会错）。行数量级 ≤200，「查全表 key + last_seen 再在 Rust 侧挑要删的」
/// 比写带 `IN(...)` 的 DELETE 更简单更不易错（rusqlite 无原生数组绑定）。
fn evict_overflow(conn: &Connection, max: usize, live: &[&str]) -> Vec<String> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM session_records", [], |row| row.get(0))
        .unwrap_or(0);
    let overflow = (count as usize).saturating_sub(max);
    if overflow == 0 {
        return Vec::new();
    }
    // 按由旧到新取候选，跳过存活会话，删够 overflow 行为止。
    let candidates: Vec<String> = match conn.prepare(
        "SELECT key FROM session_records ORDER BY last_seen_at_ms ASC, key ASC",
    ) {
        Ok(mut statement) => statement
            .query_map([], |row| row.get::<_, String>(0))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default(),
        Err(error) => {
            eprintln!("session store evict query failed: {error}");
            return Vec::new();
        }
    };
    let live_set: std::collections::HashSet<&str> = live.iter().copied().collect();
    let mut evicted = Vec::with_capacity(overflow);
    for key in candidates {
        if evicted.len() >= overflow {
            break;
        }
        if live_set.contains(key.as_str()) {
            continue;
        }
        match conn.execute("DELETE FROM session_records WHERE key = ?1", [&key]) {
            Ok(_) => evicted.push(key),
            Err(error) => eprintln!("session store evict failed for {key}: {error}"),
        }
    }
    evicted
}

/// 把坏库改名到 `<path>.broken-<时间戳>`（含同目录的 `-wal`/`-shm`/`-journal` 旁文件）。
/// 改名失败也继续（上层会退化为降级态）。
fn move_aside(path: &PathBuf) {
    let suffix = now_ms();
    for candidate in sidecar_paths(path) {
        if !candidate.exists() {
            continue;
        }
        let mut name = candidate
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "session-records".to_string());
        name.push_str(".broken-");
        name.push_str(&suffix.to_string());
        let target = candidate.with_file_name(name);
        let _ = std::fs::rename(&candidate, &target);
    }
}

/// 一个 SQLite 主文件对应的旁文件集合（主 + journal；本项目不开 WAL，故无 -wal/-shm，
/// 但为兼容手开 WAL 的情况一并挪走）。
fn sidecar_paths(path: &PathBuf) -> Vec<PathBuf> {
    let mut paths = vec![path.clone()];
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut name = path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();
        name.push_str(suffix);
        paths.push(path.with_file_name(name));
    }
    paths
}

/// 供 UI 展示的存档列表。
#[tauri::command]
pub fn get_session_records(state: State<'_, SessionStoreState>) -> SessionRecordList {
    state.list()
}

/// 删除一条会话存档（主窗口）。写库后广播，其它窗口即时同步。
#[tauri::command]
pub fn delete_session_record(
    app: AppHandle,
    state: State<'_, SessionStoreState>,
    key: String,
) -> Result<SessionRecordList, String> {
    let list = state.remove(&key)?;
    let _ = app.emit(SESSION_RECORDS_EVENT, &list);
    Ok(list)
}

/// 清空全部会话存档（主窗口）。⚠️ 只删归档、不动存活会话，运行中的会在下一个事件复现。
#[tauri::command]
pub fn clear_session_records(
    app: AppHandle,
    state: State<'_, SessionStoreState>,
) -> Result<SessionRecordList, String> {
    let list = state.clear_all()?;
    let _ = app.emit(SESSION_RECORDS_EVENT, &list);
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::test_support::TempDir;
    use crate::status::{SessionPhase, StatusSource};

    /// 构造一条最小可用的存活快照（只填存档会用到的字段）。
    fn snapshot(session_id: &str, started_at: i64, last_seen: i64) -> SessionSnapshot {
        SessionSnapshot {
            key: format!("claude:{session_id}"),
            provider: Provider::Claude,
            session_id: session_id.to_string(),
            phase: SessionPhase::Running,
            source: StatusSource::Hook,
            confidence: 100,
            started_at_ms: started_at,
            last_seen_at_ms: last_seen,
            completed_at_ms: None,
            remove_at_ms: None,
            is_flashing: false,
            pid: None,
            error_code: None,
            base_url: Some("https://api.example.com".to_string()),
            upstream_url: None,
            prompt: Some("修一下这个 bug".to_string()),
            tokens: Some(TokenTotals {
                input_tokens: 100,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 3_000,
                output_tokens: 50,
            }),
            last_sequence: None,
            termination_confirmed: false,
        }
    }

    /// 建一个带存档的临时状态；`TempDir` 必须留在元组里保活，
    /// 否则 Drop 会删掉刚写入的 db，后续重开读到的是不存在的文件。
    fn state(tag: &str) -> (TempDir, PathBuf, SessionStoreState) {
        let dir = TempDir::new(tag);
        let path = dir.path().join("session-records.db");
        let store = SessionStoreState::load(path.clone(), 1_000_000);
        (dir, path, store)
    }

    #[test]
    fn loads_empty_when_no_records_yet() {
        let (_dir, _path, store) = state("empty");
        let list = store.list();
        assert_eq!(list.schema_version, SCHEMA_VERSION);
        assert!(list.records.is_empty());
    }

    #[test]
    fn flush_then_reopen_roundtrips_records() {
        let (_dir, path, store) = state("roundtrip");
        let outcome = store.record_flush(&[snapshot("s1", 100, 200)]);
        assert_eq!(outcome.written, 1);
        assert_eq!(outcome.new_keys, vec!["claude:s1".to_string()]);

        // 真正落盘：重开同一份 db 能读回来，字段逐一对上。
        let reopened = SessionStoreState::load(path, 300);
        let records = reopened.list().records;
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.key, "claude:s1");
        assert_eq!(record.provider, Provider::Claude);
        assert_eq!(record.started_at_ms, 100);
        assert_eq!(record.last_seen_at_ms, 200);
        assert_eq!(record.prompt.as_deref(), Some("修一下这个 bug"));
        let tokens = record.tokens.expect("tokens persisted");
        assert_eq!(tokens.input_tokens, 100);
        assert_eq!(tokens.cache_read_input_tokens, 3_000);
    }

    #[test]
    fn second_flush_of_identical_batch_writes_nothing() {
        // 节流核心：同一批存活会话每 500ms 都进来，内容没变就不该再碰磁盘。
        let (_dir, _path, store) = state("throttle");
        let sessions = [snapshot("s1", 100, 200)];
        assert_eq!(store.record_flush(&sessions).written, 1);
        assert_eq!(store.record_flush(&sessions).written, 0);
        // 只有 last_seen 变化才算脏行。
        let mut moved = snapshot("s1", 100, 200);
        moved.last_seen_at_ms = 400;
        assert_eq!(store.record_flush(&[moved]).written, 1);
    }

    #[test]
    fn flush_skips_rows_that_were_removed_from_the_store() {
        // remove 会同步清缓存 → 同一会话再被刷入时按「新行」重新登记。
        let (_dir, _path, store) = state("resurrect");
        store.record_flush(&[snapshot("s1", 100, 200)]);
        store.remove("claude:s1").expect("remove");
        assert!(store.seed_for("claude:s1").is_none());
        let outcome = store.record_flush(&[snapshot("s1", 100, 200)]);
        assert_eq!(outcome.new_keys, vec!["claude:s1".to_string()]);
    }

    #[test]
    fn eviction_drops_oldest_and_spares_live_sessions() {
        let (_dir, _path, store) = state("evict");
        // 先把库存灌满到上限：一批 MAX 个历史行（last_seen=index，old0 最旧）。
        let fill: Vec<SessionSnapshot> = (0..MAX_RECORDS)
            .map(|index| snapshot(&format!("old{index}"), index as i64, index as i64))
            .collect();
        store.record_flush(&fill);
        assert_eq!(store.list().records.len(), MAX_RECORDS, "刚到上限不淘汰");

        // 再刷一批「此刻存活」的会话：1 个极旧但存活 + 5 个新的，把总量顶到 MAX+6。
        // 本批成员都是 live（豁免淘汰）；应淘汰库存里最旧的 6 个**不存活**行 old0..old5。
        let mut ancient_live = snapshot("aaa-ancient", 1, 0); // last_seen=0 + key 排在 old0 前
        ancient_live.last_seen_at_ms = 0;
        let fresh: Vec<SessionSnapshot> = (1..=5)
            .map(|index| snapshot(&format!("new{index}"), 1_000 + index, 1_000 + index))
            .collect();
        let mut batch = vec![ancient_live];
        batch.extend(fresh);
        store.record_flush(&batch);

        let records = store.list();
        assert_eq!(records.records.len(), MAX_RECORDS, "超出部分被淘汰");
        assert!(
            records
                .records
                .iter()
                .any(|record| record.key == "claude:aaa-ancient"),
            "存活会话不得被淘汰，哪怕它按 last_seen/key 排最旧"
        );
        assert!(
            !records.records.iter().any(|record| record.key == "claude:old0"),
            "最旧的不存活行应首先被腾出去"
        );
    }

    #[test]
    fn startup_prune_removes_records_older_than_the_window() {
        let (dir, path, store) = state("prune");
        // 一条远超 30 天的行。
        let ancient = snapshot("ancient", 1, 1);
        store.record_flush(&[ancient]);
        assert_eq!(store.list().records.len(), 1);
        // 用「30 天 + 1 秒之后」的时刻重开：启动清理必须把它删掉。
        let later = 1 + MAX_RECORD_AGE_MS + 1_000;
        let reopened = SessionStoreState::load(path, later);
        assert!(reopened.list().records.is_empty());
        // 回填缓存同步为空（不能留幽灵 seed）。
        assert!(reopened.seed_for("claude:ancient").is_none());
        let _ = dir;
    }

    #[test]
    fn remove_rejects_unknown_key_and_returns_the_rest() {
        let (_dir, _path, store) = state("remove");
        store.record_flush(&[snapshot("a", 1, 1), snapshot("b", 2, 2)]);
        assert!(store.remove("claude:nope").is_err());
        let list = store.remove("claude:a").expect("remove a");
        assert_eq!(list.records.len(), 1);
        assert_eq!(list.records[0].key, "claude:b");
        assert!(store.seed_for("claude:a").is_none());
        assert!(store.seed_for("claude:b").is_some());
    }

    #[test]
    fn clear_empties_store_and_seeds_but_live_rows_return() {
        let (_dir, _path, store) = state("clear");
        let live = snapshot("s1", 100, 200);
        store.record_flush(std::slice::from_ref(&live));
        let list = store.clear_all().expect("clear");
        assert!(list.records.is_empty());
        assert!(store.seed_for("claude:s1").is_none());
        // ⚠️ 需求决策的体现：清空不动内存，存活会话下一轮刷盘会把行写回来。
        let outcome = store.record_flush(std::slice::from_ref(&live));
        assert_eq!(outcome.new_keys, vec!["claude:s1".to_string()]);
        assert_eq!(store.list().records.len(), 1);
    }

    #[test]
    fn revision_advances_only_on_content_changes() {
        let (_dir, _path, store) = state("revision");
        let sessions = [snapshot("s1", 100, 200)];
        assert_eq!(store.record_flush(&sessions).written, 1);
        let after_first = store.list().revision;
        assert_eq!(store.record_flush(&sessions).written, 0);
        assert_eq!(store.list().revision, after_first, "无变化不得推进 revision");
    }

    #[test]
    fn seed_lookup_exposes_only_backfill_fields() {
        let (_dir, _path, store) = state("seed");
        store.record_flush(&[snapshot("s1", 100, 200)]);
        let seed = store.seed_for("claude:s1").expect("seed present");
        assert_eq!(seed.started_at_ms, 100);
        assert_eq!(seed.prompt.as_deref(), Some("修一下这个 bug"));
        assert_eq!(seed.base_url.as_deref(), Some("https://api.example.com"));
        assert_eq!(seed.tokens.input_tokens, 100);
        assert!(store.seed_for("claude:missing").is_none());
    }

    #[test]
    fn started_at_never_moves_later_across_flushes() {
        // UPSERT 的 MIN(started) 防御：两条写入路径顺序颠倒也不该把创建时刻推晚。
        let (_dir, path, store) = state("min-started");
        store.record_flush(&[snapshot("s1", 500, 600)]);
        let mut older = snapshot("s1", 100, 700);
        older.last_seen_at_ms = 700;
        store.record_flush(std::slice::from_ref(&older));
        let reopened = SessionStoreState::load(path, 700);
        assert_eq!(reopened.list().records[0].started_at_ms, 100);
    }

    #[test]
    fn junk_file_is_moved_aside_and_rebuilt() {
        // 不是 SQLite 文件（手改 / 残留）：load 必须挪开它重建空库，而不是卡死启动。
        let dir = TempDir::new("junk");
        let path = dir.path().join("session-records.db");
        std::fs::write(&path, b"definitely not a sqlite database").expect("write junk");
        let store = SessionStoreState::load(path.clone(), 1_000_000);
        assert!(store.list().records.is_empty());
        // 坏文件被改名保留（含 .broken- 后缀），不是原地删掉。
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read dir")
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .collect();
        assert!(
            leftovers.iter().any(|name| name.contains(".broken-")),
            "坏库应被挪开而非丢弃：{leftovers:?}"
        );
        // 新库可用。
        store.record_flush(&[snapshot("s1", 1, 1)]);
        assert_eq!(store.list().records.len(), 1);
    }

    #[test]
    fn occupied_path_is_moved_aside_and_recovers() {
        // 目标路径被别的东西占住（这里用目录模拟）：第一次打不开 → 挪开 → 第二次建成真库。
        // 自愈优先于降级；两条路径都必须「不 panic、启动不被卡住」。
        let dir = TempDir::new("occupied");
        let path = dir.path().join("session-records.db");
        std::fs::create_dir(&path).expect("occupy path with a directory");
        let store = SessionStoreState::load(path.clone(), 1_000);
        // 挪开占位后重建成功 → 可写可读。
        store.record_flush(&[snapshot("s1", 1, 1)]);
        assert_eq!(store.list().records.len(), 1);
        // 原占位以 .broken- 之名保留在目录里，而不是被静默删除。
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read dir")
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .collect();
        assert!(
            leftovers.iter().any(|name| name.contains(".broken-")),
            "占位物应被挪开而非丢弃：{leftovers:?}"
        );
    }

    #[test]
    fn disabled_store_degrades_without_panicking() {
        // 直接构造降级态（inner: None）：读返回空、写返回 Err、刷盘零副作用。
        let store = SessionStoreState {
            inner: None,
            seeds: Arc::new(Mutex::new(HashMap::new())),
            path: PathBuf::from("nowhere/session-records.db"),
        };
        assert!(store.list().records.is_empty());
        assert!(store.clear_all().is_err());
        assert!(store.remove("claude:s1").is_err());
        assert_eq!(store.record_flush(&[snapshot("s1", 1, 1)]).written, 0);
        assert!(store.seed_for("claude:s1").is_none());
    }
}
