//! 插件包在盘上的落点、id 与路径的规则，以及全部体积上限。
//!
//! ⚠️ 这里的每个常量都是**契约的一部分**：前端 `src/lib/plugin-types.ts` 手抄一份字面量，
//! 由 `src/lib/plugin-limits.spec.ts` 与本文件逐条对撞（双向：改 Rust 没改 TS 会红，
//! Rust 新增一条上限而 TS 没登记也会红）。抬帽的口径是「兜作者手滑」而不是安全边界 ——
//! 防越界靠能力声明 + 装前逐条披露，不靠这些数字。

use std::path::{Path, PathBuf};

/// 宿主插件契约的版本：只有「注入的 API 面 / 清单字段语义」变了才递增。
/// 旧包声明的版本比宿主高 → 拒装（装了也跑不起来，越早说越好）。
pub const PLUGIN_API_VERSION: u32 = 1;
/// 清单文件自身的格式版本；没做不兼容的结构改动就不要动它。
pub const MANIFEST_VERSION: u32 = 1;

/// 包目录里那份声明的文件名。
pub const MANIFEST_FILE: &str = "plugin.json";
/// app-data 下存放所有已装包的目录名。
pub const PLUGINS_DIR: &str = "plugins";
/// 包目录里存放该插件日志的目录名（只在调试开着的时候写）。
pub const LOG_DIR: &str = "logs";
/// 安装记录文件名：记「装了哪些包、从哪来、是不是在调试」。
pub const LEDGER_FILE: &str = "ledger.json";

/// 清单本身的上限：每次冷扫描都要读它，所以刻意不给更大的值。
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
/// 单个脚本产物（ESM chunk）的上限。
pub const MAX_SCRIPT_BYTES: u64 = 8 * 1024 * 1024;
/// 单个样式文件的上限。
pub const MAX_STYLE_BYTES: u64 = 1024 * 1024;
/// 单个数据文件（json / 文本）的上限。
pub const MAX_DATA_BYTES: u64 = 2 * 1024 * 1024;
/// 单个图片的上限。
pub const MAX_IMAGE_BYTES: u64 = 4 * 1024 * 1024;
/// 一个包最多多少个文件（冷扫描的 IO 预算）。
pub const MAX_FILES: usize = 256;
/// 一个包的总体积上限。
pub const MAX_TOTAL_BYTES: u64 = 48 * 1024 * 1024;

/// 插件 id 的最大长度（同时是目录名，过长会让管理区一行放不下）。
pub const MAX_ID_CHARS: usize = 40;
/// 展示名的最大长度：它会直接渲染成一行文本。
pub const MAX_NAME_CHARS: usize = 40;
/// 说明文案的最大长度。
pub const MAX_DESCRIPTION_CHARS: usize = 200;
/// 版本号字符串的最大长度。
pub const MAX_VERSION_CHARS: usize = 32;
/// 包内相对路径的最大长度。
pub const MAX_REL_PATH_CHARS: usize = 160;

/// 悬浮窗格子的下限（1 格是物理下限，0 也按 1 格算）。
pub const MIN_CELLS: u32 = 1;
/// 悬浮窗格子的上限：45px × 16 已经是最大形态，这是物理约束不是手滑帽。
pub const MAX_CELLS: u32 = 16;
/// 插入块（往别的组件信息卡尾部追加）的行数上限：与提示窗一屏预算同源。
pub const MAX_EXTENSION_ROWS: u32 = 3;
/// 插件自己信息卡宽度的夹取区间（物理像素）。
pub const MIN_TOOLTIP_WIDTH: u32 = 220;
/// 插件自己信息卡宽度的上限（清单声明与运行时 setSize 共用这一个数）。
pub const MAX_TOOLTIP_WIDTH: u32 = 420;
/// 插件自己信息卡高度的下限。
pub const MIN_TOOLTIP_HEIGHT: u32 = 120;
/// 插件自己信息卡高度的上限（与内置组件的信息卡同一档预算，提示窗再按工作区夹一次）。
pub const MAX_TOOLTIP_HEIGHT: u32 = 620;
/// 出网域名白名单的条数上限。
pub const MAX_HOSTS: usize = 64;

/// `storage` 能力的分区配额（每包）。
pub const STORAGE_PARTITION_BYTES: u64 = 8 * 1024 * 1024;
/// `storage` 单个值的上限。
pub const STORAGE_VALUE_BYTES: u64 = 256 * 1024;
/// `storage` 每包的键数上限。
pub const STORAGE_MAX_KEYS: usize = 1024;
/// `storage` 键名长度上限。
pub const STORAGE_MAX_KEY_CHARS: usize = 128;

/// `http` 响应体上限（脚本代取的数据）。
pub const HTTP_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

/// 事件环形缓冲每包多少条（拉得慢会丢最旧的，`oldestSeq` 会如实说明从哪开始丢）。
pub const EVENT_RING_LINES: usize = 256;
/// 事件巡检间隔（毫秒）：与悬浮窗快照广播同一节奏，组件不该看到「灯已变但事件还没到」。
pub const EVENT_POLL_INTERVAL_MS: u64 = 1_000;

/// 日志环形缓冲的行数（调试卡的「最近日志」从这里取，独立日志窗翻页读盘）。
pub const LOG_RING_LINES: usize = 200;
/// 单条日志的字符上限，超出直接截断（防一条无限长的 dump 把整窗刷死）。
pub const LOG_LINE_CHARS: usize = 2000;
/// 每包日志文件的上限（到顶滚动，不再追加）。
pub const LOG_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// 独立日志窗一次翻页取多少行。
pub const LOG_PAGE_LINES: usize = 200;

/// 组件私有目录的目录名（在 `plugins/<id>/` 下，与 `logs/` 同级）。
pub const FS_DIR: &str = "files";
/// 一次写入的字节上限（IPC 只能带这么多，且 base64 后更大）。
pub const FS_WRITE_BYTES: u64 = 8 * 1024 * 1024;
/// 一个包私有目录的上限。
pub const FS_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
/// 所有包私有目录的合计上限：单包帽只挡得住「一个包写太多」，挡不住「装二十个包各写满」。
pub const FS_GLOBAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// 一次读窗口（URL 的 Range、`fs.read` 的一刀）上限。
pub const FS_READ_WINDOW_BYTES: u64 = 4 * 1024 * 1024;
/// 用户选中的文件最大体积：超过就当「选错了」，不做半截服务。
pub const FS_PICK_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// 一个包同时能握着几个「选中的文件」句柄。
pub const PICK_HANDLES_PER_PACKAGE: usize = 8;
/// 整个 App 的句柄总数帽。
pub const PICK_HANDLES_TOTAL: usize = 64;
/// 压缩包目录表最多列几项（超了就说明这包不该用这个口子读）。
pub const ZIP_ENTRIES_MAX: usize = 5000;
/// 单个条目解压后的上限（压缩炸弹的第一道）。
pub const ZIP_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
/// 解压后 / 压缩后的最大比例（第二道：头部写着 4 GiB 但实际只占 2 MiB 的那种）。
pub const ZIP_RATIO_MAX: u32 = 200;

/// 插件 id 的合法字符：小写字母 / 数字 / 短横线，且短横线不能在首尾。
///
/// id 同时用作包目录名与配置里的键，所以这里的规则就是文件系统的规则 ——
/// 不接受大写字母是为了避免 Windows（大小写不敏感）与 Linux 上出现两个「同一个包」。
pub fn is_valid_plugin_id(id: &str) -> bool {
    if id.is_empty() || id.len() > MAX_ID_CHARS {
        return false;
    }
    if id.starts_with('-') || id.ends_with('-') || id.contains("--") {
        return false;
    }
    id.bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// 判断一个「包内相对路径」是否安全。
///
/// 允许多段（ESM 的 chunk 常带 `assets/` 前缀），但必须挡掉：
/// 绝对路径、盘符、`\`、任何一段 `..`、空段、NUL 与控制字符、以 `.` 开头的保留名段。
/// 这条判断是「读边界」的全部强度 —— 包内有什么文件名不再是闸（见 guide 的读边界一节）。
pub fn is_safe_rel_path(path: &str) -> bool {
    if path.is_empty() || path.len() > MAX_REL_PATH_CHARS {
        return false;
    }
    if path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return false;
    }
    if path.chars().any(|c| (c as u32) < 0x20) {
        return false;
    }
    // 盘符形式（`C:` / `c:/`）即使没有分隔符也要挡掉。
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && (bytes[0].is_ascii_alphabetic()) {
        return false;
    }
    let mut segments = path.split('/').peekable();
    while let Some(segment) = segments.next() {
        if segment.is_empty() {
            return false;
        }
        if segment == "." || segment == ".." {
            return false;
        }
        if !segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        {
            return false;
        }
    }
    true
}

/// 把窗口格数夹进 [MIN_CELLS, MAX_CELLS]；非有限值由调用方在更早的阶段处理。
pub fn clamp_cells(cells: u32) -> u32 {
    cells.clamp(MIN_CELLS, MAX_CELLS)
}

/// 把插入块行数夹进 [1, MAX_EXTENSION_ROWS]（一屏预算是硬约束，不给更多）。
pub fn clamp_rows(rows: u32) -> u32 {
    rows.clamp(1, MAX_EXTENSION_ROWS)
}

/// 把插件声明的信息卡宽度夹进区间。
pub fn clamp_tooltip_width(width: u32) -> u32 {
    width.clamp(MIN_TOOLTIP_WIDTH, MAX_TOOLTIP_WIDTH)
}

/// 把插件声明的信息卡高度夹进区间。
pub fn clamp_tooltip_height(height: u32) -> u32 {
    height.clamp(MIN_TOOLTIP_HEIGHT, MAX_TOOLTIP_HEIGHT)
}

/// 按长度截断字符串并保证落在字符边界上（作者给的文案可能是中文）。
pub fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// 拒掉含控制字符的展示文本：它们会原样进 DOM，光标位置一变就像「名字里多了空格」。
pub fn has_control_chars(value: &str) -> bool {
    value.chars().any(|c| c.is_control())
}

/// app-data 下所有包的根目录。
pub fn plugins_root(app_data: &Path) -> PathBuf {
    app_data.join(PLUGINS_DIR)
}

/// 某个包的目录（id 已由调用方校验过）。
pub fn package_dir(app_data: &Path, id: &str) -> PathBuf {
    plugins_root(app_data).join(id)
}

/// 某个包的日志目录（`plugins/<id>/logs`）。
pub fn plugin_log_dir(app_data: &Path, id: &str) -> PathBuf {
    package_dir(app_data, id).join(LOG_DIR)
}

/// 某个包的**私有目录**（`plugins/<id>/files`）—— 组件唯一能写任意字节的地方。
///
/// ⚠️ 这里刻意不用记录里的 `dir`：本地安装的包 `dir` 指向作者的源码目录，
/// 组件写文件绝不该落进别人的工程树里。私有目录恒在 app-data 下，与包的来源无关。
pub fn plugin_files_dir(app_data: &Path, id: &str) -> PathBuf {
    package_dir(app_data, id).join(FS_DIR)
}

/// 某个包的安装记录文件路径。
pub fn ledger_path(app_data: &Path) -> PathBuf {
    plugins_root(app_data).join(LEDGER_FILE)
}

/// 按扩展名猜 MIME：只用于**给响应头**，不再是「装不装得上」的闸。
///
/// 认不出的扩展名回 `application/octet-stream` —— 包里有陌生文件是作者的自由，
/// 只要它不越过路径边界。
pub fn content_type_of(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    match lower.rsplit('.').next().unwrap_or("") {
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "html" | "htm" => "text/html; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "map" => "application/json; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 该文件按哪种体积上限算（脚本 / 样式 / 数据 / 图片 / 其它）。
pub fn size_limit_of(path: &str) -> u64 {
    let mime = content_type_of(path);
    if mime.starts_with("text/javascript") {
        MAX_SCRIPT_BYTES
    } else if mime.starts_with("text/css") {
        MAX_STYLE_BYTES
    } else if mime.starts_with("image/") || mime.starts_with("font/") {
        MAX_IMAGE_BYTES
    } else {
        MAX_DATA_BYTES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ids_a_package_can_actually_be() {
        for id in ["balance-board", "a", "clock24", "ci-timing-pro"] {
            assert!(is_valid_plugin_id(id), "{id} 应当合法");
        }
    }

    #[test]
    fn rejects_ids_that_would_break_the_directory_contract() {
        // 每条都断言「为什么被拒」，而不是只断言「被拒」：
        // 大写会让 Windows 与 Linux 认成两个包，`_`/`.` 会造出与保留名撞车的目录。
        assert!(!is_valid_plugin_id(""));
        assert!(!is_valid_plugin_id("Balance"));
        assert!(!is_valid_plugin_id("my_plugin"));
        assert!(!is_valid_plugin_id("a/b"));
        assert!(!is_valid_plugin_id(".."));
        assert!(!is_valid_plugin_id("-lead"));
        assert!(!is_valid_plugin_id("trail-"));
        assert!(!is_valid_plugin_id("double--dash"));
        assert!(!is_valid_plugin_id("中文"));
        assert!(!is_valid_plugin_id(&"a".repeat(MAX_ID_CHARS + 1)));
    }

    #[test]
    fn allows_multi_segment_relative_paths() {
        // ESM 的 chunk 带目录前缀是常态，单段规则会把正常构建产物判成坏包。
        assert!(is_safe_rel_path("main.js"));
        assert!(is_safe_rel_path("assets/index-a1b2c3.js"));
        assert!(is_safe_rel_path("chunk.v2.min.js"));
    }

    #[test]
    fn blocks_every_path_traversal_shape() {
        // 逐个断言形状：只测 `../` 的护栏等于没护栏。
        assert!(!is_safe_rel_path("../secret.txt"));
        assert!(!is_safe_rel_path("assets/../../secret.txt"));
        assert!(!is_safe_rel_path("/etc/passwd"));
        assert!(!is_safe_rel_path("C:\\Windows\\win.ini"));
        assert!(!is_safe_rel_path("C:/Windows/win.ini"));
        assert!(!is_safe_rel_path("a\\b.js"));
        assert!(!is_safe_rel_path("a//b.js"));
        assert!(!is_safe_rel_path("./a.js"));
        assert!(!is_safe_rel_path("a\0.js"));
        assert!(!is_safe_rel_path("a<b.js"));
        assert!(!is_safe_rel_path(&"a".repeat(MAX_REL_PATH_CHARS + 1)));
    }

    #[test]
    fn clamps_numbers_instead_of_rejecting_the_package() {
        assert_eq!(clamp_cells(0), MIN_CELLS);
        assert_eq!(clamp_cells(999), MAX_CELLS);
        assert_eq!(clamp_rows(99), MAX_EXTENSION_ROWS);
        assert_eq!(clamp_tooltip_width(10), MIN_TOOLTIP_WIDTH);
        assert_eq!(clamp_tooltip_width(9999), MAX_TOOLTIP_WIDTH);
        assert_eq!(clamp_tooltip_height(9999), MAX_TOOLTIP_HEIGHT);
    }

    #[test]
    fn truncation_stays_on_character_boundaries() {
        assert_eq!(truncate_chars("余额看板 v1.2.0", 4), "余额看板");
        // 截到多字节字符中间会切出半个字符，这里必须整字丢弃。
        assert_eq!(truncate_chars("ab界", 2), "ab");
    }

    #[test]
    fn control_characters_in_display_text_are_rejected() {
        assert!(has_control_chars("bad\tname"));
        assert!(!has_control_chars("余额看板"));
    }

    #[test]
    fn mime_guess_never_blocks_a_package() {
        // 陌生扩展名要能装上（返回 octet-stream 而不是「不认」）。
        assert_eq!(
            content_type_of("data.bin"),
            "application/octet-stream"
        );
        assert_eq!(content_type_of("assets/x.CSS"), "text/css; charset=utf-8");
        assert_eq!(size_limit_of("chunk.js"), MAX_SCRIPT_BYTES);
        assert_eq!(size_limit_of("a.png"), MAX_IMAGE_BYTES);
        assert_eq!(size_limit_of("a.json"), MAX_DATA_BYTES);
    }

    #[test]
    fn paths_are_built_under_the_app_data_plugins_root() {
        let dir = Path::new("app-data");
        assert_eq!(
            package_dir(dir, "balance-board"),
            PathBuf::from("app-data").join("plugins").join("balance-board")
        );
        assert_eq!(
            plugin_log_dir(dir, "x"),
            PathBuf::from("app-data").join("plugins").join("x").join("logs")
        );
    }
}
