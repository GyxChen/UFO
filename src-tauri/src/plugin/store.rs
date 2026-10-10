//! 台账（ledger）：装了哪些包、从哪来、是不是在调试。
//!
//! 三条不变量，各有对应单测：
//! 1. **本地源就地装载** —— 只校验、不复制（作者改完存盘就能看见，不需要任何链接步骤）。
//! 2. **`devReload` 为真 ⇒ 必须是本地源** —— 在 `upsert` 与 `scan_records` 两个写入口各夹一次；
//!    少一处就会出现「本地装 + 开开关 → 覆盖安装同 id 的远程包」后远程包被自动重扫。
//! 3. **坏包也要产出记录** —— 校验失败不静默消失，管理区那一行要能说出「为什么没挂上」。
//!
//! ⚠️ 台账**不管启用状态**：插件与内置组件在悬浮窗里同级，「开不开」只有一个写入方 ——
//! 外壳的 `floating-modules.json`（`modules[].enabled`）。这里再存一份就是两个开关。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{manifest, paths};

/// 台账文件的结构版本（库内布局，与线协议 `SCHEMA_VERSION` 无关，可以递增）。
pub const LEDGER_SCHEMA_VERSION: u32 = 1;

/// 来源：本地目录（就地装载）。
pub const SOURCE_LOCAL: &str = "local";
/// 来源：已拷进 app-data（zip / npm 安装走这条）。
pub const SOURCE_COPIED: &str = "copied";

/// 台账里的一条记录（只存「装载所需的最小信息」，清单内容每次从盘上读）。
/// 没有「启用」字段，理由见模块头。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LedgerEntry {
    /// 包 id（安装时已校验目录名与清单一致）。
    pub id: String,
    /// `local` / `copied`。
    pub source: String,
    /// 包目录的绝对路径。
    pub dir: String,
    /// 逐包热重载开关（⚠️ 为真时必须是本地源）。
    pub dev_reload: bool,
}

impl Default for LedgerEntry {
    /// 新条目默认「本地源 + 不调试」。
    fn default() -> Self {
        Self {
            id: String::new(),
            source: SOURCE_LOCAL.to_string(),
            dir: String::new(),
            dev_reload: false,
        }
    }
}

/// 台账文件本体。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Ledger {
    /// 台账结构版本。
    pub schema_version: u32,
    /// 每次写盘 +1：前端用它丢弃迟到的广播。
    pub revision: u64,
    /// 已装包列表（顺序 = 安装顺序）。
    pub entries: Vec<LedgerEntry>,
}

impl Default for Ledger {
    /// 空台账。
    fn default() -> Self {
        Self {
            schema_version: LEDGER_SCHEMA_VERSION,
            revision: 0,
            entries: Vec::new(),
        }
    }
}

/// 一条记录的完整视图：台账 + 清单 + 盘上核算 + 失败原因。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRecord {
    /// 包 id。
    pub id: String,
    /// 展示名：清单有效时取清单，坏包从原始 JSON 尽力读，读不到用目录名。
    pub name: String,
    /// 展示版本号（尽力而为）。
    pub version: String,
    /// 校验并夹取后的清单；`None` = 校验失败，这条挂不上。
    pub manifest: Option<manifest::Manifest>,
    /// 包目录绝对路径。
    pub dir: String,
    /// `local` / `copied`。
    pub source: String,
    /// 热重载开关（已按「本地源」夹过）。
    pub dev_reload: bool,
    /// 校验夹取时的告警（详情区逐条显示）。
    pub warnings: Vec<String>,
    /// 包内文件数。
    pub files: usize,
    /// 包总体积（字节）。
    pub bytes: u64,
    /// 上次装载耗时（前端上报的运行时读数）。
    pub load_ms: u64,
    /// 校验失败的原因（中文，直接渲染在列表行）。
    pub error: Option<String>,
}

/// 一次盘上核算的结果。
#[derive(Clone, Debug, Default)]
struct PackageStat {
    files: usize,
    bytes: u64,
    /// 文件数超过 `MAX_FILES`：不再数下去，但要在界面上说清「这里被截了」。
    overflowed: bool,
    /// 总体积超过 `MAX_TOTAL_BYTES`：同样停下，这条包不挂。
    too_big: bool,
}

/// 台账状态：命令层通过它读写。
#[derive(Clone, Default)]
pub struct StoreState(Arc<Mutex<Inner>>);

/// 互斥体里的真身。
#[derive(Debug, Default)]
struct Inner {
    /// app-data 目录（所有包的落点基准）。
    app_data: PathBuf,
    /// 台账。
    ledger: Ledger,
    /// 前端上报的装载耗时（不进台账：它是运行时读数，不是安装事实）。
    load_ms: BTreeMap<String, u64>,
}

impl StoreState {
    /// 读一次台账：文件不存在 = 空台账；解析失败也当空台账，但把原因留在 stderr。
    pub fn load(app_data: PathBuf) -> Self {
        let path = paths::ledger_path(&app_data);
        let ledger = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Ledger>(&text).unwrap_or_else(|error| {
                eprintln!("plugin ledger is unreadable ({error}), starting empty");
                Ledger::default()
            }),
            Err(_) => Ledger::default(),
        };
        Self(Arc::new(Mutex::new(Inner {
            app_data,
            ledger,
            load_ms: BTreeMap::new(),
        })))
    }

    /// 扫描全部已装包 → 管理区与装载链共用的记录表（顺序 = 台账顺序）。
    pub fn scan_records(&self) -> Vec<PluginRecord> {
        let ledger = { lock(&self.0).ledger.clone() };
        ledger
            .entries
            .iter()
            .map(|entry| {
                let dir = PathBuf::from(&entry.dir);
                let read = read_manifest(&dir);
                let stat = stat_package(&dir);
                let mut error = read.error;
                // ⚠️ 超上限 = 不挂，不是「挂上并附一行说明」：那两个数在作者文档里是承诺，
                // 只报错不拦就是没有上限 —— 而冷扫描每读一次都要付 IO 的代价。
                // 清单本身就坏时保留那条原因，它比体积读数有用。
                let mut validated = read.validated;
                if error.is_none() {
                    if stat.overflowed {
                        error = Some(format!("文件数超过 {} 的上限，体积读数不完整", paths::MAX_FILES));
                        validated = None;
                    } else if stat.too_big {
                        error = Some(format!(
                            "总体积超过 {} MiB 的上限",
                            paths::MAX_TOTAL_BYTES / (1024 * 1024)
                        ));
                        validated = None;
                    }
                }
                PluginRecord {
                    id: entry.id.clone(),
                    // 第二个写入口：读取时也夹一次，历史台账里的「远程源 + dev_reload」不得进界面。
                    dev_reload: entry.dev_reload && entry.source == SOURCE_LOCAL,
                    name: read.name,
                    version: read.version,
                    manifest: validated.map(|done| done.manifest),
                    dir: entry.dir.clone(),
                    source: entry.source.clone(),
                    warnings: read.warnings,
                    files: stat.files,
                    bytes: stat.bytes,
                    load_ms: self.load_ms_of(&entry.id),
                    error,
                }
            })
            .collect()
    }

    /// 只看某个包的记录。
    pub fn record_of(&self, id: &str) -> Option<PluginRecord> {
        self.scan_records().into_iter().find(|record| record.id == id)
    }

    /// 校验一个目录能不能装，并返回将要落台账的内容（**不写盘**）。
    ///
    /// preview / install 分两步是为了让界面先把逐条披露摊给用户看；
    /// 两步各自读盘，所以 install 会重新校验一遍 —— 用户点的「装」必须是对当下这份字节的同意。
    pub fn preview(&self, dir: &str) -> Result<PluginRecord, String> {
        let path = PathBuf::from(dir);
        if !path.is_dir() {
            return Err("选中的不是目录：插件包是一个含 plugin.json 的目录".to_string());
        }
        let read = read_manifest(&path);
        let done = match read.validated {
            Some(done) => done,
            None => {
                return Err(format!(
                    "清单校验失败：{}",
                    read.error.unwrap_or_else(|| "读不到 plugin.json".to_string())
                ))
            }
        };
        check_identity(&path, &done.manifest.id)?;
        let stat = stat_package(&path);
        // 装之前就卡住：越过这两条线就不该落台账（`install_local` 走的也是本函数）。
        if stat.overflowed {
            return Err(format!("文件数超过 {} 的上限", paths::MAX_FILES));
        }
        if stat.too_big {
            return Err(format!(
                "总体积超过 {} MiB 的上限",
                paths::MAX_TOTAL_BYTES / (1024 * 1024)
            ));
        }
        // 入口单独量一次体积：协议侧本来就按类型拒呈递（`size_limit_of`），
        // 但那要等到装载才暴露 —— 症状是「import 失败」，作者先怀疑自己路径写错。
        let entry = done.manifest.entry.clone();
        let entry_limit = paths::size_limit_of(&entry);
        let entry_bytes = fs::metadata(path.join(&entry)).map(|meta| meta.len()).unwrap_or(0);
        if entry_bytes > entry_limit {
            return Err(format!(
                "入口「{entry}」{} MiB 超过 {} MiB 上限，宿主不会呈递它",
                entry_bytes / (1024 * 1024),
                entry_limit / (1024 * 1024)
            ));
        }
        Ok(PluginRecord {
            id: done.manifest.id.clone(),
            name: read.name,
            version: read.version,
            manifest: Some(done.manifest),
            dir: abs(&path),
            source: SOURCE_LOCAL.to_string(),
            dev_reload: false,
            warnings: done.warnings,
            files: stat.files,
            bytes: stat.bytes,
            load_ms: 0,
            error: None,
        })
    }

    /// 本地目录安装：校验通过后只加一条台账（就地装载，不复制文件）。
    pub fn install_local(&self, dir: &str) -> Result<PluginRecord, String> {
        let record = self.preview(dir)?;
        let mut guard = lock(&self.0);
        upsert(
            &mut guard.ledger,
            LedgerEntry {
                id: record.id.clone(),
                source: SOURCE_LOCAL.to_string(),
                dir: record.dir.clone(),
                dev_reload: false,
            },
        );
        guard.ledger.revision += 1;
        save(&guard.app_data, &guard.ledger)?;
        Ok(record)
    }

    /// 卸载：删台账条目。
    ///
    /// ⚠️ `local` 源**不动用户的目录**（那是作者自己的工作区，删掉等于删源码）；
    /// 只有 `copied` 源才删 app-data 下那份副本，且只删 `plugins/` 里面那一层。
    pub fn uninstall(&self, id: &str) -> Result<(), String> {
        if !paths::is_valid_plugin_id(id) {
            return Err(format!("id「{id}」不合法"));
        }
        let mut guard = lock(&self.0);
        let copied: Vec<PathBuf> = guard
            .ledger
            .entries
            .iter()
            .filter(|entry| entry.id == id && entry.source == SOURCE_COPIED)
            .map(|entry| PathBuf::from(&entry.dir))
            .collect();
        let before = guard.ledger.entries.len();
        guard.ledger.entries.retain(|entry| entry.id != id);
        if guard.ledger.entries.len() == before {
            return Err(format!("没有 id 为「{id}」的已装组件"));
        }
        guard.ledger.revision += 1;
        guard.load_ms.remove(id);
        let app_data = guard.app_data.clone();
        save(&app_data, &guard.ledger)?;
        drop(guard);
        let root = paths::plugins_root(&app_data);
        for target in copied {
            if !target.starts_with(&root) {
                continue;
            }
            if let Err(error) = fs::remove_dir_all(&target) {
                eprintln!("failed to remove copied plugin dir: {error}");
            }
        }
        Ok(())
    }

    /// 开 / 关逐包热重载（第一个写入口：非本地源要开就直接拒绝）。
    pub fn set_dev_reload(&self, id: &str, dev_reload: bool) -> Result<u64, String> {
        self.edit(id, |entry| {
            if dev_reload && entry.source != SOURCE_LOCAL {
                return Err("只有本地目录安装的组件能开热重载".to_string());
            }
            entry.dev_reload = dev_reload;
            Ok(())
        })
    }

    /// 取某个包的目录（**只读台账，不扫盘**）。
    ///
    /// 协议 handler 每个产物请求都要用它：那里绝不能触发一次目录遍历，
    /// 否则一次 `import()` 十几条 chunk 就会变成十几趟全盘扫描。
    pub fn dir_of(&self, id: &str) -> Option<String> {
        lock(&self.0)
            .ledger
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.dir.clone())
    }

    /// 取当前台账版本（广播给前端，用来丢弃迟到的那一份）。
    pub fn revision(&self) -> u64 {
        lock(&self.0).ledger.revision
    }

    /// 前端上报一次装载耗时。
    pub fn report_load_ms(&self, id: &str, load_ms: u64) {
        if let Ok(mut guard) = self.0.lock() {
            guard.load_ms.insert(id.to_string(), load_ms);
        }
    }

    /// 某个包此刻是否开着逐包热重载（日志通道用它决定「留不留这一行」，**只读台账不扫盘**）。
    pub fn dev_reload_of(&self, id: &str) -> bool {
        let guard = lock(&self.0);
        guard
            .ledger
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.dev_reload && entry.source == SOURCE_LOCAL)
            .unwrap_or(false)
    }

    /// 取某个包上次上报的装载耗时。
    fn load_ms_of(&self, id: &str) -> u64 {
        self.0
            .lock()
            .ok()
            .and_then(|guard| guard.load_ms.get(id).copied())
            .unwrap_or(0)
    }

    /// 改一条记录并落盘，返回新的 revision。
    fn edit(&self, id: &str, change: impl FnOnce(&mut LedgerEntry) -> Result<(), String>) -> Result<u64, String> {
        let mut guard = lock(&self.0);
        let entry = guard
            .ledger
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| format!("没有 id 为「{id}」的已装组件"))?;
        change(entry)?;
        guard.ledger.revision += 1;
        let revision = guard.ledger.revision;
        save(&guard.app_data, &guard.ledger)?;
        Ok(revision)
    }
}

/// 加 / 覆盖一条台账记录：覆盖后的 `dev_reload` 仍要过「本地源」判据。
fn upsert(ledger: &mut Ledger, entry: LedgerEntry) {
    let dev_reload = entry.dev_reload && entry.source == SOURCE_LOCAL;
    if let Some(existing) = ledger.entries.iter_mut().find(|item| item.id == entry.id) {
        existing.source = entry.source;
        existing.dir = entry.dir;
        existing.dev_reload = dev_reload;
    } else {
        ledger.entries.push(LedgerEntry { dev_reload, ..entry });
    }
}

/// 写台账：先写临时文件再改名，避免半截 JSON 把下次启动的读路径卡住。
fn save(app_data: &Path, ledger: &Ledger) -> Result<(), String> {
    let root = paths::plugins_root(app_data);
    fs::create_dir_all(&root).map_err(|error| format!("建不出插件目录：{error}"))?;
    let target = paths::ledger_path(app_data);
    let temp = root.join("ledger.json.tmp");
    let text = serde_json::to_string_pretty(ledger).map_err(|error| format!("台账写不出：{error}"))?;
    fs::write(&temp, text).map_err(|error| format!("台账写不出：{error}"))?;
    fs::rename(&temp, target).map_err(|error| format!("台账放不下：{error}"))
}

/// 来源目录名必须等于清单 id —— 否则 app-data 里的目录名与 id 会各说一套。
fn check_identity(dir: &Path, id: &str) -> Result<(), String> {
    let Some(stem) = dir.file_name().and_then(|name| name.to_str()) else {
        return Err("拿不到所选目录的名字".to_string());
    };
    if stem == id {
        return Ok(());
    }
    Err(format!(
        "目录名「{stem}」与清单 id「{id}」不一致：请把包目录改成与 id 同名，或修正清单里的 id"
    ))
}

/// 一次清单读取的结果（失败时也要给出能显示的名字与版本 + 中文原因）。
struct ManifestRead {
    name: String,
    version: String,
    warnings: Vec<String>,
    validated: Option<manifest::Validated>,
    error: Option<String>,
}

/// 读 `<dir>/plugin.json` 并校验。
///
/// 读不到文件 / JSON 坏 / 校验失败都不 panic：把原因变成 `error` 带出去，
/// 名字与版本尽量从原始 JSON 里捞（管理区那一行要显示「旧版时钟 v0.1.0 · 校验失败」）。
fn read_manifest(dir: &Path) -> ManifestRead {
    let fallback_name = dir
        .file_name()
        .and_then(|stem| stem.to_str())
        .unwrap_or("unknown")
        .to_string();
    let manifest_path = dir.join(paths::MANIFEST_FILE);
    // 先卡体积再读：这份文件每次冷扫描都要整读一遍，作者侧写成一块大 JSON 就是每启动一次慢一次。
    if fs::metadata(&manifest_path)
        .map(|meta| meta.len())
        .unwrap_or(0)
        > paths::MAX_MANIFEST_BYTES
    {
        return ManifestRead {
            name: fallback_name,
            version: String::new(),
            warnings: Vec::new(),
            validated: None,
            error: Some(format!(
                "plugin.json 超过 {} KiB 的上限",
                paths::MAX_MANIFEST_BYTES / 1024
            )),
        };
    }
    let text = match fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) => {
            return ManifestRead {
                name: fallback_name,
                version: String::new(),
                warnings: Vec::new(),
                validated: None,
                error: Some(format!("读不到 plugin.json：{error}")),
            }
        }
    };
    match manifest::from_text(&text) {
        Ok(done) => ManifestRead {
            name: done.manifest.name.clone(),
            version: done.manifest.version.clone(),
            warnings: done.warnings.clone(),
            validated: Some(done),
            error: None,
        },
        Err(error) => {
            let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            let loose_name = string_of(&value, "name");
            ManifestRead {
                name: if loose_name.is_empty() { fallback_name } else { loose_name },
                version: string_of(&value, "version"),
                warnings: Vec::new(),
                validated: None,
                error: Some(error),
            }
        }
    }
}

/// 从松散 JSON 里取一个字符串字段；结构不对就当空。
fn string_of(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string()
}

/// 取绝对路径（失败就原样返回：台账里的路径只用于读文件与显示）。
fn abs(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

/// 数一个包的体积与文件数（不跟符号链接跑，文件数到上限就停）。
fn stat_package(dir: &Path) -> PackageStat {
    let mut stat = PackageStat::default();
    stat_into(dir, &mut stat);
    stat
}

/// 递归累加（独立出来是为了能在超限时整体停下）。
fn stat_into(dir: &Path, stat: &mut PackageStat) {
    if stat.overflowed || stat.too_big {
        return;
    }
    let Ok(reader) = fs::read_dir(dir) else {
        return;
    };
    for entry in reader.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // 只走真目录：symlink 目录会让这趟扫描跑出包外（Windows 上开发者常做 junction）。
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            stat_into(&entry.path(), stat);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if stat.files >= paths::MAX_FILES {
            stat.overflowed = true;
            return;
        }
        stat.files += 1;
        if let Ok(meta) = entry.metadata() {
            stat.bytes += meta.len();
            if stat.bytes > paths::MAX_TOTAL_BYTES {
                stat.too_big = true;
                return;
            }
        }
    }
}

/// 取一把锁；中毒的 Mutex 也照用（`panic = "abort"` 下不会有中毒状态残留）。
fn lock<T>(value: &Arc<Mutex<T>>) -> MutexGuard<'_, T> {
    match value.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// 当前毫秒时间戳。
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一个隔离的临时 app-data（每个用例一个，互不干扰）。
    fn temp_app_data(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ufo-plugin-store-{}-{}-{}",
            tag,
            std::process::id(),
            now_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("临时目录建不出来");
        dir
    }

    /// 往 `root/<dir_name>/` 写一份包，清单里的 id 单独给（造「目录名 ≠ id」的用例要用）。
    fn write_package_named(root: &Path, dir_name: &str, manifest_id: &str, version: &str) -> PathBuf {
        let dir = root.join(dir_name);
        fs::create_dir_all(&dir).expect("包目录建不出来");
        fs::write(dir.join("main.js"), "export default { mount() {} }").expect("入口写不出");
        fs::write(
            dir.join(paths::MANIFEST_FILE),
            format!(
                r#"{{ "id": "{manifest_id}", "name": "余额看板", "version": "{version}", "description": "看余额", "entry": "main.js", "surfaces": {{ "cell": 2, "settings": true }}, "permissions": [], "hosts": [] }}"#
            ),
        )
        .expect("清单写不出");
        dir
    }

    /// 写一份目录名与清单 id 一致的包（正常情况）。
    fn write_package(root: &Path, id: &str, version: &str) -> PathBuf {
        write_package_named(root, id, id, version)
    }

    /// 一份必挂不上的包（缺 entry）。
    fn write_broken_package(root: &Path, id: &str) -> PathBuf {
        let dir = root.join(id);
        fs::create_dir_all(&dir).expect("包目录建不出来");
        fs::write(
            dir.join(paths::MANIFEST_FILE),
            format!(r#"{{ "id": "{id}", "name": "旧版时钟", "version": "0.1.0" }}"#),
        )
        .expect("清单写不出");
        dir
    }

    #[test]
    fn installing_a_local_package_stores_only_a_pointer_to_its_own_directory() {
        let root = temp_app_data("install");
        let package = write_package(&root, "balance-board", "1.0.0");
        let store = StoreState::load(root.clone());
        let record = store
            .install_local(&abs(&package))
            .expect("本地包装得上");
        assert_eq!(record.source, SOURCE_LOCAL);
        assert!(!record.dev_reload, "新装的包默认不开调试");
        // 就地装载：app-data/plugins 下不该出现副本目录。
        let copied = paths::package_dir(&root, "balance-board");
        assert!(!copied.exists(), "本地源不该被复制进 app-data");
        let records = store.scan_records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "余额看板");
        assert_eq!(records[0].files, 2);
        assert!(records[0].bytes > 0);
        assert!(records[0].manifest.is_some());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn install_refuses_a_directory_whose_name_is_not_the_id() {
        let root = temp_app_data("identity");
        // 目录叫 wrong-name、清单 id 写 balance-board：作者从别处拷来的包就是这个形状。
        let package = write_package_named(&root, "wrong-name", "balance-board", "1.0.0");
        let store = StoreState::load(root.clone());
        let error = store
            .install_local(&abs(&package))
            .expect_err("目录名与 id 不同必须拒装");
        assert!(error.contains("目录名「wrong-name」"), "{error}");
        assert!(error.contains("清单 id「balance-board」"), "{error}");
        assert!(store.scan_records().is_empty(), "被拒的包不该进台账");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_broken_package_still_produces_a_record_with_a_readable_reason() {
        let root = temp_app_data("broken");
        let package = write_broken_package(&root, "old-clock");
        let mut ledger = Ledger::default();
        upsert(
            &mut ledger,
            LedgerEntry {
                id: "old-clock".to_string(),
                dir: abs(&package),
                ..LedgerEntry::default()
            },
        );
        save(&root, &ledger).expect("台账写不出");
        let store = StoreState::load(root.clone());
        let records = store.scan_records();
        assert_eq!(records.len(), 1, "坏包也要进表");
        assert!(records[0].manifest.is_none());
        // 名字与版本从原始 JSON 里捞出来，列表行不至于只剩 id。
        assert_eq!(records[0].name, "旧版时钟");
        assert_eq!(records[0].version, "0.1.0");
        assert!(records[0].error.as_deref().unwrap_or("").contains("entry"), "{:?}", records[0].error);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dev_reload_survives_a_rescan_of_a_local_package() {
        let root = temp_app_data("survive");
        let package = write_package(&root, "balance-board", "1.0.0");
        let store = StoreState::load(root.clone());
        store.install_local(&abs(&package)).expect("装不上");
        store.set_dev_reload("balance-board", true).expect("本地源该能开");
        let records = store.scan_records();
        assert!(records[0].dev_reload, "重扫之后开关还得在");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn remote_sources_cannot_enable_dev_reload() {
        let root = temp_app_data("remote");
        let mut ledger = Ledger::default();
        upsert(
            &mut ledger,
            LedgerEntry {
                id: "from-url".to_string(),
                source: SOURCE_COPIED.to_string(),
                dir: abs(&write_package(&root, "from-url", "1.0.0")),
                dev_reload: false,
            },
        );
        save(&root, &ledger).expect("台账写不出");
        let store = StoreState::load(root.clone());
        let error = store
            .set_dev_reload("from-url", true)
            .expect_err("远程源不许开热重载");
        assert!(error.contains("只有本地目录"), "{error}");
        assert!(!store.scan_records()[0].dev_reload);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dev_reload_dies_when_the_source_becomes_remote() {
        // 覆盖安装的形状：先本地 + 开开关，再用 copied 源 upsert 同 id。
        let mut ledger = Ledger::default();
        upsert(
            &mut ledger,
            LedgerEntry {
                id: "x".to_string(),
                source: SOURCE_LOCAL.to_string(),
                dir: "C:/work/x".to_string(),
                dev_reload: true,
            },
        );
        assert!(ledger.entries[0].dev_reload);
        upsert(
            &mut ledger,
            LedgerEntry {
                id: "x".to_string(),
                source: SOURCE_COPIED.to_string(),
                dir: "app-data/plugins/x".to_string(),
                // 覆盖安装带来的开关状态：即使外面写了 true，也必须被判据夹回 false。
                dev_reload: true,
            },
        );
        assert!(!ledger.entries[0].dev_reload, "远程源不能带着热重载开关活下来");
    }

    #[test]
    fn an_oversized_package_is_refused_and_names_the_cap() {
        let root = temp_app_data("oversize");
        let package = write_package(&root, "balance-board", "1.0.0");
        fs::write(package.join("blob.bin"), vec![0u8; paths::MAX_TOTAL_BYTES as usize + 1])
            .expect("blob 写不进");
        let store = StoreState::load(root.clone());
        let error = store.preview(&abs(&package)).expect_err("超体积的包不该装");
        assert!(error.contains("总体积超过 48 MiB"), "{error}");
        let _ = fs::remove_dir_all(&root);
    }

    /// 入口超体积帽：装之前就该拒，而不是等装载时来一句看不懂的 import 失败。
    #[test]
    fn an_oversized_entry_is_refused_before_it_is_served() {
        let root = temp_app_data("big-entry");
        let package = write_package(&root, "balance-board", "1.0.0");
        fs::write(package.join("main.js"), vec![b' '; paths::MAX_SCRIPT_BYTES as usize + 1])
            .expect("入口写不进");
        let store = StoreState::load(root.clone());
        let error = store.preview(&abs(&package)).expect_err("超上限的入口不该装");
        assert!(error.contains("入口「main.js」"), "{error}");
        assert!(error.contains("超过 8 MiB 上限"), "{error}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_oversized_manifest_is_refused_before_it_is_parsed() {
        let root = temp_app_data("big-manifest");
        let package = write_package(&root, "balance-board", "1.0.0");
        fs::write(
            package.join("plugin.json"),
            vec![b' '; paths::MAX_MANIFEST_BYTES as usize + 1],
        )
        .expect("清单写不进");
        let store = StoreState::load(root.clone());
        let error = store.preview(&abs(&package)).expect_err("超上限的清单不该装");
        assert!(error.contains("plugin.json 超过 64 KiB"), "{error}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_write_bumps_the_revision_and_persists() {
        let root = temp_app_data("toggle");
        let package = write_package(&root, "balance-board", "1.0.0");
        let store = StoreState::load(root.clone());
        store.install_local(&abs(&package)).expect("装不上");
        // 装一次是第 1 版，改一次是第 2 版：前端靠这个数丢弃迟到的广播。
        let revision = store.set_dev_reload("balance-board", true).expect("开得动");
        assert_eq!(revision, 2);
        // 换一个 StoreState 读同一份盘：证明真的落盘了。
        let reopened = StoreState::load(root.clone());
        assert!(reopened.scan_records()[0].dev_reload);
        assert!(reopened
            .set_dev_reload("missing-id", true)
            .expect_err("没装的包改不了")
            .contains("没有 id"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn uninstall_drops_the_entry_but_never_the_authors_directory() {
        let root = temp_app_data("uninstall");
        let package = write_package(&root, "balance-board", "1.0.0");
        let store = StoreState::load(root.clone());
        store.install_local(&abs(&package)).expect("装不上");
        store.uninstall("balance-board").expect("卸得掉");
        assert!(store.scan_records().is_empty());
        assert!(package.exists(), "本地源是作者的工作区，卸载不能删它");
        assert!(store
            .uninstall("balance-board")
            .expect_err("重复卸载要报错")
            .contains("没有 id"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unreadable_ledger_degrades_to_empty_instead_of_panicking() {
        let root = temp_app_data("corrupt");
        fs::create_dir_all(paths::plugins_root(&root)).expect("目录建不出");
        fs::write(paths::ledger_path(&root), "{ not json").expect("坏文件写不出");
        let store = StoreState::load(root.clone());
        assert!(store.scan_records().is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn load_ms_reports_are_read_back_without_touching_the_ledger() {
        let root = temp_app_data("loadms");
        let package = write_package(&root, "balance-board", "1.0.0");
        let store = StoreState::load(root.clone());
        store.install_local(&abs(&package)).expect("装不上");
        assert_eq!(store.scan_records()[0].load_ms, 0);
        store.report_load_ms("balance-board", 42);
        assert_eq!(store.scan_records()[0].load_ms, 42);
        let _ = fs::remove_dir_all(&root);
    }
}
