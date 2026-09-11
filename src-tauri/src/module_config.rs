use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::status::StatusState;

const SCHEMA_VERSION: u16 = 1;
const AGENT_STATUS: &str = "agent-status";
const DEFAULT_RETENTION_MS: u64 = 30_000;
const MAX_RETENTION_MS: u64 = 24 * 60 * 60 * 1000;
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FloatingModuleConfig {
    pub schema_version: u16,
    pub revision: u64,
    pub modules: Vec<FloatingModuleEntry>,
    pub completed_retention_ms: u64,
    pub max_cells_by_layout: MaxCellsByLayout,
}

/// 加载期过渡形态：仍接受旧的 `max_visible_sessions`，写出时不再保留。
#[derive(Debug, Deserialize)]
struct FloatingModuleConfigWire {
    schema_version: u16,
    revision: u64,
    modules: Vec<FloatingModuleEntry>,
    completed_retention_ms: u64,
    #[serde(default)]
    max_cells_by_layout: Option<MaxCellsByLayout>,
    #[serde(default)]
    max_visible_sessions: Option<u8>,
}

impl FloatingModuleConfigWire {
    fn into_config(self) -> FloatingModuleConfig {
        let max_cells_by_layout = match self.max_cells_by_layout {
            Some(cells) => cells.clamp_each(),
            None => MaxCellsByLayout::filled(self.max_visible_sessions.unwrap_or(MAX_CELLS)),
        };
        FloatingModuleConfig {
            schema_version: self.schema_version,
            revision: self.revision,
            modules: self.modules,
            completed_retention_ms: self.completed_retention_ms,
            max_cells_by_layout,
        }
    }
}

impl Default for FloatingModuleConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            revision: 1,
            modules: vec![FloatingModuleEntry {
                id: AGENT_STATUS.to_string(),
                enabled: true,
                order: 0,
            }],
            completed_retention_ms: DEFAULT_RETENTION_MS,
            max_cells_by_layout: MaxCellsByLayout::default(),
        }
    }
}

impl FloatingModuleConfig {
    pub fn validate(mut self) -> Result<Self, String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err("unsupported module config schema".to_string());
        }
        self.completed_retention_ms = self.completed_retention_ms.clamp(1_000, MAX_RETENTION_MS);
        self.max_cells_by_layout = self.max_cells_by_layout.clamp_each();
        for module in &self.modules {
            if module.id.len() > 64 || module.id.contains(['/', '\\']) {
                return Err("invalid module id".to_string());
            }
        }
        if !self.modules.iter().any(|module| module.id == AGENT_STATUS) {
            self.modules.push(FloatingModuleEntry {
                id: AGENT_STATUS.to_string(),
                enabled: true,
                order: self.modules.len().min(u16::MAX as usize) as u16,
            });
        }
        Ok(self)
    }
}

#[derive(Clone)]
pub struct ModuleState {
    pub config: Arc<Mutex<FloatingModuleConfig>>,
    pub path: PathBuf,
}

impl ModuleState {
    pub fn load(path: PathBuf) -> Self {
        let config = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<FloatingModuleConfigWire>(&bytes).ok())
            .map(FloatingModuleConfigWire::into_config)
            .and_then(|config| config.validate().ok())
            .unwrap_or_default();
        Self {
            config: Arc::new(Mutex::new(config)),
            path,
        }
    }

    pub fn retention_ms(&self) -> u64 {
        self.config
            .lock()
            .map(|config| config.completed_retention_ms)
            .unwrap_or(DEFAULT_RETENTION_MS)
    }
}

#[tauri::command]
pub fn get_floating_module_config(state: State<'_, ModuleState>) -> FloatingModuleConfig {
    state
        .config
        .lock()
        .expect("module config lock poisoned")
        .clone()
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

    let encoded = serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?;
    if let Some(parent) = state.path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = state.path.with_extension("tmp");
    fs::write(&temporary, encoded).map_err(|error| error.to_string())?;
    if state.path.exists() {
        fs::remove_file(&state.path).map_err(|error| error.to_string())?;
    }
    fs::rename(&temporary, &state.path).map_err(|error| error.to_string())?;

    *state
        .config
        .lock()
        .map_err(|_| "module config lock poisoned".to_string())? = config.clone();
    if let Some(status_state) = app.try_state::<StatusState>() {
        if let Ok(mut retention_ms) = status_state.retention_ms.lock() {
            *retention_ms = config.completed_retention_ms;
        }
    }
    let _ = app.emit("floating-module-config", &config);
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_agent_status_enabled() {
        let config = FloatingModuleConfig::default();
        assert_eq!(config.schema_version, SCHEMA_VERSION);
        assert_eq!(config.modules.len(), 1);
        assert_eq!(config.modules[0].id, AGENT_STATUS);
        assert!(config.modules[0].enabled);
    }

    #[test]
    fn default_retention_is_thirty_seconds() {
        let config = FloatingModuleConfig::default();
        assert_eq!(config.completed_retention_ms, 30_000);
    }

    #[test]
    fn default_max_cells_are_eight() {
        let config = FloatingModuleConfig::default();
        assert_eq!(config.max_cells_by_layout, MaxCellsByLayout::filled(MAX_CELLS));
    }

    #[test]
    fn validate_clamps_retention_to_range() {
        let mut config = FloatingModuleConfig::default();
        config.completed_retention_ms = 0;
        let validated = config.validate().unwrap();
        assert_eq!(validated.completed_retention_ms, 1_000);

        let mut config = FloatingModuleConfig::default();
        config.completed_retention_ms = u64::MAX;
        let validated = config.validate().unwrap();
        assert_eq!(validated.completed_retention_ms, MAX_RETENTION_MS);
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
    fn validate_adds_agent_status_if_missing() {
        let mut config = FloatingModuleConfig::default();
        config.modules.clear();
        let validated = config.validate().unwrap();
        assert!(validated.modules.iter().any(|m| m.id == AGENT_STATUS));
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
    fn config_serialization_roundtrip() {
        let config = FloatingModuleConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("max_visible_sessions"));
        let deserialized: FloatingModuleConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.schema_version, config.schema_version);
        assert_eq!(deserialized.modules.len(), config.modules.len());
        assert_eq!(deserialized.max_cells_by_layout, config.max_cells_by_layout);
    }

    #[test]
    fn migrates_legacy_max_visible_sessions() {
        let json = r#"{
            "schema_version": 1,
            "revision": 2,
            "modules": [{"id":"agent-status","enabled":true,"order":0}],
            "completed_retention_ms": 30000,
            "max_visible_sessions": 5
        }"#;
        let wire: FloatingModuleConfigWire = serde_json::from_str(json).unwrap();
        let config = wire.into_config().validate().unwrap();
        assert_eq!(config.max_cells_by_layout, MaxCellsByLayout::filled(5));
    }

    #[test]
    fn prefers_max_cells_by_layout_over_legacy_field() {
        let json = r#"{
            "schema_version": 1,
            "revision": 2,
            "modules": [{"id":"agent-status","enabled":true,"order":0}],
            "completed_retention_ms": 30000,
            "max_visible_sessions": 5,
            "max_cells_by_layout": {
                "square": 4,
                "corner": 6,
                "horizontal": 8,
                "vertical": 7
            }
        }"#;
        let wire: FloatingModuleConfigWire = serde_json::from_str(json).unwrap();
        let config = wire.into_config().validate().unwrap();
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
    fn module_state_loads_default_on_missing_file() {
        let dir = std::env::temp_dir().join("ufo-test-missing-config");
        let _ = fs::remove_dir_all(&dir);
        let state = ModuleState::load(dir.join("floating-modules.json"));
        let config = state.config.lock().unwrap();
        assert_eq!(config.schema_version, SCHEMA_VERSION);
        assert!(config.modules.iter().any(|m| m.id == AGENT_STATUS));
        assert_eq!(config.max_cells_by_layout, MaxCellsByLayout::default());
    }
}
