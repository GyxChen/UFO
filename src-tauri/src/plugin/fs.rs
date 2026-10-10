//! 组件包的文件能力：私有目录 + 用户亲手选中的文件（句柄制）。
//!
//! 三条边界全部在这里落地，命令层只管闸（有没有声明 `fs`）：
//!
//! 1. **私有目录**（`plugins/<id>/files`）是组件唯一能写任意字节的地方，帽有三顶：
//!    单文件 / 整包 / 全局。逐段路径校验 + 新建文件的父目录先规范化再对表，
//!    符号链接逃逸与 `..` 都在这一步变成错误。
//! 2. **用户选中的文件只给会话内句柄**：句柄不落台账、不跨启动，重启后必须重选 ——
//!    这正是装前披露里那句「每次都要重新选」的实现。句柄里带一段进程内随机数，
//!    否则 `<id>-1` 这种可猜的编号会让 A 包读到 B 包刚选中的文件。
//! 3. **压缩包条目名只当标识符**，永不拼进文件系统路径。宿主因此要解格式，接四件事：
//!    单条目解压上限、压缩比上限（炸弹）、加密 / 非 deflate 明确报错、目录表按 mtime 缓存。
//!
//! ⚠️ `panic = "abort"`：这里不许 unwrap / expect / 索引 / slice 未校验的下标。

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use base64::Engine as _;

use super::handle;
use super::paths;

/// 一次「用户选中的文件」对外的形状；此后所有读都靠 `handle`。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedFile {
    /// 会话内句柄（不可猜）。
    pub handle: String,
    /// 文件名（只用于界面显示与扩展名判断）。
    pub name: String,
    /// 文件总字节数：作者据此决定分块还是一次读完。
    pub size: u64,
    /// 是不是宿主认得出的压缩包（zip 家族）。
    pub archive: bool,
}

/// 压缩包目录表里的一项。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZipEntry {
    /// 条目名（压缩包里的相对路径，**只当标识符**）。
    pub name: String,
    /// 解压后的字节数。
    pub size: u64,
}

/// 私有目录里的一项。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateFile {
    /// 相对私有目录的路径（与写入时给的那个串一致）。
    pub name: String,
    /// 字节数。
    pub size: u64,
}

/// 某个包私有目录的占用读数（管理区详情用）。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsUsage {
    /// 当前字节数。
    pub bytes: u64,
    /// 这一包的上限。
    pub limit: u64,
    /// 文件个数。
    pub files: usize,
}

/// 一个已发放的句柄（内部形状，含归属包 id）。
#[derive(Clone, Debug)]
struct Pick {
    plugin_id: String,
    path: PathBuf,
    name: String,
    size: u64,
    archive: bool,
    /// 发句柄时看到的修改时间：目录表缓存靠它判失效。
    modified_ms: i64,
}

/// 运行态。
#[derive(Clone, Default)]
pub struct FsState {
    app_data: PathBuf,
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug, Default)]
struct Inner {
    /// handle → 选中的文件。
    picks: BTreeMap<String, Pick>,
    /// 句柄序号（与随机段一起构成 handle）。
    seq: u64,
    /// handle → (发句柄时的 mtime, 目录表)：换文件后 mtime 变了，缓存自动失效。
    entries: BTreeMap<String, (i64, Vec<ZipEntry>)>,
}

/// 宿主认得出的压缩包扩展名（zip 家族）。
///
/// 认不出的扩展名一律按不透明字节给作者 —— 少认一种不会出错，多认一种（如 rar）会解不出来。
const ARCHIVE_EXTENSIONS: &[&str] = &[
    "zip", "epub", "jar", "docx", "xlsx", "pptx", "vsdx", "apk", "whl", "cbz", "egg", "ody",
];

impl FsState {
    /// 用 app-data 建运行态（与日志、存储同一份 app-data）。
    pub fn new(app_data: PathBuf) -> Self {
        Self {
            app_data,
            inner: Arc::new(Mutex::new(Inner::default())),
        }
    }

    /// 这个包的私有目录。
    fn root(&self, plugin_id: &str) -> PathBuf {
        paths::plugin_files_dir(&self.app_data, plugin_id)
    }

    // ---- 用户选中的文件 ----

    /// 记下「用户刚在对话框里选中的这个文件」，发一个会话内句柄。
    ///
    /// 调用方（命令层）只会在原生对话框真的返回路径之后走到这里，
    /// 因此「选中」这个动作就是授权本身 —— 这里再挡三类不该发的：不存在 / 不是文件 / 太大。
    pub fn grant(&self, plugin_id: &str, path: PathBuf) -> Result<PickedFile, String> {
        if !paths::is_valid_plugin_id(plugin_id) {
            return Err(format!("id「{plugin_id}」不合法"));
        }
        let meta = path
            .metadata()
            .map_err(|_| "选中的文件读不到（可能已被移动或删除）".to_string())?;
        if !meta.is_file() {
            return Err("选中的不是单个文件；这个口子不接整目录".to_string());
        }
        let size = meta.len();
        if size > paths::FS_PICK_MAX_BYTES {
            return Err(format!(
                "这个文件有 {size} 字节，超过本宿主的 {} 字节上限",
                paths::FS_PICK_MAX_BYTES
            ));
        }
        let name = path
            .file_name()
            .map(|v| v.to_string_lossy().to_string())
            .unwrap_or_else(|| "选中的文件".to_string());
        let archive = is_archive_name(&name);

        let mut guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        // 两道帽：先看这一包握了几个，再看整个 App（防止一堆包各握满）。
        let mine = guard.picks.values().filter(|p| p.plugin_id == plugin_id).count();
        if mine >= paths::PICK_HANDLES_PER_PACKAGE {
            return Err(format!(
                "这个包已握着 {} 个选中的文件，请先用完再选",
                paths::PICK_HANDLES_PER_PACKAGE
            ));
        }
        if guard.picks.len() >= paths::PICK_HANDLES_TOTAL {
            return Err("同时打开的选中文件已达上限，请关掉一些组件再试".to_string());
        }
        guard.seq += 1;
        let handle = handle::unguessable(plugin_id, guard.seq, &path.to_string_lossy());
        let pick = Pick {
            plugin_id: plugin_id.to_string(),
            path,
            name,
            size,
            archive,
            modified_ms: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
        };
        let issued = PickedFile {
            handle: handle.clone(),
            name: pick.name.clone(),
            size: pick.size,
            archive: pick.archive,
        };
        guard.picks.insert(handle, pick);
        Ok(issued)
    }

    /// 取回这个句柄，并确认它属于来问的那个包。
    fn pick_of(&self, plugin_id: &str, handle: &str) -> Result<Pick, String> {
        let guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let pick = guard
            .picks
            .get(handle)
            .ok_or_else(|| "这个句柄已经失效了：重新选一次文件（句柄不跨启动）".to_string())?;
        if pick.plugin_id != plugin_id {
            return Err("这个句柄不属于你".to_string());
        }
        Ok(pick.clone())
    }

    /// 句柄指向的文件现在还在不在、多大（作者用来发现「文件被挪走了」）。
    pub fn stat_pick(&self, plugin_id: &str, handle: &str) -> Result<PickedFile, String> {
        let pick = self.pick_of(plugin_id, handle)?;
        let meta = pick
            .path
            .metadata()
            .map_err(|_| "选中的文件读不到（可能已被移动或删除）".to_string())?;
        Ok(PickedFile {
            handle: handle.to_string(),
            name: pick.name,
            size: meta.len(),
            archive: pick.archive,
        })
    }

    /// 句柄属于哪个包。
    ///
    /// 协议层只有 URL、没有调用方身份 —— 句柄里的随机段就是授权凭证，
    /// 这里把它还原成归属，用来查「这个包声明过 fs 吗」和「是不是还在装着」。
    pub fn owner_of(&self, handle: &str) -> Option<String> {
        let guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.picks.get(handle).map(|pick| pick.plugin_id.clone())
    }

    /// 私有目录里某个文件的大小（协议层算 Content-Range 用，不必整读）。
    pub fn private_size(&self, plugin_id: &str, rel: &str) -> Result<u64, String> {
        let target = self.resolve(plugin_id, rel)?;
        let meta = target
            .metadata()
            .map_err(|_| "这个文件还没写过（或已被删掉）".to_string())?;
        if !meta.is_file() {
            return Err("那不是一个文件".to_string());
        }
        Ok(meta.len())
    }

    /// 按字节窗口读选中的文件（`[start, start+len)`），len 会被夹进上限与文件尾。
    pub fn read_pick(&self, plugin_id: &str, handle: &str, start: u64, len: u64) -> Result<Vec<u8>, String> {
        let pick = self.pick_of(plugin_id, handle)?;
        read_window_at(&pick.path, pick.size, start, len)
    }

    /// 列出压缩包目录表（缓存按 mtime 失效）。
    pub fn entries(&self, plugin_id: &str, handle: &str) -> Result<Vec<ZipEntry>, String> {
        let pick = self.pick_of(plugin_id, handle)?;
        if !pick.archive {
            return Err("这个文件不是宿主认得出的压缩包（zip 家族）".to_string());
        }
        let cached: Option<(i64, Vec<ZipEntry>)> = {
            let guard = match self.inner.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.entries.get(handle).cloned()
        };
        if let Some((modified_ms, list)) = cached {
            if modified_ms == pick.modified_ms {
                return Ok(list);
            }
        }
        let list = list_entries(&pick.path)?;
        let mut guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.entries.insert(handle.to_string(), (pick.modified_ms, list.clone()));
        Ok(list)
    }

    /// 读压缩包里某一个条目的一个字节窗口。
    ///
    /// ⚠️ deflate 条目**不能从中间开始解**：`start > 0` 时宿主是从头解到 `start+len` 再切窗口。
    /// 这也是界面承诺「整本条目上限」的原因 —— 大条目请用 URL 让浏览器自己分段取。
    pub fn read_member(
        &self,
        plugin_id: &str,
        handle: &str,
        name: &str,
        start: u64,
        len: u64,
    ) -> Result<Vec<u8>, String> {
        let pick = self.pick_of(plugin_id, handle)?;
        let bytes = inflate_member(&pick.path, name)?;
        let total = bytes.len() as u64;
        window_of(&bytes, total, start, len)
    }

    /// 这个句柄的条目有多大（URL 层要据此算 Content-Range，不必再解一遍）。
    pub fn member_size(&self, plugin_id: &str, handle: &str, name: &str) -> Result<u64, String> {
        let list = self.entries(plugin_id, handle)?;
        list.into_iter().find(|item| item.name == name).map(|item| item.size).ok_or_else(|| {
            "压缩包里没有这一项；请先用 entries() 看清条目名".to_string()
        })
    }

    /// 这个包不再握着任何句柄（停用 / 重载 / 卸载）：句柄与目录表缓存一起丢。
    pub fn revoke(&self, plugin_id: &str) {
        let mut guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let stale: Vec<String> = guard
            .picks
            .iter()
            .filter(|(_, pick)| pick.plugin_id == plugin_id)
            .map(|(handle, _)| handle.clone())
            .collect();
        for handle in &stale {
            guard.picks.remove(handle);
            guard.entries.remove(handle);
        }
    }

    // ---- 私有目录 ----

    /// 把「相对私有目录的路径」解析成一个安全可写的绝对位置。
    ///
    /// 做法：先挡掉不合法的相对路径，再把**父目录**建出来并规范化，
    /// 确认它仍在私有目录内，最后拼上单个文件名 ——
    /// 文件名段不含分隔符是 `is_safe_rel_path` 保证的，所以这一步之后不可能再跑出去。
    fn resolve(&self, plugin_id: &str, rel: &str) -> Result<PathBuf, String> {
        if !paths::is_valid_plugin_id(plugin_id) {
            return Err(format!("id「{plugin_id}」不合法"));
        }
        if !paths::is_safe_rel_path(rel) {
            return Err("路径不是合法的相对路径（不许绝对、盘符、反斜杠、`..` 或空段）".to_string());
        }
        let root = self.root(plugin_id);
        if let Err(error) = std::fs::create_dir_all(&root) {
            return Err(format!("私有目录建不出来：{error}"));
        }
        let root = std::fs::canonicalize(&root).map_err(|error| format!("私有目录打不开：{error}"))?;
        let (parent_rel, file_name) = match rel.rsplit_once('/') {
            Some((parent, name)) => (parent, name),
            None => ("", rel),
        };
        let parent = if parent_rel.is_empty() {
            root.clone()
        } else {
            root.join(parent_rel)
        };
        std::fs::create_dir_all(&parent).map_err(|error| format!("子目录建不出来：{error}"))?;
        let parent = std::fs::canonicalize(&parent).map_err(|error| format!("子目录打不开：{error}"))?;
        if !parent.starts_with(&root) {
            return Err("路径越出了这个组件的私有目录".to_string());
        }
        Ok(parent.join(file_name))
    }

    /// 写一个文件（`encoding` 只认 `utf8` / `base64`）。
    ///
    /// 三顶帽在落盘之前判完：坏数据不会留下半个文件，也不会「写完才发现超了」。
    pub fn write(&self, plugin_id: &str, rel: &str, data: &str, encoding: &str) -> Result<PrivateFile, String> {
        let bytes = match encoding {
            "utf8" | "" => data.as_bytes().to_vec(),
            "base64" => base64::prelude::BASE64_STANDARD
                .decode(data)
                .map_err(|_| "base64 解码失败：请确认没有换行外的空白与缺位补等号".to_string())?,
            other => return Err(format!("不支持的编码「{other}」，只认 utf8 / base64")),
        };
        if bytes.len() as u64 > paths::FS_WRITE_BYTES {
            return Err(format!(
                "一次写入 {} 字节，超过单文件上限 {} 字节",
                bytes.len(),
                paths::FS_WRITE_BYTES
            ));
        }
        let target = self.resolve(plugin_id, rel)?;
        let root = self.root(plugin_id);
        let package_bytes = dir_bytes(&root).map(|walk| walk.0)?;
        let global_bytes = self.global_bytes()?;
        check_caps(
            bytes.len() as u64,
            package_bytes,
            global_bytes,
            paths::FS_PACKAGE_BYTES,
            paths::FS_GLOBAL_BYTES,
        )?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| format!("子目录建不出来：{error}"))?;
        }
        std::fs::write(&target, &bytes).map_err(|error| format!("写入失败：{error}"))?;
        Ok(PrivateFile {
            name: rel.to_string(),
            size: bytes.len() as u64,
        })
    }

    /// 读自己私有目录里的一个窗口。
    pub fn read_private(&self, plugin_id: &str, rel: &str, start: u64, len: u64) -> Result<Vec<u8>, String> {
        let target = self.resolve(plugin_id, rel)?;
        let meta = target
            .metadata()
            .map_err(|_| "这个文件还没写过（或已被删掉）".to_string())?;
        if !meta.is_file() {
            return Err("那不是一个文件".to_string());
        }
        read_window_at(&target, meta.len(), start, len)
    }

    /// 列私有目录（递归，深度有上限；只给相对路径）。
    pub fn list(&self, plugin_id: &str) -> Result<Vec<PrivateFile>, String> {
        let root = self.root(plugin_id);
        if !root.exists() {
            return Ok(Vec::new());
        }
        let mut out: Vec<PrivateFile> = Vec::new();
        collect_files(&root, &root, &mut out, paths::MAX_FILES)?;
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// 删一个文件（不存在也算成功，便于反复点）。
    pub fn remove(&self, plugin_id: &str, rel: &str) -> Result<bool, String> {
        let target = self.resolve(plugin_id, rel)?;
        match std::fs::remove_file(&target) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(format!("删除失败：{error}")),
        }
    }

    /// 这个包的占用读数。
    pub fn usage(&self, plugin_id: &str) -> Result<FsUsage, String> {
        let root = self.root(plugin_id);
        let (bytes, files) = if root.exists() {
            dir_bytes(&root)?
        } else {
            (0, 0)
        };
        Ok(FsUsage {
            bytes,
            limit: paths::FS_PACKAGE_BYTES,
            files,
        })
    }

    /// 卸载：句柄丢掉 + 私有目录整片删除（承诺过「卸载即删」）。
    pub fn forget(&self, plugin_id: &str) -> Result<(), String> {
        self.revoke(plugin_id);
        let root = self.root(plugin_id);
        if root.exists() {
            std::fs::remove_dir_all(&root).map_err(|error| format!("私有目录删不掉：{error}"))?;
        }
        Ok(())
    }

    /// 所有包的私有目录合计（全局帽的读数）。
    fn global_bytes(&self) -> Result<u64, String> {
        let root = paths::plugins_root(&self.app_data);
        if !root.exists() {
            return Ok(0);
        }
        let mut total = 0u64;
        let Ok(entries) = std::fs::read_dir(&root) else {
            return Ok(0);
        };
        for entry in entries.flatten() {
            let files = entry.path().join(paths::FS_DIR);
            if files.is_dir() {
                total += dir_bytes(&files)?.0;
            }
        }
        Ok(total)
    }
}

/// 三顶帽的判定，纯函数：写成纯函数是为了能在测试里用小数字把三条都打到。
fn check_caps(
    incoming: u64,
    package_bytes: u64,
    global_bytes: u64,
    package_limit: u64,
    global_limit: u64,
) -> Result<(), String> {
    if incoming > package_limit || package_bytes.saturating_add(incoming) > package_limit {
        return Err(format!(
            "这个组件的私有目录最多 {package_limit} 字节，已用 {package_bytes} 字节，写不下这一份"
        ));
    }
    if global_bytes.saturating_add(incoming) > global_limit {
        return Err(format!(
            "所有组件的私有目录合计已达上限（{global_limit} 字节），请先卸载不再用的组件"
        ));
    }
    Ok(())
}

/// 一个文件名是不是宿主认得出的压缩包。
fn is_archive_name(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((_, ext)) => ARCHIVE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

/// 一个字节窗口 `[start, start+len)`：`len` 夹到上限，`start+len` 夹到文件尾。
fn window_of(bytes: &[u8], total: u64, start: u64, len: u64) -> Result<Vec<u8>, String> {
    if start >= total {
        return Ok(Vec::new());
    }
    let limit = len.min(paths::FS_READ_WINDOW_BYTES).max(1);
    let end = start.saturating_add(limit).min(total);
    // start < total 且 end <= total 且 end > start，下面这段切片一定在界内。
    let from = start as usize;
    let to = end as usize;
    if to <= from {
        return Ok(Vec::new());
    }
    Ok(bytes.get(from..to).map(|slice| slice.to_vec()).unwrap_or_default())
}

/// 从磁盘上的某个文件按窗口取字节（不整读：400 MB 的书不该进内存）。
fn read_window_at(path: &Path, total: u64, start: u64, len: u64) -> Result<Vec<u8>, String> {
    if total == 0 {
        return Ok(Vec::new());
    }
    let limit = len.min(paths::FS_READ_WINDOW_BYTES).max(1);
    let end = start.saturating_add(limit).min(total);
    if start >= total || end <= start {
        return Ok(Vec::new());
    }
    let want = (end - start) as usize;
    let mut file = File::open(path).map_err(|error| format!("文件打不开：{error}"))?;
    if start > 0 {
        file.seek(SeekFrom::Start(start))
            .map_err(|error| format!("定位失败：{error}"))?;
    }
    let mut buffer = vec![0u8; want];
    let mut filled = 0usize;
    while filled < want {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(format!("读取失败：{error}")),
        }
    }
    buffer.truncate(filled);
    Ok(buffer)
}

/// 打开压缩包读目录表（只列，不解内容）。
fn list_entries(path: &Path) -> Result<Vec<ZipEntry>, String> {
    let file = File::open(path).map_err(|error| format!("文件打不开：{error}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| format!("不是能读的压缩包：{error}"))?;
    let count = archive.len();
    if count > paths::ZIP_ENTRIES_MAX {
        return Err(format!(
            "这个压缩包有 {count} 项，超过本宿主一次能列的 {} 项",
            paths::ZIP_ENTRIES_MAX
        ));
    }
    let mut out = Vec::new();
    for index in 0..count {
        let Ok(entry) = archive.by_index(index) else {
            return Err("压缩包目录表读不下去（结构可能损坏）".to_string());
        };
        if entry.is_dir() {
            continue;
        }
        out.push(ZipEntry {
            name: entry.name().to_string(),
            size: entry.size(),
        });
    }
    Ok(out)
}

/// 解出压缩包里某一个条目的全部字节（夹在两顶帽下）。
///
/// 目录表里的 `size` 是**声明值**，可以撒谎（炸弹就这么写），所以边解边数，
/// 解到超上限立刻停 —— 声明值只用来提前拒绝，不用来信任。
fn inflate_member(path: &Path, name: &str) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|error| format!("文件打不开：{error}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| format!("不是能读的压缩包：{error}"))?;
    let count = archive.len().min(paths::ZIP_ENTRIES_MAX);
    let mut found: Option<usize> = None;
    for index in 0..count {
        if let Ok(entry) = archive.by_index(index) {
            if entry.name() == name {
                found = Some(index);
                break;
            }
        }
    }
    let index = match found {
        Some(index) => index,
        None => return Err("压缩包里没有这一项；请先用 entries() 看清条目名".to_string()),
    };
    let mut entry = match archive.by_index(index) {
        Ok(entry) => entry,
        // 加密条目在这一句就会失败（`zip` 不带解密能力）。单独把它认出来：
        // 「这一项打不开」会让作者以为是路径写错，而真正的原因是它上了锁。
        Err(error) => {
            let text = error.to_string();
            if text.contains("ncrypt") || text.contains("password") {
                return Err("这个条目是加密的，宿主不解密码".to_string());
            }
            return Err(format!("这一项打不开：{text}"));
        }
    };
    let declared = entry.size();
    let compressed = entry.compressed_size();
    if declared > paths::ZIP_ENTRY_BYTES {
        return Err(format!(
            "这一项解压后有 {declared} 字节，超过单条目上限 {} 字节",
            paths::ZIP_ENTRY_BYTES
        ));
    }
    // 压缩比：声明值已经过大时再看比例，防「头部写 60 MiB、实际只占 60 KiB」的那种。
    if compressed > 0 && declared / compressed > paths::ZIP_RATIO_MAX as u64 {
        return Err(format!(
            "这一项的压缩比超过 {} 倍，按压缩包炸弹处理",
            paths::ZIP_RATIO_MAX
        ));
    }
    let cap = paths::ZIP_ENTRY_BYTES as usize;
    let mut bytes: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        match entry.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                if bytes.len() + read > cap {
                    return Err(format!(
                        "这一项实际解压超过 {} 字节上限，已停在这里",
                        paths::ZIP_ENTRY_BYTES
                    ));
                }
                bytes.extend_from_slice(chunk.get(..read).unwrap_or(&chunk));
            }
            Err(error) => return Err(format!("解压失败（条目可能损坏或非 deflate）：{error}")),
        }
    }
    Ok(bytes)
}

/// 目录里全部文件的字节数与个数（深度有上限，不跟符号链接）。
fn dir_bytes(root: &Path) -> Result<(u64, usize), String> {
    let mut total = 0u64;
    let mut files = 0usize;
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 8 {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.file_type() else {
                continue;
            };
            if meta.is_symlink() {
                // 不跟符号链接：私有目录里的软链是作者自己造的，跟着走就出了这个目录。
                continue;
            }
            if meta.is_dir() {
                stack.push((path, depth + 1));
                continue;
            }
            if let Ok(size) = path.metadata().map(|m| m.len()) {
                total += size;
                files += 1;
            }
        }
    }
    Ok((total, files))
}

/// 递归收集文件（相对 `root` 的路径），到 `max` 个就停。
fn collect_files(root: &Path, dir: &Path, out: &mut Vec<PrivateFile>, max: usize) -> Result<(), String> {
    if out.len() >= max {
        return Ok(());
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        if out.len() >= max {
            return Ok(());
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_files(root, &path, out, max)?;
            continue;
        }
        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
        let name = path
            .strip_prefix(root)
            .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| String::new());
        if !name.is_empty() {
            out.push(PrivateFile { name, size });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// 每测一个独立目录（进程号 + 标签；测完删掉）。
    fn temp_app_data(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ufo-plugin-fs-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    /// 造一个能读的压缩包（一个 deflate 条目 + 一个 stored 条目 + 一个「越界形状」的条目名）。
    fn make_archive(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
        use zip::write::SimpleFileOptions;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, data, deflate) in entries {
            let options = if *deflate {
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
            } else {
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)
            };
            writer.start_file(*name, options).expect("开条目");
            writer.write_all(data).expect("写条目");
        }
        writer.finish().expect("收包").into_inner()
    }

    /// 把一个压缩包落到磁盘上，返回路径。
    fn write_archive_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        std::fs::create_dir_all(dir).expect("建目录");
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("写包");
        path
    }

    #[test]
    fn caps_are_checked_by_a_pure_function() {
        // 三条各打一次，且只有对应那条红：单文件超帽 / 整包超帽 / 全局超帽。
        let err_single = check_caps(200, 0, 0, 100, 10_000).expect_err("单文件超帽");
        assert!(err_single.contains("私有目录最多"), "{err_single}");
        let err_package = check_caps(60, 60, 0, 100, 10_000).expect_err("整包超帽");
        assert!(err_package.contains("写不下这一份"), "{err_package}");
        let err_global = check_caps(10, 10, 9_995, 100, 10_000).expect_err("全局超帽");
        assert!(err_global.contains("合计已达上限"), "{err_global}");
        assert!(check_caps(10, 10, 10, 100, 10_000).is_ok());
    }

    #[test]
    fn private_paths_that_escape_are_refused_before_any_write() {
        let app = temp_app_data("escape");
        let state = FsState::new(app.clone());
        for rel in [
            "../evil.txt",
            "a/../../evil.txt",
            "/etc/passwd",
            "C:/Windows/win.ini",
            "a\\b.txt",
            "",
            ".",
            "..",
            "a//b.txt",
        ] {
            let err = state
                .write("probe", rel, "x", "utf8")
                .expect_err("这种路径必须写不进去");
            assert!(err.contains("路径") || err.contains("id"), "{rel} -> {err}");
        }
        // 合法的多段路径要写得进去，并且落在私有目录里面。
        assert!(state.write("probe", "cache/note.txt", "你好", "utf8").is_ok());
        let written = paths::plugin_files_dir(&app, "probe").join("cache").join("note.txt");
        assert!(written.exists(), "写的位置不对：{}", written.display());
        assert_eq!(std::fs::read_to_string(&written).expect("读回"), "你好");
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn bad_base64_and_unknown_encoding_error_instead_of_panicking() {
        let app = temp_app_data("base64");
        let state = FsState::new(app.clone());
        let err = state.write("probe", "x.bin", "!!!not base64!!!", "base64").expect_err("坏 base64");
        assert!(err.contains("base64"), "{err}");
        let err2 = state.write("probe", "x.bin", "x", "hex").expect_err("未知编码");
        assert!(err2.contains("不支持的编码"), "{err2}");
        // 正常路径：base64 的字节要原样落盘（「可写任意字节」的承诺）。
        let raw = vec![0u8, 1, 250, 255];
        let encoded = base64::prelude::BASE64_STANDARD.encode(&raw);
        state.write("probe", "x.bin", &encoded, "base64").expect("写二进制");
        assert_eq!(std::fs::read(paths::plugin_files_dir(&app, "probe").join("x.bin")).expect("读回"), raw);
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn read_and_usage_agree_with_what_was_written() {
        let app = temp_app_data("usage");
        let state = FsState::new(app.clone());
        state.write("probe", "a.txt", "0123456789", "utf8").expect("写 a");
        state.write("probe", "sub/b.txt", "abc", "utf8").expect("写 b");
        let window = state.read_private("probe", "a.txt", 2, 4).expect("读窗口");
        assert_eq!(String::from_utf8(window).expect("utf8"), "2345");
        // len 为 0 时按 1 字节给（`demand` 式的 0 不等于「不读」）。
        assert_eq!(state.read_private("probe", "a.txt", 0, 0).expect("零长度").len(), 1);
        // 超出文件尾的起点回空，而不是报错。
        assert!(state.read_private("probe", "a.txt", 99, 4).expect("过尾").is_empty());
        let usage = state.usage("probe").expect("占用");
        assert_eq!(usage.bytes, 13, "占用数不对：{:?}", usage.bytes);
        assert_eq!(usage.files, 2);
        let listed: Vec<String> = state.list("probe").expect("列目录").into_iter().map(|f| f.name).collect();
        assert_eq!(listed, vec!["a.txt".to_string(), "sub/b.txt".to_string()]);
        assert!(state.remove("probe", "a.txt").expect("删"));
        assert!(!state.remove("probe", "a.txt").expect("再删要幂等"));
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn forget_removes_the_private_dir_and_the_handles() {
        let app = temp_app_data("forget");
        let state = FsState::new(app.clone());
        state.write("probe", "a.txt", "x", "utf8").expect("写");
        let file = write_archive_file(&app, "book.epub", &make_archive(&[("a", b"hi", true)]));
        let issued = state.grant("probe", file.clone()).expect("发句柄");
        state.forget("probe").expect("卸载清理");
        assert!(!paths::plugin_files_dir(&app, "probe").exists(), "私有目录要整片删掉");
        assert!(state.read_pick("probe", &issued.handle, 0, 4).is_err(), "句柄也该失效");
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn handles_are_scoped_to_the_package_that_asked() {
        let app = temp_app_data("scope");
        let state = FsState::new(app.clone());
        let file = write_archive_file(&app, "book.epub", &make_archive(&[("OEBPS/a.xhtml", b"<h1>hi</h1>", true)]));
        let issued = state.grant("probe", file.clone()).expect("probe 拿到句柄");
        // 别的包拿同一个句柄：必须拒（句柄就是授权凭证，不能通用）。
        let err = state.stat_pick("other", &issued.handle).expect_err("other 不该拿到");
        assert!(err.contains("不属于你"), "{err}");
        // 未知句柄也说清是「失效」，别让作者以为路径写错了。
        let stale = state.stat_pick("probe", "probe-1-deadbeef").expect_err("假句柄");
        assert!(stale.contains("重新选"), "{stale}");
        assert_eq!(state.stat_pick("probe", &issued.handle).expect("自己的").size, file_metadata_size(&file));
        let _ = std::fs::remove_dir_all(&app);
    }

    /// 文件大小（测试里的对照值）。
    fn file_metadata_size(path: &Path) -> u64 {
        path.metadata().map(|m| m.len()).unwrap_or(0)
    }

    #[test]
    fn grant_refuses_missing_directories_and_non_zip_names() {
        let app = temp_app_data("grant");
        let state = FsState::new(app.clone());
        assert!(state.grant("probe", app.join("nope.bin")).is_err(), "不存在该拒");
        assert!(state.grant("probe", app.clone()).is_err(), "目录不是文件");
        // 句柄帽：per-package 到顶后是这一条原因，不是全局那条。
        let file = write_archive_file(&app, "note.zip", &make_archive(&[("a", b"1", false)]));
        let mut handles = Vec::new();
        for _ in 0..paths::PICK_HANDLES_PER_PACKAGE {
            handles.push(state.grant("probe", file.clone()).expect("发句柄").handle);
        }
        let err = state.grant("probe", file.clone()).expect_err("该到顶了");
        assert!(err.contains("已握着"), "{err}");
        // 另一个包仍可发（帽是按包算的）。
        assert!(state.grant("other", file).is_ok());
        state.revoke("probe");
        let gone = state.read_pick("probe", &handles[0], 0, 1).expect_err("revoke 之后失效");
        assert!(gone.contains("重新选"), "{gone}");
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn zip_entries_are_read_by_name_and_windowed() {
        let app = temp_app_data("zip");
        let state = FsState::new(app.clone());
        let bytes = make_archive(&[
            ("mimetype", b"application/epub+zip", false),
            ("OEBPS/Text/ch1.xhtml", "<h1>第一章</h1>".as_bytes(), true),
            ("../evil.txt", b"traversal", true),
        ]);
        let file = write_archive_file(&app, "book.epub", &bytes);
        let issued = state.grant("probe", file.clone()).expect("发句柄");
        assert!(issued.archive, "epub 该认成压缩包");
        let names: Vec<String> = state
            .entries("probe", &issued.handle)
            .expect("列目录表")
            .into_iter()
            .map(|item| item.name)
            .collect();
        assert_eq!(names.len(), 3, "{names:?}");
        // 条目名里的 `../` 只当标识符：它照样能读出来，但绝不会被拼成文件系统路径。
        assert!(names.contains(&"../evil.txt".to_string()), "{names:?}");
        let body = state
            .read_member("probe", &issued.handle, "OEBPS/Text/ch1.xhtml", 0, 64)
            .expect("读条目");
        assert_eq!(String::from_utf8_lossy(&body), "<h1>第一章</h1>");
        let size = state
            .member_size("probe", &issued.handle, "OEBPS/Text/ch1.xhtml")
            .expect("条目大小");
        assert_eq!(size as usize, "<h1>第一章</h1>".len(), "{size}");
        // 窗口切法与非压缩包的反驳都要说清。
        let part = state
            .read_member("probe", &issued.handle, "mimetype", 0, 12)
            .expect("stored 条目");
        assert_eq!(String::from_utf8_lossy(&part), "application/");
        let missing = state
            .read_member("probe", &issued.handle, "nope.txt", 0, 4)
            .expect_err("没有这一项");
        assert!(missing.contains("没有这一项"), "{missing}");
        let not_zip = write_archive_file(&app, "plain.bin", b"not a zip");
        let other = state.grant("probe", not_zip).expect("发句柄");
        assert!(!other.archive);
        let err = state.entries("probe", &other.handle).expect_err("非压缩包");
        assert!(err.contains("不是宿主认得出"), "{err}");
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn zip_bomb_shapes_are_refused_not_decompressed() {
        let app = temp_app_data("bomb");
        let state = FsState::new(app.clone());
        // 一坨零：deflate 压得极狠，压缩比远超上限 —— 头部声明的 size 也是真的。
        let payload = vec![0u8; 8 * 1024 * 1024];
        let bytes = make_archive(&[("big.bin", payload.as_slice(), true)]);
        let file = write_archive_file(&app, "bomb.zip", &bytes);
        let issued = state.grant("probe", file).expect("发句柄");
        let err = state
            .read_member("probe", &issued.handle, "big.bin", 0, 1024)
            .expect_err("炸弹该拒");
        assert!(err.contains("压缩比") || err.contains("上限"), "{err}");
        // 窗口大小不影响这条判定（它是「要不要开始解」的闸，不是「读多少」的闸）。
        let again = state
            .read_member("probe", &issued.handle, "big.bin", 0, 16)
            .expect_err("同样该拒");
        assert!(again.contains("压缩比") || again.contains("上限"), "{again}");
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn window_math_never_runs_past_the_end() {
        let bytes: Vec<u8> = (0u8..10).collect();
        let total = bytes.len() as u64;
        assert_eq!(window_of(&bytes, total, 0, 4).expect("头"), vec![0, 1, 2, 3]);
        assert_eq!(window_of(&bytes, total, 7, 4).expect("尾被夹"), vec![7, 8, 9]);
        assert!(window_of(&bytes, total, total, 4).expect("过尾").is_empty());
        // 巨大 len 只给到窗口上限，不会按 len 分配。
        assert_eq!(window_of(&bytes, total, 0, u64::MAX).expect("夹上限").len(), 10);
    }
}
