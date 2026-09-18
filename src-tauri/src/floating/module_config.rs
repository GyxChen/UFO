use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

const SCHEMA_VERSION: u16 = 1;
const MIN_CELLS: u8 = 4;
const MAX_CELLS: u8 = 8;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FloatingModuleEntry {
    pub id: String,
    pub enabled: bool,
    pub order: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct MaxCellsByLayout {
    pub square: u8,
    pub corner: u8,
    pub horizontal: u8,
    pub vertical: u8,
}

impl MaxCellsByLayout {
    fn filled(value: u8) -> Self {
        let cells = value.clamp(MIN_CELLS, MAX_CELLS);
        Self {
            square: cells,
            corner: cells,
            horizontal: cells,
            vertical: cells,
        }
    }

    fn clamp_each(mut self) -> Self {
        self.square = self.square.clamp(MIN_CELLS, MAX_CELLS);
        self.corner = self.corner.clamp(MIN_CELLS, MAX_CELLS);
        self.horizontal = self.horizontal.clamp(MIN_CELLS, MAX_CELLS);
        self.vertical = self.vertical.clamp(MIN_CELLS, MAX_CELLS);
        self
    }
}

impl Default for MaxCellsByLayout {
    fn default() -> Self {
        Self::filled(MAX_CELLS)
    }
}

/// 所有组件的配置块：键 = 组件 id，值 = 该组件自己解释的 JSON 对象。
///
/// 外壳**不认识**里面任何字段（读写都整块透传），因此新增一个组件的配置项
/// 既不用改外壳，也不用改本文件 —— 组件自己在 `module_settings` 下建自己的键。
pub type ModuleSettingsMap = BTreeMap<String, Value>;

/// 组件配置的「认领」钩子（组合根登记，外壳在配置刚读出来时逐条调用）。
///
/// 唯一用途是**迁移旧版扁平字段**：外壳的配置文件曾经把某个组件的字段平铺在顶层，
/// 现在要收进该组件自己的配置块。旧字段叫什么名字只有组件自己知道，所以由组件在
/// 组合根登记这个回调自己认领，外壳只负责把它不认识的顶层字段原样递过来。
///
/// - `settings`：整份 [`ModuleSettingsMap`]（组件只该碰自己那个键）
/// - `legacy`：顶层多出来的未知字段
/// - 返回：是否认领了内容（外壳据此决定要不要把清理过的配置写回磁盘）
pub type ModuleSettingsMigration = fn(&mut ModuleSettingsMap, &BTreeMap<String, Value>) -> bool;

/// 配置变更钩子（组合根登记，外壳在配置落盘后逐条调用）。
///
/// 外壳不认识任何组件字段，也不知道「配置改了要通知谁」；组件自己在这里把新配置
/// 翻译成自己的运行时状态（如把超时推给状态监测），从而做到改完即时生效。
pub type ModuleConfigHook = Box<dyn Fn(&AppHandle, &FloatingModuleConfig) + Send + Sync>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FloatingModuleConfig {
    pub schema_version: u16,
    pub revision: u64,
    /// 组件的启用状态与排序（仅此两项与具体组件无关，故留在外壳）。
    pub modules: Vec<FloatingModuleEntry>,
    /// 各形态的格子上限：那是**整个悬浮窗**的尺寸参数，同级组件共享，故留在外壳。
    pub max_cells_by_layout: MaxCellsByLayout,
    /// 各组件自己的配置块（见 [`ModuleSettingsMap`]）。
    #[serde(default)]
    pub module_settings: ModuleSettingsMap,
}

/// 加载期过渡形态：接受旧的 `max_visible_sessions` 与旧的顶层组件字段，
/// 写出时两者都不再保留（组件字段收进 `module_settings`）。
#[derive(Debug, Deserialize)]
struct FloatingModuleConfigWire {
    schema_version: u16,
    revision: u64,
    modules: Vec<FloatingModuleEntry>,
    #[serde(default)]
    max_cells_by_layout: Option<MaxCellsByLayout>,
    #[serde(default)]
    max_visible_sessions: Option<u8>,
    /// 组件自己的配置块（当前形态）。
    #[serde(default)]
    module_settings: ModuleSettingsMap,
    /// 顶层多出来的字段（旧版把组件字段平铺在这里）。外壳不理解它们，
    /// 原样交给各组件的认领钩子（见 [`ModuleSettingsMigration`]）。
    #[serde(flatten)]
    legacy: BTreeMap<String, Value>,
}

impl FloatingModuleConfigWire {
    /// 转成正式配置：先让各组件认领旧字段，再夹取外壳自己的取值。
    ///
    /// 返回值第二项 = 是否有旧字段被认领，调用方据此决定要不要把清理后的配置写回磁盘。
    fn into_config(self, migrations: &[ModuleSettingsMigration]) -> (FloatingModuleConfig, bool) {
        let max_cells_by_layout = match self.max_cells_by_layout {
            Some(cells) => cells.clamp_each(),
            None => MaxCellsByLayout::filled(self.max_visible_sessions.unwrap_or(MAX_CELLS)),
        };
        let mut module_settings = self.module_settings;
        let mut adopted = false;
        for migration in migrations {
            // 用 `|=` 而不是短路：某个组件认领了，后面的组件也还是得看一眼。
            adopted |= migration(&mut module_settings, &self.legacy);
        }
        (
            FloatingModuleConfig {
                schema_version: self.schema_version,
                revision: self.revision,
                modules: self.modules,
                max_cells_by_layout,
                module_settings,
            },
            adopted,
        )
    }
}

impl Default for FloatingModuleConfig {
    /// 兜底配置不列任何组件：启用状态由前端注册表里的 `defaultEnabled` 决定，
    /// 外壳不需要（也不应该）点名任何具体组件。
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            revision: 1,
            modules: Vec::new(),
            max_cells_by_layout: MaxCellsByLayout::default(),
            module_settings: ModuleSettingsMap::new(),
        }
    }
}

impl FloatingModuleConfig {
    /// 校验并夹取**外壳自己认识**的字段；组件配置块整块透传，不在这里解释。
    pub fn validate(mut self) -> Result<Self, String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err("unsupported module config schema".to_string());
        }
        self.max_cells_by_layout = self.max_cells_by_layout.clamp_each();
        for module in &self.modules {
            if module.id.len() > 64 || module.id.contains(['/', '\\']) {
                return Err("invalid module id".to_string());
            }
        }
        Ok(self)
    }
}

/// 把配置原子地写到 `path`（先写 `.tmp` 再改名，避免留下半截文件）。
fn persist(path: &Path, config: &FloatingModuleConfig) -> Result<(), String> {
    let encoded = serde_json::to_vec_pretty(config).map_err(|error| error.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, encoded).map_err(|error| error.to_string())?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    fs::rename(&temporary, path).map_err(|error| error.to_string())?;
    Ok(())
}

#[derive(Clone)]
pub struct ModuleState {
    pub config: Arc<Mutex<FloatingModuleConfig>>,
    pub path: PathBuf,
    /// 组合根登记的配置变更钩子（`Mutex` 让 `ModuleState` 保持 `Clone` 且登记不必可变借用）。
    hooks: Arc<Mutex<Vec<ModuleConfigHook>>>,
}

impl ModuleState {
    /// 读盘 + 迁移。
    ///
    /// `migrations` 是组合根登记的组件认领钩子（见 [`ModuleSettingsMigration`]）；
    /// 有旧字段被认领时把清理后的配置**写回一次** —— 旧字段从此不再出现在文件里。
    pub fn load(path: PathBuf, migrations: &[ModuleSettingsMigration]) -> Self {
        let loaded = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<FloatingModuleConfigWire>(&bytes).ok())
            .map(|wire| wire.into_config(migrations))
            .and_then(|(config, adopted)| config.validate().ok().map(|config| (config, adopted)));
        let (config, adopted) = loaded.unwrap_or_default();
        if adopted {
            // 迁移写回：内容对使用者没变，因此不 bump revision、不广播，只是把旧字段清掉。
            let _ = persist(&path, &config);
        }
        Self {
            config: Arc::new(Mutex::new(config)),
            path,
            hooks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 取当前配置的副本（组合根启动时用它把已落盘的配置推给各组件）。
    pub fn snapshot(&self) -> FloatingModuleConfig {
        self.config
            .lock()
            .map(|config| config.clone())
            .unwrap_or_default()
    }

    /// 登记配置变更钩子（组合根调用）。可登记多个，写盘后按登记顺序调用。
    pub fn on_config_change(&self, hook: ModuleConfigHook) {
        if let Ok(mut hooks) = self.hooks.lock() {
            hooks.push(hook);
        }
    }

    /// 把新配置逐个交给已登记的钩子（外壳只负责转发，不解释内容）。
    fn notify(&self, app: &AppHandle, config: &FloatingModuleConfig) {
        let Ok(hooks) = self.hooks.lock() else {
            return;
        };
        for hook in hooks.iter() {
            hook(app, config);
        }
    }
}

#[tauri::command]
pub fn get_floating_module_config(state: State<'_, ModuleState>) -> FloatingModuleConfig {
    state.snapshot()
}

#[tauri::command]
pub fn set_floating_module_config(
    app: AppHandle,
    state: State<'_, ModuleState>,
    mut config: FloatingModuleConfig,
) -> Result<FloatingModuleConfig, String> {
    let current_revision = state
        .config
        .lock()
        .map_err(|_| "module config lock poisoned".to_string())?
        .revision;
    config.revision = current_revision.saturating_add(1);
    config = config.validate()?;
    persist(&state.path, &config)?;

    *state
        .config
        .lock()
        .map_err(|_| "module config lock poisoned".to_string())? = config.clone();
    // 通知各组件：外壳不知道谁要这些配置，由组件自己在钩子里取用（如把超时推给状态监测）。
    state.notify(&app, &config);
    let _ = app.emit("floating-module-config", &config);
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用认领钩子：把顶层 `stub_value` 收进 `stub` 组件自己的配置块。
    fn adopt_stub(settings: &mut ModuleSettingsMap, legacy: &BTreeMap<String, Value>) -> bool {
        let Some(value) = legacy.get("stub_value") else {
            return false;
        };
        settings.insert("stub".to_string(), serde_json::json!({ "value": value }));
        true
    }

    #[test]
    fn default_config_lists_no_modules() {
        // 外壳不点名任何组件：启用状态由前端注册表的 defaultEnabled 决定。
        let config = FloatingModuleConfig::default();
        assert_eq!(config.schema_version, SCHEMA_VERSION);
        assert!(config.modules.is_empty());
        assert!(config.module_settings.is_empty());
    }

    #[test]
    fn default_max_cells_are_eight() {
        let config = FloatingModuleConfig::default();
        assert_eq!(config.max_cells_by_layout, MaxCellsByLayout::filled(MAX_CELLS));
    }

    #[test]
    fn migrates_legacy_max_visible_sessions() {
        let json = r#"{
            "schema_version": 1,
            "revision": 2,
            "modules": [{"id":"stub","enabled":true,"order":0}],
            "max_visible_sessions": 5
        }"#;
        let wire: FloatingModuleConfigWire = serde_json::from_str(json).unwrap();
        let (config, _) = wire.into_config(&[]);
        let config = config.validate().unwrap();
        assert_eq!(config.max_cells_by_layout, MaxCellsByLayout::filled(5));
    }

    #[test]
    fn prefers_max_cells_by_layout_over_legacy_field() {
        let json = r#"{
            "schema_version": 1,
            "revision": 2,
            "modules": [{"id":"stub","enabled":true,"order":0}],
            "max_visible_sessions": 5,
            "max_cells_by_layout": {
                "square": 4,
                "corner": 6,
                "horizontal": 8,
                "vertical": 7
            }
        }"#;
        let wire: FloatingModuleConfigWire = serde_json::from_str(json).unwrap();
        let (config, _) = wire.into_config(&[]);
        let config = config.validate().unwrap();
        assert_eq!(
            config.max_cells_by_layout,
            MaxCellsByLayout {
                square: 4,
                corner: 6,
                horizontal: 8,
                vertical: 7,
            }
        );
    }

    #[test]
    fn validate_clamps_max_cells_by_layout() {
        let mut config = FloatingModuleConfig::default();
        config.max_cells_by_layout = MaxCellsByLayout {
            square: 0,
            corner: 255,
            horizontal: 3,
            vertical: 6,
        };
        let validated = config.validate().unwrap();
        assert_eq!(validated.max_cells_by_layout.square, MIN_CELLS);
        assert_eq!(validated.max_cells_by_layout.corner, MAX_CELLS);
        assert_eq!(validated.max_cells_by_layout.horizontal, MIN_CELLS);
        assert_eq!(validated.max_cells_by_layout.vertical, 6);
    }

    #[test]
    fn validate_rejects_wrong_schema_version() {
        let mut config = FloatingModuleConfig::default();
        config.schema_version = 99;
        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_rejects_module_id_with_slash() {
        let mut config = FloatingModuleConfig::default();
        config.modules.push(FloatingModuleEntry {
            id: "bad/id".to_string(),
            enabled: true,
            order: 1,
        });
        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_rejects_module_id_with_backslash() {
        let mut config = FloatingModuleConfig::default();
        config.modules.push(FloatingModuleEntry {
            id: "bad\\id".to_string(),
            enabled: true,
            order: 1,
        });
        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_rejects_overlong_module_id() {
        let mut config = FloatingModuleConfig::default();
        config.modules.push(FloatingModuleEntry {
            id: "a".repeat(65),
            enabled: true,
            order: 1,
        });
        assert!(config.validate().is_err());
    }

    #[test]
    fn module_settings_roundtrip_untouched() {
        // 外壳不认识组件字段：整块透传，原样存回。
        let mut config = FloatingModuleConfig::default();
        config
            .module_settings
            .insert("stub".to_string(), serde_json::json!({"answer": 42}));
        let json = serde_json::to_string(&config).unwrap();
        // 外壳只写自己的字段 + 组件配置块，旧版扁平字段不再写出。
        assert!(json.contains("module_settings"));
        assert!(!json.contains("max_visible_sessions"));
        let deserialized: FloatingModuleConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.schema_version, config.schema_version);
        assert_eq!(
            deserialized.module_settings.get("stub"),
            Some(&serde_json::json!({"answer": 42}))
        );
    }

    #[test]
    fn unknown_top_level_fields_are_handed_to_migrations() {
        // 旧版扁平字段交给组件自己的认领钩子；外壳完全不知道它叫什么。
        let json = r#"{
            "schema_version": 1,
            "revision": 3,
            "modules": [{"id":"stub","enabled":true,"order":0}],
            "stub_value": "legacy"
        }"#;
        let wire: FloatingModuleConfigWire = serde_json::from_str(json).unwrap();
        let (config, adopted) = wire.into_config(&[adopt_stub]);
        assert!(adopted);
        assert_eq!(
            config.module_settings.get("stub"),
            Some(&serde_json::json!({"value": "legacy"}))
        );
    }

    #[test]
    fn load_migrates_legacy_fields_and_rewrites_the_file_once() {
        let dir = std::env::temp_dir().join("ufo-test-module-config-migration");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("floating-modules.json");
        fs::write(
            &path,
            r#"{
                "schema_version": 1,
                "revision": 4,
                "modules": [{"id":"stub","enabled":true,"order":0}],
                "stub_value": "legacy"
            }"#,
        )
        .unwrap();

        let state = ModuleState::load(path.clone(), &[adopt_stub]);
        let config = state.snapshot();
        assert_eq!(
            config.module_settings.get("stub"),
            Some(&serde_json::json!({"value": "legacy"}))
        );
        // 写回后文件里不再有旧字段（组件配置块接管了它）。
        let rewritten = fs::read_to_string(&path).unwrap();
        assert!(rewritten.contains("module_settings"));
        assert!(!rewritten.contains("stub_value"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn module_state_loads_default_on_missing_file() {
        let dir = std::env::temp_dir().join("ufo-test-missing-config");
        let _ = fs::remove_dir_all(&dir);
        let state = ModuleState::load(dir.join("floating-modules.json"), &[]);
        let config = state.snapshot();
        assert_eq!(config.schema_version, SCHEMA_VERSION);
        assert!(config.modules.is_empty());
        assert_eq!(config.max_cells_by_layout, MaxCellsByLayout::default());
    }
}
