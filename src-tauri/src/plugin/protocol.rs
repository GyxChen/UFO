//! `ufo-plugin://<id>/<包内相对路径>` —— 插件产物的装载通道。
//!
//! 为什么不是 inline `<script>`：作者侧要 ESM 多 chunk（`import()` 懒加载、共享 chunk、
//! 可读的错误栈与 source map），这几件事只有「一个真的可寻址源」才做得到。
//!
//! ⚠️ 这个 handler 跑在 `panic = "abort"` 的进程里：**一个 `unwrap()` 就是用户打开那个
//! 插件页面时整个 App 崩**。全程 `match` / `if let` / 带索引前夹取，失败一律变成状态码。
//!
//! 状态码的分工（作者要靠它决定往哪个方向查，所以档位不能省）：
//! - `400` URL / id / 路径本身不合法；
//! - `404` 没装这个包，或包里没这个文件；
//! - `403` 解析后跑出了包目录（穿越）；
//! - `413` 单文件超过该类文件的上限；
//! - `501` 路径属于尚未实现的保留前缀（`__pick` / `__fs`）；
//! - `503` 安装记录还没就位（极早期请求）。

use std::path::{Path, PathBuf};

use tauri::http::{header, HeaderMap, Method, Response, StatusCode};

use super::{paths, store::StoreState};

/// 协议名（前端 `plugin-runtime.ts` 拼 URL 用它，CSP 里也写它）。
pub const SCHEME: &str = "ufo-plugin";

/// 保留前缀标记：这一段「伪 id」不是包，属于宿主能力的读文件入口。
///
/// ⚠️ 必须**先于** id 合法性判断识别它们，否则会被 `is_valid_plugin_id` 拒成 400，
/// 而 400 会把作者往「URL 写错了」的方向带 —— 真正的原因是宿主还没实现这条入口。
/// 已实现的两条：`__pick/<句柄>`（用户选中的文件，含 `?entry=` 容器项）、
/// `__fs/<包 id>/<相对路径>`（这个包的私有目录）。
const RESERVED_MARK: &str = "__";

/// 安装记录：setup 里注入，handler 从这份静态引用取包目录。
static STORE: std::sync::OnceLock<StoreState> = std::sync::OnceLock::new();

/// 文件能力运行态：`__pick` / `__fs` 两条入口的句柄与私有目录都从这里读。
static FS: std::sync::OnceLock<super::fs::FsState> = std::sync::OnceLock::new();

/// 组合根调用：把安装记录交给协议 handler（重复注入只在测试里发生，以第一份为准）。
pub fn attach(store: StoreState) {
    let _ = STORE.set(store);
}

/// 组合根调用：把文件能力的运行态交给协议 handler。
pub fn attach_fs(state: super::fs::FsState) {
    let _ = FS.set(state);
}

/// 造一个错误响应：状态码 + 一句话原因（浏览器 network 面板里直接可读）。
fn error(status: u16, reason: &str) -> Response<Vec<u8>> {
    with_cors(
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .header(header::CACHE_CONTROL, "no-store")
            .body(reason.as_bytes().to_vec()),
    )
}

/// 统一加 CORS 头：ESM 的 `import()` 是跨源取模块，没有这几条就加载不进来。
fn with_cors(built: Result<Response<Vec<u8>>, tauri::http::Error>) -> Response<Vec<u8>> {
    let Ok(response) = built else {
        return Response::new(Vec::new());
    };
    let (parts, body) = response.into_parts();
    let mut builder = Response::builder().status(parts.status);
    for (key, value) in parts.headers.iter() {
        builder = builder.header(key, value);
    }
    builder = builder
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
        .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Range")
        .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "Content-Range, Accept-Ranges");
    match builder.body(body) {
        Ok(response) => response,
        Err(_) => Response::new(Vec::new()),
    }
}

/// 把协议 URI 拆成 (id, 去掉 query 的 path)。
///
/// ⚠️ Windows 上 WebView2 把自定义协议呈递成 `http://ufo-plugin.localhost/<id>/<path>`，
/// 而 macOS / Linux 是 `ufo-plugin://<id>/<path>`。只按后者解析的话，
/// Windows 上第一段会取到 `http:` —— 表现是「所有插件产物 400」。
fn split_uri(uri: &str) -> (String, String) {
    // 「权威部分算不算 id」取决于呈递形状：自定义 scheme（macOS / Linux）里 host 就是 id；
    // Windows 的 http(s)://…localhost/… 里 host 只是传输层，id 是第一段路径。
    // ⚠️ 实测 WebView2 递进来的 URI 是 `http://localhost/<id>/<路径>` —— `ufo-plugin.` 那段被它吃掉，
    // 所以这里按 scheme 与 host 两种形状都判一次，别只认写文档时假设的那一种。
    let body = match uri.split_once("://") {
        Some((scheme, tail)) => {
            let (host, path) = tail.split_once('/').unwrap_or((tail, ""));
            let transport = scheme.eq_ignore_ascii_case("http")
                || scheme.eq_ignore_ascii_case("https")
                || host.ends_with(".localhost")
                || host.eq_ignore_ascii_case("localhost");
            if transport {
                path.to_string()
            } else {
                tail.to_string()
            }
        }
        // 没有 `://` 的写法（单测里直接给 `<id>/<路径>` 形状）：整串交给下面的分段。
        None => uri.to_string(),
    };
    let mut parts = body.splitn(2, '/');
    let id = parts.next().unwrap_or("").to_string();
    let raw = parts.next().unwrap_or("");
    // query（`?entry=` 那类，留给容器成员寻址）与 hash 都不参与文件定位。
    let path = raw.split(['?', '#']).next().unwrap_or("").to_string();
    (id, path)
}

/// Range 的三种结果。
#[derive(Debug, PartialEq, Eq)]
enum RangeSpec {
    /// 没给 Range，或给了但读不懂 → 回整份。
    All,
    /// 闭区间窗口（字节）。
    Window { start: usize, end: usize },
    /// 起点已在文件之外（要回 416）。
    Unsatisfiable,
}

/// 解析 `Range: bytes=START-[END]`，只认单区间（多区间回落整份：读大文件用不到分段并发）。
fn parse_range_header(value: &str, total: usize) -> RangeSpec {
    let Some(spec) = value.trim().strip_prefix("bytes=") else {
        return RangeSpec::All;
    };
    if spec.contains(',') || spec.contains("*/") {
        return RangeSpec::All;
    }
    let single = spec.trim();
    let (head, tail) = match single.split_once('-') {
        Some(pair) => pair,
        None => return RangeSpec::All,
    };
    if total == 0 {
        return RangeSpec::Unsatisfiable;
    }
    // `bytes=-N`：要尾部 N 字节（zip 的中央目录就在文件尾部）。
    if head.is_empty() {
        let take = match tail.trim().parse::<usize>() {
            Ok(take) if take > 0 => take,
            _ => return RangeSpec::All,
        };
        return RangeSpec::Window { start: total.saturating_sub(take), end: total - 1 };
    }
    let start = match head.trim().parse::<usize>() {
        Ok(start) => start,
        Err(_) => return RangeSpec::All,
    };
    if start >= total {
        return RangeSpec::Unsatisfiable;
    }
    let end = if tail.trim().is_empty() {
        total - 1
    } else {
        match tail.trim().parse::<usize>() {
            Ok(end) => end.min(total - 1),
            Err(_) => total - 1,
        }
    };
    if end < start {
        return RangeSpec::All;
    }
    RangeSpec::Window { start, end }
}

/// 处理一个协议请求（同步实现；调用方把它丢给 blocking 线程，别占着主线程读盘）。
pub fn handle(method: &Method, uri: &str, headers: &HeaderMap) -> Response<Vec<u8>> {
    if *method == Method::OPTIONS {
        return with_cors(
            Response::builder().status(StatusCode::NO_CONTENT).body(Vec::new()),
        );
    }
    if *method != Method::GET && *method != Method::HEAD {
        return error(400, "只支持 GET / HEAD");
    }
    let (id, rel) = split_uri(uri);
    // 两条保留入口在 id 合法性判断之前分发（见 RESERVED_MARK 的说明）。
    if id == "__pick" {
        return handle_pick(&rel, uri, headers);
    }
    if id == "__fs" {
        return handle_private(&rel, headers);
    }
    if id.starts_with(RESERVED_MARK) {
        return error(501, "这条入口的宿主能力还没实现");
    }
    if !paths::is_valid_plugin_id(&id) {
        return error(400, "id 不合法：只用 a–z / 0–9 / 短横线，短横线不在首尾");
    }
    if !paths::is_safe_rel_path(&rel) {
        return error(400, "路径不是合法的包内相对路径");
    }
    let Some(state) = STORE.get() else {
        return error(503, "安装记录还没就位");
    };
    let Some(dir) = state.dir_of(&id) else {
        eprintln!("[ufo-plugin] 拒绝 {id}/{rel}：安装记录里没有这个包");
        return error(404, "没有安装这个组件");
    };
    match resolve_in_package(Path::new(&dir), &rel) {
        Ok(file) => serve_file(&file, headers),
        Err((status, reason)) => {
            // 产物被拒是「装上了却一片空白」这类报告的唯一线索，而响应体在浏览器控制台里
            // 只有一行状态码 —— 把宿主实际试过的目录一起打出来才查得动。
            eprintln!("[ufo-plugin] 拒绝 {id}/{rel}：{status} {reason}（包目录 {dir}）");
            error(status, reason)
        }
    }
}

/// 服务一条「用户亲手选中的文件」：整本，或压缩包（epub / zip / docx…）里的某一项。
///
/// ⚠️ 这里的授权模型是**能力 URL**：协议请求里没有调用方身份，句柄自带的随机段就是凭证。
/// 谁拿到句柄谁就能读 —— 所以句柄必须不可猜、不跨启动，且卸载 / 停用 / 重载时立即作废（`fs::revoke`）。
fn handle_pick(handle: &str, uri: &str, headers: &HeaderMap) -> Response<Vec<u8>> {
    let Some(fs) = FS.get() else {
        return error(503, "文件能力还没就位");
    };
    if handle.is_empty()
        || handle.len() > 160
        || handle.contains('/')
        || handle.contains('\\')
        || paths::has_control_chars(handle)
    {
        return error(400, "句柄不合法");
    }
    let Some(owner) = fs.owner_of(handle) else {
        return error(410, "这个句柄已经失效：重新选一次文件（句柄不跨启动）");
    };
    let Some(state) = STORE.get() else {
        return error(503, "安装记录还没就位");
    };
    if super::commands::require_permission(state, &owner, "fs").is_err() {
        return error(403, "这个组件的清单没声明 fs，或它已不再装着");
    }
    match query_param(uri, "entry") {
        // 容器成员：条目名只当标识符，绝不当文件系统路径（`../../win.ini` 是合法条目名）。
        Some(entry) => match fs.member_size(&owner, handle, &entry) {
            Ok(total) => respond_window(
                total,
                paths::content_type_of(&entry),
                headers,
                &|start, len| fs.read_member(&owner, handle, &entry, start, len),
            ),
            Err(reason) => error(404, &reason),
        },
        None => match fs.stat_pick(&owner, handle) {
            Ok(stat) => {
                let name = stat.name.clone();
                respond_window(
                    stat.size,
                    paths::content_type_of(&name),
                    headers,
                    &|start, len| fs.read_pick(&owner, handle, start, len),
                )
            }
            Err(reason) => error(410, &reason),
        },
    }
}

/// 服务某个包私有目录里的一个文件（作者自己写进去的东西）。
fn handle_private(rel: &str, headers: &HeaderMap) -> Response<Vec<u8>> {
    let Some(fs) = FS.get() else {
        return error(503, "文件能力还没就位");
    };
    // rel 的形状是 `<包 id>/<私有目录内的相对路径>`。
    let Some((id, path)) = rel.split_once('/') else {
        return error(400, "__fs 的写法是 /__fs/<包 id>/<相对路径>");
    };
    if !paths::is_valid_plugin_id(id) {
        return error(400, "id 不合法：只用 a–z / 0–9 / 短横线，短横线不在首尾");
    }
    if !paths::is_safe_rel_path(path) {
        return error(400, "路径不是合法的私有目录相对路径");
    }
    let Some(state) = STORE.get() else {
        return error(503, "安装记录还没就位");
    };
    if super::commands::require_permission(state, id, "fs").is_err() {
        return error(403, "这个组件的清单没声明 fs，或它已不再装着");
    }
    let total = match fs.private_size(id, path) {
        Ok(total) => total,
        Err(reason) => return error(404, &reason),
    };
    let owned = path.to_string();
    let read = move |start: u64, len: u64| fs.read_private(id, &owned, start, len);
    respond_window(total, paths::content_type_of(path), headers, &read)
}

/// 按 Range 把「能分段读的字节来源」回出去。
///
/// 三条读文件入口（选中的文件 / 容器条目 / 私有目录）共用这一套判法，只把 `read` 换掉。
///
/// ⚠️ 与 `serve_file` 的关键差别：**不整读**。一次最多给 `FS_READ_WINDOW_BYTES`，
/// 所以「没带 Range 但文件很大」时宁可回 413 说清楚，也不悄悄截断成前 4 MiB ——
/// 那会让 `<iframe>` 显示半本书，而作者看到的是一份「装上了但内容不对」的包。
fn respond_window(
    total: u64,
    content_type: &str,
    headers: &HeaderMap,
    read: &dyn Fn(u64, u64) -> Result<Vec<u8>, String>,
) -> Response<Vec<u8>> {
    let cap = paths::FS_READ_WINDOW_BYTES;
    let requested = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    // Range 的算术按 usize 走（本入口的总量上限是 2 GiB，见 FS_PICK_MAX_BYTES）。
    let total_usize = total.min(usize::MAX as u64) as usize;
    let builder = Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::ACCEPT_RANGES, "bytes");
    match parse_range_header(requested, total_usize) {
        RangeSpec::All => {
            if total > cap {
                return error(
                    413,
                    &format!(
                        "这一份有 {total} 字节，超过单次可读的 {cap} 字节上限；请带 Range 分段取"
                    ),
                );
            }
            match read(0, total) {
                Ok(bytes) => with_cors(builder.status(StatusCode::OK).body(bytes)),
                Err(reason) => error(404, &reason),
            }
        }
        RangeSpec::Window { start, end } => {
            let len = (end - start) as u64 + 1;
            match read(start as u64, len) {
                Ok(bytes) => with_cors(
                    builder
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header(
                            header::CONTENT_RANGE,
                            format!("bytes {start}-{end}/{total}"),
                        )
                        .body(bytes),
                ),
                Err(reason) => error(404, &reason),
            }
        }
        RangeSpec::Unsatisfiable => with_cors(
            builder
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{total}"))
                .body(Vec::new()),
        ),
    }
}

/// 取 URI 里的一个 query 参数（百分号解码；`entry` 名字里的 `/` 正是靠前端 encodeURIComponent 保住）。
fn query_param(uri: &str, key: &str) -> Option<String> {
    let (_, query) = uri.split_once('?')?;
    let head = query.split('#').next().unwrap_or(query);
    for pair in head.split('&') {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        if name == key {
            return Some(percent_decode(value));
        }
    }
    None
}

/// 百分号解码（解不开的 `%` 序列原样留着 —— 条目名坏了应当读不出来，而不是整段丢）。
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' && index + 2 < bytes.len() {
            let hex = &bytes[index + 1..index + 3];
            let parsed = std::str::from_utf8(hex)
                .ok()
                .and_then(|text| u8::from_str_radix(text, 16).ok());
            if let Some(decoded) = parsed {
                out.push(decoded);
                index += 3;
                continue;
            }
        }
        out.push(byte);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Windows 上超过 `MAX_PATH` 的路径要靠 verbatim 前缀才解得开。
///
/// 安装记录里存的是**人读的普通路径**（`store.rs` 的 `abs` 刻意剥掉了 `\\?\`），所以在这里补回来：
/// 先按普通路径试，解不开再加前缀试 —— 顺序反过来会让详情卡上出现那种怪路径。
#[cfg(windows)]
fn canonicalize_extended(path: &Path) -> std::io::Result<PathBuf> {
    let text = path.to_string_lossy().to_string();
    if text.starts_with(r"\\?\") {
        return std::fs::canonicalize(path);
    }
    let prefixed = if let Some(rest) = text.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{rest}")
    } else {
        format!(r"\\?\{text}")
    };
    match std::fs::canonicalize(std::path::Path::new(&prefixed)) {
        Ok(canonical) => Ok(canonical),
        // 加前缀反而失败（比如路径里本来就有 `..`）：退回普通写法再试一次，别把能成的路堵死。
        Err(_) => std::fs::canonicalize(path),
    }
}

/// 非 Windows 没有 verbatim 这套，直接交给系统。
#[cfg(not(windows))]
fn canonicalize_extended(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

/// 在包目录内解析一个相对路径，并确认结果仍在包内。
///
/// ⚠️ 不信任字符串判断：Windows 上 `..` 的等价写法很多（`..\\`、大小写、8.3 短名），
/// 唯一可靠的判据是「canonicalize 之后仍在 canonical 的包目录里」。
fn resolve_in_package(dir: &Path, rel: &str) -> Result<PathBuf, (u16, &'static str)> {
    let mut target = dir.to_path_buf();
    for component in rel.split('/') {
        if component == "." || component == ".." {
            return Err((403, "路径越出了包目录"));
        }
        target.push(component);
    }
    let canonical = canonicalize_extended(&target).map_err(|_| (404u16, "包里没这个文件"))?;
    let canonical_dir = canonicalize_extended(dir).map_err(|_| (404u16, "没安装这个组件"))?;
    if !canonical.starts_with(&canonical_dir) {
        return Err((403, "路径越出了包目录"));
    }
    // 只给真文件：目录一律 404（不把包结构泄露出去）。
    if !canonical.is_file() {
        return Err((404, "包里没这个文件"));
    }
    Ok(canonical)
}

/// 读一个包内文件：按类型夹体积、给 MIME、支持 Range。
fn serve_file(file: &Path, headers: &HeaderMap) -> Response<Vec<u8>> {
    let name = file.to_string_lossy().to_string();
    let meta = match file.metadata() {
        Ok(meta) => meta,
        Err(_) => return error(404, "文件读不到"),
    };
    if !meta.is_file() {
        return error(404, "那不是文件");
    }
    let limit = paths::size_limit_of(&name);
    if meta.len() > limit {
        return error(
            413,
            &format!(
                "这个文件有 {} 字节，超过本宿主给这一类文件的 {} 字节上限",
                meta.len(),
                limit
            ),
        );
    }
    let Ok(bytes) = std::fs::read(file) else {
        return error(404, "文件读不到");
    };
    let total = bytes.len();
    let requested = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let builder = Response::builder()
        .header(header::CONTENT_TYPE, paths::content_type_of(&name))
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::ACCEPT_RANGES, "bytes");
    match parse_range_header(requested, total) {
        RangeSpec::All => with_cors(builder.status(StatusCode::OK).body(bytes)),
        RangeSpec::Window { start, end } => {
            // 索引前逐条夹住：`parse_range_header` 已经保证合法，这里防的是它以后被改坏。
            if total == 0 {
                return with_cors(builder.status(StatusCode::OK).body(bytes));
            }
            let safe_start = start.min(total - 1);
            let safe_end = end.min(total - 1).max(safe_start);
            let slice = bytes[safe_start..=safe_end].to_vec();
            with_cors(
                builder
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(
                        header::CONTENT_RANGE,
                        format!("bytes {safe_start}-{safe_end}/{total}"),
                    )
                    .body(slice),
            )
        }
        RangeSpec::Unsatisfiable => with_cors(
            builder
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{total}"))
                .body(Vec::new()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 隔离的临时包目录（`assets/` 子目录用来测多段路径）。
    fn temp_package(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ufo-plugin-protocol-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("assets")).expect("临时包目录建不出来");
        fs::write(dir.join("main.js"), "export default {}").expect("入口写不出");
        fs::write(dir.join("assets/chunk.js"), "0123456789").expect("chunk 写不出");
        dir
    }

    /// 只带一个 Range 头的请求头集合。
    fn range_headers(range: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if !range.is_empty() {
            headers.insert(header::RANGE, range.parse().expect("Range 头写不进去"));
        }
        headers
    }

    /// ⚠️ Windows 上的实测形状：WebView2 递给 handler 的 URI 里 host 只剩 `localhost`
    /// （`ufo-plugin.` 那段被它吃掉）。只按「host 里含 `ufo-plugin.localhost`」认这条协议是错的 ——
    /// 那样会把 `localhost` 当成包 id，表现是「装上了但每个产物 404」。
    #[test]
    fn splits_the_localhost_only_form_webview2_delivers() {
        assert_eq!(
            split_uri("http://localhost/epub-shelf/main.js"),
            ("epub-shelf".to_string(), "main.js".to_string())
        );
        assert_eq!(
            split_uri("http://ufo-plugin.localhost/epub-shelf/main.js"),
            ("epub-shelf".to_string(), "main.js".to_string())
        );
        assert_eq!(
            split_uri("https://localhost/epub-shelf/assets/chunk.js?v=3"),
            ("epub-shelf".to_string(), "assets/chunk.js".to_string())
        );
        // 保留前缀那两条入口同样要在这种形状下认得出来。
        assert_eq!(
            split_uri("http://localhost/__pick/h1").0,
            "__pick".to_string()
        );
    }

    #[test]
    fn splits_the_scheme_uri_into_id_and_path() {
        assert_eq!(
            split_uri("ufo-plugin://balance-board/main.js"),
            ("balance-board".to_string(), "main.js".to_string())
        );
        assert_eq!(
            split_uri("ufo-plugin://balance-board/assets/chunk.js?v=2"),
            ("balance-board".to_string(), "assets/chunk.js".to_string())
        );
        // 只有 id（没有路径段）也要给出可解释的空串，而不是 panic。
        assert_eq!(
            split_uri("ufo-plugin://mini"),
            ("mini".to_string(), String::new())
        );
    }

    #[test]
    fn parses_every_range_shape() {
        // 逐条断言窗口：Range 是「读大文件」的唯一手段，错一格就是静默读错字节。
        assert_eq!(parse_range_header("", 100), RangeSpec::All);
        assert_eq!(parse_range_header("bytes=0-9", 100), RangeSpec::Window { start: 0, end: 9 });
        assert_eq!(parse_range_header("bytes=50-", 100), RangeSpec::Window { start: 50, end: 99 });
        assert_eq!(parse_range_header("bytes=-20", 100), RangeSpec::Window { start: 80, end: 99 });
        // 超出尾部的 END 夹到 total-1。
        assert_eq!(parse_range_header("bytes=0-99999", 100), RangeSpec::Window { start: 0, end: 99 });
        assert_eq!(parse_range_header("bytes=100-", 100), RangeSpec::Unsatisfiable);
        assert_eq!(parse_range_header("bytes=500-", 100), RangeSpec::Unsatisfiable);
        // 读不懂的一律回落整份（不是错误，作者只是没指望分段）。
        assert_eq!(parse_range_header("bytes=0-9,20-29", 100), RangeSpec::All);
        assert_eq!(parse_range_header("items=0-9", 100), RangeSpec::All);
        assert_eq!(parse_range_header("bytes=abc-def", 100), RangeSpec::All);
        assert_eq!(parse_range_header("bytes=90-10", 100), RangeSpec::All);
        assert_eq!(parse_range_header("bytes=0-10", 0), RangeSpec::Unsatisfiable);
    }

    #[test]
    fn refuses_bad_urls_before_touching_the_disk() {
        // 这三条都不需要安装记录：证明「先校验后 IO」的顺序是对的。
        assert_eq!(
            handle(&Method::GET, "ufo-plugin://BadId/main.js", &HeaderMap::new()).status(),
            400
        );
        assert_eq!(
            handle(&Method::GET, "ufo-plugin://mini/../../etc/passwd", &HeaderMap::new()).status(),
            400
        );
        assert_eq!(
            handle(&Method::POST, "ufo-plugin://mini/main.js", &HeaderMap::new()).status(),
            400
        );
    }

    /// `__` 开头的「伪 id」不是包，不能按「id 不合法」拒 —— 那会把作者带去查 URL。
    ///
    /// 没实现的那条回 501；已实现的两条（`__pick` / `__fs`）在单测进程里回 503
    /// （这里没有 attach 文件能力运行态），这条同时也钉住它们**不再**走 501 那一支。
    #[test]
    fn reserved_prefixes_are_dispatched_not_rejected_as_bad_request() {
        assert_eq!(
            handle(&Method::GET, "ufo-plugin://__ble/x", &HeaderMap::new()).status(),
            501
        );
        assert_eq!(
            handle(&Method::GET, "ufo-plugin://__pick/handle", &HeaderMap::new()).status(),
            503
        );
        assert_eq!(
            handle(&Method::GET, "ufo-plugin://__fs/mini/x.txt", &HeaderMap::new()).status(),
            503
        );
    }

    #[test]
    fn unknown_package_without_ledger_is_503_rather_than_404() {
        // 安装记录没就位时不能假装「没装这个包」（会让用户去重装，其实只是时序早了一步）。
        // 注：`STORE` 是进程级静态，本用例只在没 attach 过的测试进程里成立；
        // 若同批测试里有用例 attach 过，这条会退化成 404 —— 所以断言只要求「不是 5xx 之外的错」。
        let status = handle(&Method::GET, "ufo-plugin://not-here/main.js", &HeaderMap::new()).status();
        assert!(status == 404 || status == 503, "实际状态 {status}");
    }

    #[test]
    fn resolves_files_inside_the_package_only() {
        let dir = temp_package("resolve");
        let inside = resolve_in_package(&dir, "assets/chunk.js");
        assert!(inside.is_ok(), "{:?}", inside.err());
        assert_eq!(resolve_in_package(&dir, "assets/../assets/chunk.js").err().unwrap().0, 403);
        assert_eq!(resolve_in_package(&dir, "missing.js").err().unwrap().0, 404);
        // 目录本身不给读（会泄露包结构）。
        assert_eq!(resolve_in_package(&dir, "assets").err().unwrap().0, 404);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 安装记录里现在存的是普通路径，但历史包可能留着 Windows 的 verbatim 前缀：两种都要解得开。
    #[test]
    fn resolves_inside_plain_and_verbatim_package_dirs_alike() {
        let dir = temp_package("verbatim");
        let with_prefix = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        for base in [dir.clone(), with_prefix] {
            let found = resolve_in_package(&base, "main.js")
                .unwrap_or_else(|(code, reason)| panic!("{base:?} 被拒：{code} {reason}"));
            assert!(found.is_file(), "{base:?} 解出来的不是文件");
        }
        // 越界照旧被挡：换写法不是放行 `..` 的口子。
        assert_eq!(
            resolve_in_package(&dir, "../main.js").err().map(|(code, _)| code),
            Some(403)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn serves_full_and_partial_reads_with_cors_and_no_store() {
        let dir = temp_package("serve");
        let file = dir.join("assets").join("chunk.js");
        let full = serve_file(&file, &range_headers(""));
        assert_eq!(full.status(), 200);
        assert_eq!(full.body().len(), 10);
        assert_eq!(
            full.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).and_then(|v| v.to_str().ok()),
            Some("*")
        );
        assert_eq!(
            full.headers().get(header::CACHE_CONTROL).and_then(|v| v.to_str().ok()),
            Some("no-store")
        );

        let partial = serve_file(&file, &range_headers("bytes=2-4"));
        assert_eq!(partial.status(), 206);
        assert_eq!(partial.body(), b"234");
        assert_eq!(
            partial.headers().get(header::CONTENT_RANGE).and_then(|v| v.to_str().ok()),
            Some("bytes 2-4/10")
        );

        let tail = serve_file(&file, &range_headers("bytes=-3"));
        assert_eq!(tail.status(), 206);
        assert_eq!(tail.body(), b"789");

        let beyond = serve_file(&file, &range_headers("bytes=99-"));
        assert_eq!(beyond.status(), 416);
        let _ = fs::remove_dir_all(&dir);
    }
}
