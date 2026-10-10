//! 清单 `plugin.json` 的形状与校验。
//!
//! 校验哲学（与 UI 上「装不上时要说清为什么」直接对应）：
//! **结构性错误拒装**（给了就没法正常工作：坏 id、越界路径、未知能力名），
//! **数字一律夹取**（超范围只让它自己受限：格子数、信息卡尺寸、文案长度）。
//! 每条 `Err` 的文案都会原样出现在管理区那一行，所以要说清「哪儿不对 + 该写成什么」。

use serde::{Deserialize, Serialize};

use super::{capability, paths};

/// 清单本体：外壳只认这一份声明，包里的其它文件都不参与「能碰什么」的判定。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Manifest {
    /// 清单格式版本（结构不兼容才递增）。
    pub manifest_version: u32,
    /// 该包要求的宿主 API 版本；比宿主新就拒装。
    pub api_version: u32,
    /// 包 id：同时是目录名、配置键与存储分区名。
    pub id: String,
    /// 管理区显示名。
    pub name: String,
    /// 展示用的版本号（`x.y.z`，可带预发布后缀）。
    pub version: String,
    /// 一句话说明，管理区列表行与详情卡都要渲染它。
    pub description: String,
    /// ESM 入口（包内相对路径），装载就是 `import()` 它。
    pub entry: String,
    /// 界面挂载点的声明（与「能力」是两回事：这里只说插在哪，不授权碰任何东西）。
    pub surfaces: Surfaces,
    /// 宿主能力名列表；未登记的名字 = 拒装。
    pub permissions: Vec<String>,
    /// 出网域名白名单，只在声明 `http` 时生效。
    pub hosts: Vec<String>,
}

impl Default for Manifest {
    /// 缺省值：老包不写版本就当当前版本。
    fn default() -> Self {
        Self {
            manifest_version: paths::MANIFEST_VERSION,
            api_version: paths::PLUGIN_API_VERSION,
            id: String::new(),
            name: String::new(),
            version: String::new(),
            description: String::new(),
            entry: String::new(),
            surfaces: Surfaces::default(),
            permissions: Vec::new(),
            hosts: Vec::new(),
        }
    }
}

/// 界面挂载点的声明。四项都是「有 / 没有」，格子数与信息卡尺寸带数字。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Surfaces {
    /// 悬浮窗里申请几格；`None` = 不占格子（只在信息卡 / 设置页出现的组件）。
    pub cell: Option<u32>,
    /// 自己的独立信息卡（悬停插件自己的格子时弹）。
    pub tooltip: Option<OwnTooltip>,
    /// 往别的组件信息卡尾部追加一块。
    pub extension: Option<Extension>,
    /// 主窗一级页签里是否有自己的设置页。
    pub settings: bool,
}

/// `surfaces.tooltip`：独立信息卡。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OwnTooltip {
    /// 显式开关：写了 `tooltip` 但 `own: false` 等于没声明（避免「写了空对象就以为有卡」）。
    pub own: bool,
    /// 期望窗口尺寸 `[宽, 高]`（物理像素）；省略 = 用宿主默认。外壳夹进工作区。
    pub size: Option<[u32; 2]>,
}

/// `surfaces.extension`：插入块。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Extension {
    /// 插进哪个组件的信息卡（目标组件 id）。
    pub into: String,
    /// 最多贡献几行；上限由提示窗的一屏预算决定。
    pub max_rows: u32,
}

impl Default for Extension {
    /// 不给行数就按预算给满（3 行）。
    fn default() -> Self {
        Self {
            into: String::new(),
            max_rows: paths::MAX_EXTENSION_ROWS,
        }
    }
}

/// 校验结果：夹取过的清单 + 需要在界面上如实告知的告警。
#[derive(Debug, Clone)]
pub struct Validated {
    /// 已把数字夹进合法区间的清单（落盘与装载都用它）。
    pub manifest: Manifest,
    /// 非致命问题（截断、多余声明等），管理区详情里要显示。
    pub warnings: Vec<String>,
}

/// 从文本解析清单：JSON 语法错误变成中文消息（作者第一眼看到的就是这句）。
pub fn parse(text: &str) -> Result<Manifest, String> {
    serde_json::from_str::<Manifest>(text)
        .map_err(|error| format!("清单不是合法 JSON 或字段类型不对：{error}"))
}

/// 解析 + 校验的一入口（安装与扫描台账都走这条）。
pub fn from_text(text: &str) -> Result<Validated, String> {
    validate(parse(text)?)
}

/// 版本号：`主.次.补丁`，可带 `-预发布` 与 `+构建` 后缀。
///
/// 只做形状校验，不做 semver 比较 —— 本轮刻意不解析 range（那是 npm 安装器的入口，
/// 尽头是依赖求解）。
fn is_valid_version(value: &str) -> bool {
    let core = value
        .split(['-', '+'])
        .next()
        .unwrap_or_default();
    let mut parts = core.split('.');
    let (Some(major), Some(minor), Some(patch), None) = (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    [major, minor, patch]
        .iter()
        .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// 校验一个域名单元：接受 `api.example.com`，拒绝带 scheme / 路径 / 端口 / 空标签的形式。
fn is_valid_host(raw: &str) -> bool {
    let host = raw.trim();
    if host.is_empty()
        || host.len() > 253
        || host.contains('/')
        || host.contains(':')
        || host.contains('@')
        || host.contains('*')
    {
        return false;
    }
    host.split('.')
        .all(|label| !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

/// 校验并夹取一份清单。
pub fn validate(input: Manifest) -> Result<Validated, String> {
    let mut manifest = input;
    let mut warnings: Vec<String> = Vec::new();

    if manifest.manifest_version > paths::MANIFEST_VERSION {
        return Err(format!(
            "清单版本 {} 比本宿主支持的 {} 新，装不上",
            manifest.manifest_version, paths::MANIFEST_VERSION
        ));
    }
    manifest.manifest_version = paths::MANIFEST_VERSION;

    if manifest.api_version > paths::PLUGIN_API_VERSION {
        return Err(format!(
            "该组件需要宿主 API v{}，当前宿主是 v{}",
            manifest.api_version, paths::PLUGIN_API_VERSION
        ));
    }
    manifest.api_version = paths::PLUGIN_API_VERSION;

    if !paths::is_valid_plugin_id(&manifest.id) {
        return Err(format!(
            "id「{}」不合法：只能用 a–z / 0–9 / 短横线，短横线不在首尾且不连续，长度 ≤{}",
            manifest.id, paths::MAX_ID_CHARS
        ));
    }

    if paths::has_control_chars(&manifest.name) {
        return Err("name 含控制字符（制表 / 换行 / 零宽），管理区那一行会显示成残缺文本".to_string());
    }
    if manifest.name.trim().is_empty() {
        warnings.push("name 为空，管理区改用 id 显示".to_string());
        manifest.name = manifest.id.clone();
    } else {
        manifest.name = manifest.name.trim().to_string();
    }
    let name_chars = manifest.name.chars().count();
    if name_chars > paths::MAX_NAME_CHARS {
        manifest.name = paths::truncate_chars(&manifest.name, paths::MAX_NAME_CHARS);
        warnings.push(format!(
            "name 从 {} 字截到 {} 字",
            name_chars, paths::MAX_NAME_CHARS
        ));
    }

    if manifest.version.trim().is_empty() || !is_valid_version(manifest.version.trim()) {
        return Err(format!(
            "version「{}」不是 x.y.z 形式（如 1.2.0，可带 -rc.1 后缀）",
            manifest.version
        ));
    }
    manifest.version = manifest.version.trim().to_string();
    if manifest.version.chars().count() > paths::MAX_VERSION_CHARS {
        manifest.version = paths::truncate_chars(&manifest.version, paths::MAX_VERSION_CHARS);
        warnings.push("version 过长已截断".to_string());
    }

    manifest.description = manifest.description.trim().to_string();
    let desc_chars = manifest.description.chars().count();
    if desc_chars > paths::MAX_DESCRIPTION_CHARS {
        manifest.description = paths::truncate_chars(&manifest.description, paths::MAX_DESCRIPTION_CHARS);
        warnings.push(format!(
            "description 从 {} 字截到 {} 字",
            desc_chars, paths::MAX_DESCRIPTION_CHARS
        ));
    }

    if manifest.entry.trim().is_empty() {
        return Err("清单缺少 entry（ESM 入口的包内路径，如 \"main.js\"）".to_string());
    }
    if !paths::is_safe_rel_path(&manifest.entry) {
        return Err(format!(
            "entry「{}」不是合法的包内相对路径（禁绝对路径、盘符、`\\`、`..`、空段）",
            manifest.entry
        ));
    }
    let entry_lower = manifest.entry.to_ascii_lowercase();
    if !(entry_lower.ends_with(".js") || entry_lower.ends_with(".mjs")) {
        return Err(format!(
            "entry「{}」必须是 .js / .mjs 的 ESM 产物",
            manifest.entry
        ));
    }

    // 能力名：未登记的直接拒装（声明了就用不了的口子，比拼错一个名字更糟）。
    let mut seen: Vec<String> = Vec::new();
    for name in &manifest.permissions {
        if !capability::is_known_permission(name) {
            return Err(format!(
                "权限「{}」本宿主不认识；可用：{}",
                name,
                capability::permission_ids().join(" / ")
            ));
        }
        if !seen.contains(name) {
            seen.push(name.clone());
        } else {
            warnings.push(format!("权限「{name}」重复声明，已合并"));
        }
    }
    manifest.permissions = seen;

    let has_http = manifest.permissions.iter().any(|name| name == "http");
    let mut hosts: Vec<String> = Vec::new();
    for raw in &manifest.hosts {
        if is_valid_host(raw) {
            let host = raw.trim().to_ascii_lowercase();
            if !hosts.contains(&host) {
                hosts.push(host);
            }
        } else {
            warnings.push(format!(
                "hosts 里的「{raw}」不是域名形式（不带 scheme / 路径 / 端口），已忽略"
            ));
        }
    }
    if hosts.len() > paths::MAX_HOSTS {
        hosts.truncate(paths::MAX_HOSTS);
        warnings.push(format!(
            "hosts 超过 {} 条，只保留前面那些",
            paths::MAX_HOSTS
        ));
    }
    if has_http && hosts.is_empty() {
        return Err(
            "声明了 http 却没有可用的 hosts 白名单：这样任何域名都不会放行，功能必然失败".to_string(),
        );
    }
    if !has_http && !hosts.is_empty() {
        warnings.push("hosts 只在声明 http 时生效，本包没声明 http，已忽略".to_string());
        hosts.clear();
    }
    manifest.hosts = hosts;

    // 挂载点：数字一律夹取，只有「自相矛盾」才报错。
    if let Some(cells) = manifest.surfaces.cell {
        let clamped = paths::clamp_cells(cells);
        if clamped != cells {
            warnings.push(format!("格子数 {cells} 超出区间，按 {clamped} 格算"));
        }
        manifest.surfaces.cell = Some(clamped);
    }
    if let Some(tooltip) = manifest.surfaces.tooltip.as_mut() {
        if !tooltip.own {
            warnings.push("写了 surfaces.tooltip 但 own 为 false，独立信息卡不会出现".to_string());
        }
        if let Some([width, height]) = tooltip.size {
            let clamped = [
                paths::clamp_tooltip_width(width),
                paths::clamp_tooltip_height(height),
            ];
            if clamped[0] != width || clamped[1] != height {
                warnings.push(format!(
                    "信息卡尺寸 {width}×{height} 超出区间，夹成 {}×{}",
                    clamped[0], clamped[1]
                ));
            }
            tooltip.size = Some(clamped);
        }
    }
    if let Some(extension) = manifest.surfaces.extension.as_ref() {
        if !paths::is_valid_plugin_id(&extension.into) {
            return Err(format!(
                "surfaces.extension.into「{}」不是合法的组件 id",
                extension.into
            ));
        }
        if extension.into == manifest.id {
            return Err("surfaces.extension.into 不能指向自己（插入块是往别的组件卡里加东西）".to_string());
        }
    }
    if let Some(extension) = manifest.surfaces.extension.as_mut() {
        let clamped = paths::clamp_rows(extension.max_rows);
        if clamped != extension.max_rows {
            warnings.push(format!(
                "插入块行数 {} 超出 1–{}，按 {clamped} 行算",
                extension.max_rows, paths::MAX_EXTENSION_ROWS
            ));
        }
        extension.max_rows = clamped;
    }
    if manifest.surfaces.cell.is_none()
        && manifest.surfaces.tooltip.is_none()
        && manifest.surfaces.extension.is_none()
        && !manifest.surfaces.settings
    {
        warnings.push("没声明任何挂载点：这个包装上了也不会出现在任何窗口里".to_string());
    }

    Ok(Validated { manifest, warnings })
}

impl Manifest {
    /// 该包实际申请的挂载点 id 列表（顺序固定，管理区徽标按它渲染）。
    ///
    /// 返回的是 `capability::SURFACES` 里的同一批 id，因此前端可以直接查同一张表拿图标与文案。
    /// 只在测试里当断言助手用（正式链路读的是 `surfaces` 本体）。
    #[cfg(test)]
    pub fn declared_surfaces(&self) -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = Vec::new();
        if self.surfaces.cell.is_some() {
            ids.push("cell");
        }
        if self
            .surfaces
            .tooltip
            .as_ref()
            .is_some_and(|tooltip| tooltip.own)
        {
            ids.push("tooltip-own");
        }
        if self.surfaces.extension.is_some() {
            ids.push("tooltip-extension");
        }
        if self.surfaces.settings {
            ids.push("settings");
        }
        ids
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一份合法的最小清单（各条断言在它之上单点改动，确保红的就是那条原因）。
    fn base() -> Manifest {
        Manifest {
            id: "balance-board".to_string(),
            name: "余额看板".to_string(),
            version: "1.2.0".to_string(),
            description: "把账户余额画进悬浮窗一格".to_string(),
            entry: "main.js".to_string(),
            surfaces: Surfaces {
                cell: Some(2),
                tooltip: Some(OwnTooltip {
                    own: true,
                    size: Some([340, 260]),
                }),
                extension: Some(Extension {
                    into: "agent-status".to_string(),
                    max_rows: 3,
                }),
                settings: true,
            },
            permissions: vec!["http".to_string(), "storage".to_string()],
            hosts: vec!["api.example.com".to_string()],
            ..Manifest::default()
        }
    }

    #[test]
    fn accepts_the_shape_shown_in_the_ui_mock() {
        // 这段 JSON 与 `reference/mock/plugin-center.html` 的「06 契约」逐字对应。
        let text = r#"{
            "id": "balance-board",
            "name": "余额看板",
            "version": "1.2.0",
            "description": "把账户余额画进悬浮窗一格",
            "entry": "main.js",
            "surfaces": {
                "cell": 2,
                "tooltip": { "own": true, "size": [340, 260] },
                "extension": { "into": "agent-status", "maxRows": 3 },
                "settings": true
            },
            "permissions": [ "http", "storage" ],
            "hosts": [ "api.example.com" ]
        }"#;
        let done = from_text(text).expect("mock 里那份清单必须装得上");
        assert_eq!(done.warnings, Vec::<String>::new());
        assert_eq!(done.manifest.surfaces.cell, Some(2));
        assert_eq!(done.manifest.declared_surfaces(), vec!["cell", "tooltip-own", "tooltip-extension", "settings"]);
    }

    #[test]
    fn rejects_structural_errors_with_a_specific_reason() {
        // 逐条断言拒绝原因：只断言「被拒」是假护栏。
        let cases: Vec<(String, Manifest, &str)> = vec![
            (bad_id(), bad_id_manifest(), "id「Balance_Board」不合法"),
            (no_entry(), base_entry(""), "清单缺少 entry"),
            (unsafe_entry(), base_entry("../secret.js"), "不是合法的包内相对路径"),
            (wrong_ext(), base_entry("main.css"), "必须是 .js / .mjs"),
            (unknown_perm(), base_perm("netwrok"), "本宿主不认识"),
            (bad_version(), base_version("v1"), "不是 x.y.z 形式"),
            (self_extension(), base_ext_into("balance-board"), "不能指向自己"),
            (bad_into(), base_ext_into("Bad Into"), "不是合法的组件 id"),
            (http_without_hosts(), base_no_hosts(), "没有可用的 hosts 白名单"),
            (new_api(), base_api(99), "需要宿主 API v99"),
            (new_manifest(), base_manifest_version(9), "清单版本 9"),
        ];
        for (label, manifest, expected) in cases {
            let error = validate(manifest).expect_err("{label} 应当被拒");
            assert!(error.contains(expected), "{label} 的原因文案不对：{error}");
        }
    }

    #[test]
    fn clamps_numbers_and_warns_instead_of_refusing() {
        let mut manifest = base();
        manifest.surfaces.cell = Some(999);
        manifest.surfaces.tooltip = Some(OwnTooltip {
            own: true,
            size: Some([10, 5000]),
        });
        manifest.surfaces.extension = Some(Extension {
            into: "agent-status".to_string(),
            max_rows: 12,
        });
        let done = validate(manifest).expect("数字越界不该拒装");
        assert_eq!(done.manifest.surfaces.cell, Some(paths::MAX_CELLS));
        assert_eq!(
            done.manifest.surfaces.tooltip.unwrap().size,
            Some([paths::MIN_TOOLTIP_WIDTH, paths::MAX_TOOLTIP_HEIGHT])
        );
        assert_eq!(done.manifest.surfaces.extension.unwrap().max_rows, 3);
        assert_eq!(done.warnings.len(), 3, "三条夹取各留一条告警：{:?}", done.warnings);
    }

    #[test]
    fn trims_and_dedupes_the_list_fields() {
        let mut manifest = base();
        manifest.permissions = vec!["http".to_string(), "http".to_string(), "storage".to_string()];
        manifest.hosts = vec![
            " API.Example.com ".to_string(),
            "api.example.com".to_string(),
            "https://bad.example.com/".to_string(),
            "10.0.0.8:8080".to_string(),
        ];
        let done = validate(manifest).expect("重复与大小写不该拒装");
        assert_eq!(done.manifest.permissions, vec!["http", "storage"]);
        assert_eq!(done.manifest.hosts, vec!["api.example.com"]);
        // 一条重复权限 + 两条坏域名 = 三条告警。
        assert_eq!(done.warnings.len(), 3, "{:?}", done.warnings);
    }

    #[test]
    fn hosts_without_http_are_dropped_and_named() {
        let mut manifest = base();
        manifest.permissions = vec!["storage".to_string()];
        let done = validate(manifest).expect("多余的 hosts 只该警告");
        assert!(done.manifest.hosts.is_empty());
        assert!(done.warnings.iter().any(|w| w.contains("只在声明 http 时生效")));
    }

    #[test]
    fn a_package_with_no_surfaces_is_still_installable_but_says_so() {
        let mut manifest = base();
        manifest.surfaces = Surfaces::default();
        manifest.permissions = vec![];
        manifest.hosts.clear();
        let done = validate(manifest).expect("纯后台包装得上");
        assert!(done.warnings.iter().any(|w| w.contains("不会出现在任何窗口里")));
        assert!(done.manifest.declared_surfaces().is_empty());
    }

    #[test]
    fn tooltip_object_with_own_false_does_not_claim_a_card() {
        let mut manifest = base();
        manifest.surfaces.tooltip = Some(OwnTooltip { own: false, size: None });
        let done = validate(manifest).expect("own:false 只是不弹卡");
        assert!(!done.manifest.declared_surfaces().contains(&"tooltip-own"));
        assert!(done.warnings.iter().any(|w| w.contains("独立信息卡不会出现")));
    }

    #[test]
    fn empty_name_falls_back_to_id_with_a_warning() {
        let mut manifest = base();
        manifest.name = "   ".to_string();
        let done = validate(manifest).expect("空 name 该回落而不是拒装");
        assert_eq!(done.manifest.name, "balance-board");
        assert!(done.warnings.iter().any(|w| w.contains("name 为空")));
    }

    #[test]
    fn control_characters_in_name_are_refused() {
        let mut manifest = base();
        manifest.name = "余额\t看板".to_string();
        let error = validate(manifest).expect_err("控制字符要拒装");
        assert!(error.contains("控制字符"), "{error}");
    }

    #[test]
    fn version_shape_is_checked_without_being_a_semver_comparator() {
        for good in ["1.2.3", "0.0.1", "1.2.3-rc.1", "1.2.3+build.9"] {
            let mut manifest = base();
            manifest.version = good.to_string();
            validate(manifest).unwrap_or_else(|error| panic!("{good} 应当合法：{error}"));
        }
        for bad in ["1.2", "1.2.3.4", "v1.2.3", "1.2.x", ""] {
            let mut manifest = base();
            manifest.version = bad.to_string();
            assert!(validate(manifest).is_err(), "{bad} 应当被拒");
        }
    }

    #[test]
    fn missing_optional_fields_take_the_documented_defaults() {
        let text = r#"{ "id": "mini", "name": "迷你", "version": "1.0.0", "entry": "main.js" }"#;
        let done = from_text(text).expect("最小清单要装得上");
        assert_eq!(done.manifest.api_version, paths::PLUGIN_API_VERSION);
        assert_eq!(done.manifest.manifest_version, paths::MANIFEST_VERSION);
        assert_eq!(done.manifest.name, "迷你");
    }

    #[test]
    fn broken_json_becomes_a_readable_message_not_a_panic() {
        let error = parse("{ \"id\": }").expect_err("坏 JSON 要变成 Err");
        assert!(error.contains("不是合法 JSON"), "{error}");
        // 类型写错（cells 给了字符串）也走同一条文案，不能 panic。
        let wrong_type = parse(r#"{ "surfaces": { "cell": "two" } }"#).expect_err("字段类型错要报错");
        assert!(wrong_type.contains("字段类型不对"), "{wrong_type}");
    }

    // ── 构造各种「单点错误」的助手 ────────────────────────────────

    fn base_entry(entry: &str) -> Manifest {
        let mut manifest = base();
        manifest.entry = entry.to_string();
        manifest
    }

    fn base_perm(permission: &str) -> Manifest {
        let mut manifest = base();
        manifest.permissions = vec![permission.to_string()];
        manifest
    }

    fn base_version(version: &str) -> Manifest {
        let mut manifest = base();
        manifest.version = version.to_string();
        manifest
    }

    fn base_ext_into(into: &str) -> Manifest {
        let mut manifest = base();
        manifest.surfaces.extension = Some(Extension {
            into: into.to_string(),
            max_rows: 3,
        });
        manifest
    }

    fn base_no_hosts() -> Manifest {
        let mut manifest = base();
        manifest.hosts.clear();
        manifest
    }

    fn base_api(api: u32) -> Manifest {
        let mut manifest = base();
        manifest.api_version = api;
        manifest
    }

    fn base_manifest_version(value: u32) -> Manifest {
        let mut manifest = base();
        manifest.manifest_version = value;
        manifest
    }

    fn bad_id_manifest() -> Manifest {
        let mut manifest = base();
        manifest.id = "Balance_Board".to_string();
        manifest
    }

    // 这几个函数只为让上面的表驱动用例读起来是一行一件事。
    fn bad_id() -> String { "id".to_string() }
    fn no_entry() -> String { "entry".to_string() }
    fn unsafe_entry() -> String { "entry 越界".to_string() }
    fn wrong_ext() -> String { "entry 扩展名".to_string() }
    fn unknown_perm() -> String { "权限名".to_string() }
    fn bad_version() -> String { "version".to_string() }
    fn self_extension() -> String { "自环".to_string() }
    fn bad_into() -> String { "into 形状".to_string() }
    fn http_without_hosts() -> String { "出网无白名单".to_string() }
    fn new_api() -> String { "API 版本".to_string() }
    fn new_manifest() -> String { "清单版本".to_string() }
}
