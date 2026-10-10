//! 组件包的持久化存储：`host.storage` 的后端（需求 6 里的「持久化存储」）。
//!
//! 形状：一个包一个 JSON 文件（`<app-data>/plugin-store/<id>.json`），内容就是「键 → 值」一张表。
//! 之所以不给它一个真数据库：包能存的东西藏量本来就被配额卡死（8 MiB），
//! 一张能 `cat` 出来的 JSON 反而更好排查，卸载也是删一个文件。
//!
//! 三条口径：
//! 1. **按包隔离**：路径只由已校验过的 id 决定，包 A 拿不到包 B 的分区（命令层也不接受自定义路径）；
//! 2. **配额报错不静默淘汰**：超限一律 `Err`，让作者当场看见，而不是「昨天还在的数据今天悄悄没了」；
//! 3. **值就是 JSON**：宿主不解释内容，只保证存得下、取得回、卸得掉。
//!
//! ⚠️ `panic = "abort"`：这里不 `unwrap` / 不索引 / 不 slice，读盘失败一律变 `Err(String)`。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use serde_json::Value;

use super::paths;

/// app-data 下存放各包分区的目录名。
pub const STORE_DIR: &str = "plugin-store";

/// 某个包的占用读数（管理区详情用：存了多少、还剩多少）。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    /// 分区文件当前字节数。
    pub bytes: u64,
    /// 分区上限。
    pub limit: u64,
    /// 键数。
    pub keys: usize,
    /// 键数上限。
    pub key_limit: usize,
}

/// 运行态。
#[derive(Clone, Default)]
pub struct StorageState(Arc<Mutex<Inner>>);

#[derive(Debug, Default)]
struct Inner {
    /// app-data 基准。
    app_data: PathBuf,
}

impl StorageState {
    /// 建运行态。
    pub fn new(app_data: PathBuf) -> Self {
        Self(Arc::new(Mutex::new(Inner { app_data })))
    }

    /// 读一个键；没有这个键时返回 `None`（不是错误）。
    pub fn get(&self, id: &str, key: &str) -> Result<Option<Value>, String> {
        let table = self.read(id)?;
        Ok(table.get(key).cloned())
    }

    /// 列出这个包的所有键（按字典序，BTreeMap 天然有序）。
    pub fn keys(&self, id: &str) -> Result<Vec<String>, String> {
        Ok(self.read(id)?.keys().cloned().collect())
    }

    /// 写一个键：逐条查配额，超了就报错并说清撞到的是哪一条。
    pub fn set(&self, id: &str, key: &str, value: Value) -> Result<(), String> {
        check_key(key)?;
        let encoded = serde_json::to_string(&value).map_err(|error| format!("值不是可序列化的 JSON：{error}"))?;
        if encoded.len() as u64 > paths::STORAGE_VALUE_BYTES {
            return Err(format!(
                "单个值的上限是 {} 字节，这个值 {} 字节；大的东西别塞进 storage",
                paths::STORAGE_VALUE_BYTES,
                encoded.len()
            ));
        }
        let mut table = self.read(id)?;
        let fresh = !table.contains_key(key);
        if fresh && table.len() + 1 > paths::STORAGE_MAX_KEYS {
            return Err(format!(
                "键数上限是 {}，这个包已经用满了；先 remove 一些再写",
                paths::STORAGE_MAX_KEYS
            ));
        }
        table.insert(key.to_string(), value);
        // 分区上限在 `write` 里查（先算长度、不超限才落盘）：所以「超限」等于「这次什么都没写」，
        // 不需要回滚，也不会留下半截文件。
        self.write(id, &table)
    }

    /// 删一个键；不存在时返回 false（不算错误）。
    pub fn remove(&self, id: &str, key: &str) -> Result<bool, String> {
        check_key(key)?;
        let mut table = self.read(id)?;
        let removed = table.remove(key).is_some();
        if removed {
            self.write(id, &table)?;
        }
        Ok(removed)
    }

    /// 这个包的占用读数。
    pub fn usage(&self, id: &str) -> Result<StorageUsage, String> {
        let app_data = self.app_data();
        Ok(self.usage_of(&app_data, id))
    }

    /// 卸载时删掉整个分区（连带它的键）。
    pub fn forget(&self, id: &str) {
        if !paths::is_valid_plugin_id(id) {
            return;
        }
        let file = partition_path(&self.app_data(), id);
        let _ = fs::remove_file(&file);
    }

    /// 读整张表：文件不存在 = 空表；内容坏掉 = 报错（不许静默当空的，那等于用户的记忆没了）。
    fn read(&self, id: &str) -> Result<BTreeMap<String, Value>, String> {
        if !paths::is_valid_plugin_id(id) {
            return Err(format!("id「{id}」不合法"));
        }
        let file = partition_path(&self.app_data(), id);
        if !file.exists() {
            return Ok(BTreeMap::new());
        }
        let text = fs::read_to_string(&file).map_err(|error| format!("读不出这个包的存储：{error}"))?;
        if text.trim().is_empty() {
            return Ok(BTreeMap::new());
        }
        serde_json::from_str(&text).map_err(|error| format!("这个包的存储文件已损坏：{error}"))
    }

    /// 写整张表（临时文件 + 改名，避免半截 JSON）。
    fn write(&self, id: &str, table: &BTreeMap<String, Value>) -> Result<(), String> {
        let app_data = self.app_data();
        let dir = store_dir(&app_data);
        fs::create_dir_all(&dir).map_err(|error| format!("建不出存储目录：{error}"))?;
        let file = partition_path(&app_data, id);
        let temp = file.with_extension("json.tmp");
        let text = serde_json::to_string(table).map_err(|error| format!("存不下：{error}"))?;
        if text.len() as u64 > paths::STORAGE_PARTITION_BYTES {
            return Err(format!(
                "这个包的存储上限是 {} 字节，现在要写 {} 字节",
                paths::STORAGE_PARTITION_BYTES,
                text.len()
            ));
        }
        fs::write(&temp, text).map_err(|error| format!("写不出：{error}"))?;
        fs::rename(&temp, &file).map_err(|error| format!("放不进：{error}"))
    }

    /// 占用读数（不经过 `read`，损坏的文件也要能报出字节数）。
    fn usage_of(&self, app_data: &Path, id: &str) -> StorageUsage {
        let file = partition_path(app_data, id);
        let bytes = fs::metadata(&file).map(|meta| meta.len()).unwrap_or(0);
        let keys = fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<BTreeMap<String, Value>>(&text).ok())
            .map(|table| table.len())
            .unwrap_or(0);
        StorageUsage {
            bytes,
            limit: paths::STORAGE_PARTITION_BYTES,
            keys,
            key_limit: paths::STORAGE_MAX_KEYS,
        }
    }

    /// 取 app-data 基准（锁外也要用，所以单独拿出来）。
    fn app_data(&self) -> PathBuf {
        lock(&self.0).app_data.clone()
    }
}

/// 键名规则：非空、长度受限、不许控制字符与路径分隔（它将来可能被拿去拼文件名）。
fn check_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("键不能是空串".to_string());
    }
    if key.chars().count() > paths::STORAGE_MAX_KEY_CHARS {
        return Err(format!(
            "键名最长 {} 字符，这个 {} 字符",
            paths::STORAGE_MAX_KEY_CHARS,
            key.chars().count()
        ));
    }
    if key.contains('/') || key.contains('\\') || key.chars().any(|c| c.is_control()) {
        return Err("键名不能含路径分隔符或控制字符".to_string());
    }
    Ok(())
}

/// 存放各包分区的目录。
fn store_dir(app_data: &Path) -> PathBuf {
    app_data.join(STORE_DIR)
}

/// 某个包的分区文件。
fn partition_path(app_data: &Path, id: &str) -> PathBuf {
    store_dir(app_data).join(format!("{id}.json"))
}

/// 取一把锁（中毒也照用）。
fn lock<T>(value: &Arc<Mutex<T>>) -> MutexGuard<'_, T> {
    match value.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 隔离的临时 app-data。
    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ufo-plugin-store-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("临时目录建不出来");
        dir
    }

    #[test]
    fn round_trip_keeps_json_types() {
        let root = temp_root("round");
        let store = StorageState::new(root.clone());
        store.set("p", "cfg", json!({ "n": 3, "ok": true })).expect("写得进");
        let back = store.get("p", "cfg").expect("读得出").expect("存在");
        assert_eq!(back["n"].as_i64(), Some(3));
        assert_eq!(back["ok"].as_bool(), Some(true));
        // 没写过的键是「没有」，不是错误。
        assert!(store.get("p", "别的").expect("读得动").is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn packages_are_isolated_by_id() {
        let root = temp_root("isolate");
        let store = StorageState::new(root.clone());
        store.set("a", "k", json!(1)).expect("a 写得进");
        store.set("b", "k", json!(2)).expect("b 写得进");
        assert_eq!(store.get("a", "k").unwrap().unwrap(), json!(1));
        assert_eq!(store.get("b", "k").unwrap().unwrap(), json!(2));
        assert_eq!(store.keys("a").unwrap(), vec!["k".to_string()]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bad_ids_and_keys_are_refused_before_touching_the_disk() {
        let root = temp_root("keys");
        let store = StorageState::new(root.clone());
        assert!(store.set("Bad Id", "k", json!(1)).is_err());
        assert!(store.set("p", "", json!(1)).is_err());
        assert!(store.set("p", "../escape", json!(1)).is_err());
        assert!(store.set("p", "a/b", json!(1)).is_err());
        assert!(store.set("p", "控\t制", json!(1)).is_err());
        assert!(store.set("p", &"k".repeat(paths::STORAGE_MAX_KEY_CHARS + 1), json!(1)).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn oversized_value_is_refused_with_the_number_that_hit_the_wall() {
        let root = temp_root("value");
        let store = StorageState::new(root.clone());
        let big = "字".repeat((paths::STORAGE_VALUE_BYTES as usize / 3) + 10);
        let error = store.set("p", "k", json!(big)).expect_err("超单值上限必须拒");
        assert!(error.contains("单个值的上限"), "{error}");
        assert!(store.get("p", "k").unwrap().is_none(), "被拒的写入不许留半截");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn key_count_cap_is_enforced_and_rewriting_an_existing_key_is_free() {
        let root = temp_root("keys2");
        let store = StorageState::new(root.clone());
        for index in 0..paths::STORAGE_MAX_KEYS {
            store.set("p", &format!("k{index}"), json!(1)).expect("写满之前都该成功");
        }
        // 覆盖已有键不算新增。
        store.set("p", "k0", json!(2)).expect("覆盖已有的键不该被键数挡住");
        let error = store.set("p", "one-more", json!(1)).expect_err("再加一个就超了");
        assert!(error.contains("键数上限"), "{error}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn partition_cap_rolls_back_the_write() {
        let root = temp_root("cap");
        let store = StorageState::new(root.clone());
        // 直接造一个已经很大的分区文件（绕过单值上限：分几次写满）。
        let chunk = "x".repeat(paths::STORAGE_VALUE_BYTES as usize - 64);
        let rounds = (paths::STORAGE_PARTITION_BYTES as usize / chunk.len()) + 2;
        let mut overflowed = None;
        for index in 0..rounds {
            if let Err(error) = store.set("p", &format!("k{index}"), json!(chunk.clone())) {
                overflowed = Some(error);
                break;
            }
        }
        let error = overflowed.expect("写满分区必须被拒");
        assert!(error.contains("这个包的存储上限"), "{error}");
        let usage = store.usage("p").expect("读数拿得到");
        assert!(
            usage.bytes <= paths::STORAGE_PARTITION_BYTES,
            "回滚后不该留下超限的文件：{} 字节",
            usage.bytes
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_reports_whether_something_was_deleted() {
        let root = temp_root("remove");
        let store = StorageState::new(root.clone());
        store.set("p", "k", json!(1)).expect("写得进");
        assert!(store.remove("p", "k").expect("删得掉"));
        assert!(!store.remove("p", "k").expect("再删不报错"), "第二次是「没有这个东西」");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn forget_drops_the_partition() {
        let root = temp_root("forget");
        let store = StorageState::new(root.clone());
        store.set("p", "k", json!(1)).expect("写得进");
        assert!(partition_path(&root, "p").exists());
        store.forget("p");
        assert!(!partition_path(&root, "p").exists());
        assert_eq!(store.usage("p").expect("读数").bytes, 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_corrupt_partition_says_so_instead_of_looking_empty() {
        // 静默当空的后果是「用户的记忆没了但界面说不出来」，所以这条必须报。
        let root = temp_root("corrupt");
        let store = StorageState::new(root.clone());
        fs::create_dir_all(store_dir(&root)).expect("目录建不出来");
        fs::write(partition_path(&root, "p"), "{坏文件").expect("写不出坏文件");
        let error = store.get("p", "k").expect_err("损坏要报错");
        assert!(error.contains("已损坏"), "{error}");
        let _ = fs::remove_dir_all(&root);
    }
}
