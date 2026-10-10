//! 组件的密钥口子：值只进出 OS 凭据管理器（Windows = 凭据管理器 / macOS = Keychain / Linux = Secret Service）。
//!
//! 三条规矩：
//! 1. **一格一包**：service 名是 `<App 标识>/plugin/<id>`，与内置模块的 `<App 标识>/flow/<endpoint>` 分开，
//!    A 包读不到 B 包的名字，也读不到流程脚本配的那些。
//! 2. **值不落盘**：盘上只留「这个名字被这个包用过」的名册（[`paths`] 旁边的 `secrets.json`），
//!    用来在卸载时把这些条目一并删掉 —— 凭据管理器不能按 service 枚举，不留名册就会留下孤儿密钥。
//! 3. **不进日志、不回显**：这一层没有任何打印路径，界面上也只显示名字不显示值（前端那边同样只做名字）。
//!
//! ⚠️ 复用内置流程脚本那套 `SecretSource` / `validate_secret_name`，不再写第二份取数逻辑：
//! 两套实现迟早对不上，而对不上的表现是「同一个名字在两边读到不同的东西」。

use std::path::{Path, PathBuf};

use crate::modules::agent_status::flow::{validate_secret_name, KeyringSecrets, SecretSource};
use crate::modules::agent_status::status::APP_IDENTIFIER;

use super::paths;

/// 名册文件名（在 `plugins/<id>/` 下，与 `files/`、`logs/` 同级）。
const NAMES_FILE: &str = "secrets.json";

/// 一个密钥值的上限：Windows 凭据管理器对 generic password 有自己的帽，
/// 超了它报的是一句看不出所以然的话，所以在这里先给一句人话。
pub const MAX_VALUE_BYTES: usize = 4096;

/// 本包在凭据管理器里的 service 名。
///
/// id 已过 `is_valid_plugin_id`（只含小写字母 / 数字 / 短横线），不需要再清洗分隔符。
pub fn service_for(plugin_id: &str) -> String {
    format!("{APP_IDENTIFIER}/plugin/{plugin_id}")
}

/// 运行态：只需要知道 app-data 在哪（名册落那儿）。
#[derive(Clone, Default)]
pub struct SecretState {
    app_data: PathBuf,
}

impl SecretState {
    /// 用 app-data 建运行态。
    pub fn new(app_data: PathBuf) -> Self {
        Self { app_data }
    }

    /// 按名字取值；没配过回 `None`（不是错误 —— 组件据此提示用户去填）。
    pub fn get(&self, plugin_id: &str, name: &str) -> Result<Option<String>, String> {
        self.get_with(&KeyringSecrets, plugin_id, name)
    }

    /// 写值并记名（`host.secrets.set`）。
    pub fn put(&self, plugin_id: &str, name: &str, value: &str) -> Result<(), String> {
        self.put_with(&KeyringSecrets, plugin_id, name, value)
    }

    /// 删值并去名（`host.secrets.remove`）。
    pub fn remove(&self, plugin_id: &str, name: &str) -> Result<(), String> {
        self.remove_with(&KeyringSecrets, plugin_id, name)
    }

    /// 这个包存过哪些名字（只回名字；组件设置页据此显示「已配置 / 未配置」）。
    pub fn names(&self, plugin_id: &str) -> Result<Vec<String>, String> {
        if !paths::is_valid_plugin_id(plugin_id) {
            return Err(format!("id「{plugin_id}」不合法"));
        }
        read_names(&self.names_path(plugin_id))
    }

    /// 卸载：按名册把这一包的条目全删掉，再删名册。
    ///
    /// 单条删不掉不阻塞后面的（凭据管理器可能已经被人手动清过），
    /// 但会把第一句原因带回界面 —— 静默成功会让人以为密钥没了。
    pub fn forget(&self, plugin_id: &str) -> Result<(), String> {
        let source = KeyringSecrets;
        self.forget_with(&source, plugin_id)
    }

    // ---- 下面四个是「可注入凭据来源」的版本，测试用它们而不碰真机凭据 ----

    /// 按名字取值（注入来源）。
    pub fn get_with(
        &self,
        source: &dyn SecretSource,
        plugin_id: &str,
        name: &str,
    ) -> Result<Option<String>, String> {
        if !paths::is_valid_plugin_id(plugin_id) {
            return Err(format!("id「{plugin_id}」不合法"));
        }
        let name = validate_secret_name(name)?;
        Ok(source.get(&service_for(plugin_id), &name))
    }

    /// 写值并记名（注入来源）。
    pub fn put_with(
        &self,
        source: &dyn SecretSource,
        plugin_id: &str,
        name: &str,
        value: &str,
    ) -> Result<(), String> {
        if !paths::is_valid_plugin_id(plugin_id) {
            return Err(format!("id「{plugin_id}」不合法"));
        }
        let name = validate_secret_name(name)?;
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("密钥值为空；要清掉请用它提供的删除入口".to_string());
        }
        if trimmed.len() > MAX_VALUE_BYTES {
            return Err(format!(
                "密钥值 {} 字节，超过本宿主的 {MAX_VALUE_BYTES} 字节上限",
                trimmed.len()
            ));
        }
        // 先记名再写值：值写成功了名没记上会留下孤儿，反过来只是多一个空名，删的时候无事发生。
        remember_name(&self.names_path(plugin_id), &name)?;
        source.set(&service_for(plugin_id), &name, trimmed)
    }

    /// 删值并去名（注入来源）。
    pub fn remove_with(
        &self,
        source: &dyn SecretSource,
        plugin_id: &str,
        name: &str,
    ) -> Result<(), String> {
        if !paths::is_valid_plugin_id(plugin_id) {
            return Err(format!("id「{plugin_id}」不合法"));
        }
        let name = validate_secret_name(name)?;
        source.delete(&service_for(plugin_id), &name)?;
        forget_name(&self.names_path(plugin_id), &name)
    }

    /// 按名册清干净（注入来源）。
    pub fn forget_with(&self, source: &dyn SecretSource, plugin_id: &str) -> Result<(), String> {
        if !paths::is_valid_plugin_id(plugin_id) {
            return Err(format!("id「{plugin_id}」不合法"));
        }
        let path = self.names_path(plugin_id);
        let mut first_error: Option<String> = None;
        for name in read_names(&path).unwrap_or_default() {
            if let Err(reason) = source.delete(&service_for(plugin_id), &name) {
                if first_error.is_none() {
                    first_error = Some(reason);
                }
            }
        }
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        match first_error {
            Some(reason) => Err(format!("有几个密钥没删掉，请手动清理：{reason}")),
            None => Ok(()),
        }
    }

    /// 名册路径。
    fn names_path(&self, plugin_id: &str) -> PathBuf {
        paths::package_dir(&self.app_data, plugin_id).join(NAMES_FILE)
    }
}

/// 读名册（不存在 = 空；读坏了就报错，不假装「一个都没有」，否则卸载会留孤儿）。
fn read_names(path: &Path) -> Result<Vec<String>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path).map_err(|error| format!("密钥名册读不了：{error}"))?;
    let parsed: Vec<String> = serde_json::from_str(&text).map_err(|error| format!("密钥名册坏了：{error}"))?;
    Ok(parsed)
}

/// 写回名册。
fn write_names(path: &Path, names: &[String]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("目录建不出来：{error}"))?;
    }
    let text = serde_json::to_string(names).map_err(|error| format!("名册序列化失败：{error}"))?;
    std::fs::write(path, text).map_err(|error| format!("名册写不下去：{error}"))
}

/// 记一个名字（去重、排序，便于人读）。
fn remember_name(path: &Path, name: &str) -> Result<(), String> {
    let mut names = read_names(path)?;
    if !names.iter().any(|item| item == name) {
        names.push(name.to_string());
        names.sort();
        write_names(path, &names)?;
    }
    Ok(())
}

/// 去一个名字（没有也当成功）。
fn forget_name(path: &Path, name: &str) -> Result<(), String> {
    let mut names = read_names(path)?;
    let before = names.len();
    names.retain(|item| item != name);
    if names.len() != before {
        write_names(path, &names)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 假凭据库：把值记在内存里，顺带数「有没有被真的调用」。
    ///
    /// 用 `Mutex` 而不是 `RefCell`：`SecretSource` 要求 `Send + Sync`，
    /// 而这条约束正是命令层能把它交给 blocking 线程的前提。
    #[derive(Default)]
    struct FakeSecrets {
        calls: std::sync::Mutex<Vec<String>>,
        store: std::sync::Mutex<std::collections::BTreeMap<(String, String), String>>,
    }

    impl FakeSecrets {
        /// 记一次调用（锁中毒在测试里不可能发生，发生了也照样往下走，不让断言变成 panic 现场）。
        fn note(&self, text: String) {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push(text);
            }
        }
    }

    impl SecretSource for FakeSecrets {
        fn get(&self, service: &str, name: &str) -> Option<String> {
            self.note(format!("get {service} {name}"));
            self.store
                .lock()
                .ok()
                .and_then(|map| map.get(&(service.to_string(), name.to_string())).cloned())
        }

        fn set(&self, service: &str, name: &str, value: &str) -> Result<(), String> {
            self.note(format!("set {service} {name}"));
            if let Ok(mut map) = self.store.lock() {
                map.insert((service.to_string(), name.to_string()), value.to_string());
            }
            Ok(())
        }

        fn delete(&self, service: &str, name: &str) -> Result<(), String> {
            self.note(format!("del {service} {name}"));
            if let Ok(mut map) = self.store.lock() {
                map.remove(&(service.to_string(), name.to_string()));
            }
            Ok(())
        }
    }

    /// 每测一个独立 app-data。
    fn temp_app_data(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ufo-plugin-secret-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    #[test]
    fn service_is_one_namespace_per_package() {
        let mine = service_for("balance-board");
        let other = service_for("ci-timing");
        assert!(mine.starts_with(APP_IDENTIFIER), "{mine}");
        assert!(mine.ends_with("/plugin/balance-board"), "{mine}");
        assert_ne!(mine, other, "两个包共用一格就读得到对方的密钥");
        // 与内置流程脚本那格不重合：那边是 `<标识>/flow/<endpoint>`。
        assert!(
            !mine.contains("/flow/"),
            "串到 flow 那一格就能读到脚本密钥：{mine}"
        );
    }

    #[test]
    fn bad_names_and_values_are_refused_before_the_keyring_is_touched() {
        let app = temp_app_data("refuse");
        let state = SecretState::new(app.clone());
        let source = FakeSecrets::default();
        for name in ["", "   ", "a\0b", "x\ny"] {
            let err = state
                .put_with(&source, "probe", name, "v")
                .expect_err("坏名字该拒");
            assert!(err.contains("密钥名"), "{name} -> {err}");
        }
        let empty = state
            .put_with(&source, "probe", "API_KEY", "   ")
            .expect_err("空值该拒");
        assert!(empty.contains("值为空"), "{empty}");
        let huge = state
            .put_with(&source, "probe", "API_KEY", &"x".repeat(MAX_VALUE_BYTES + 1))
            .expect_err("超长该拒");
        assert!(huge.contains("上限"), "{huge}");
        // 全程没碰凭据库：这条断言才是「先校验后动手」的证据。
        let calls = source.calls.lock().expect("锁");
        assert!(calls.is_empty(), "{calls:?}");
        // 坏 id 也在这里挡住（service 名会带着它进凭据管理器）。
        let bad_id = state
            .put_with(&source, "../escape", "API_KEY", "v")
            .expect_err("坏 id");
        assert!(bad_id.contains("不合法"), "{bad_id}");
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn names_roster_round_trips_and_dedupes() {
        let app = temp_app_data("roster");
        let state = SecretState::new(app.clone());
        let source = FakeSecrets::default();
        state.put_with(&source, "probe", "B_KEY", "b").expect("写 B");
        state.put_with(&source, "probe", "A_KEY", "a").expect("写 A");
        state.put_with(&source, "probe", "A_KEY", "a2").expect("重写 A");
        assert_eq!(
            state.names("probe").expect("名册"),
            vec!["A_KEY".to_string(), "B_KEY".to_string()],
            "名册要去重 + 排序"
        );
        // 值走的是这一包那一格，不是别处的同名条目。
        let store = source.store.lock().expect("锁");
        assert!(store.contains_key(&(service_for("probe"), "A_KEY".to_string())));
        drop(store);
        assert_eq!(
            state.get_with(&source, "probe", "A_KEY").expect("读").as_deref(),
            Some("a2")
        );
        state.remove_with(&source, "probe", "A_KEY").expect("删");
        assert_eq!(state.names("probe").expect("名册"), vec!["B_KEY".to_string()]);
        assert!(state.get_with(&source, "probe", "A_KEY").expect("读").is_none());
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn forget_walks_the_roster_and_drops_it() {
        let app = temp_app_data("forget");
        let state = SecretState::new(app.clone());
        let source = FakeSecrets::default();
        state.put_with(&source, "probe", "K1", "1").expect("写 1");
        state.put_with(&source, "probe", "K2", "2").expect("写 2");
        state.forget_with(&source, "probe").expect("卸载清理");
        let calls = source.calls.lock().expect("锁");
        assert!(calls.iter().any(|c| c.ends_with("K1") && c.starts_with("del ")), "{:?}", *calls);
        assert!(calls.iter().any(|c| c.ends_with("K2") && c.starts_with("del ")), "{:?}", *calls);
        drop(calls);
        assert!(source.store.lock().expect("锁").is_empty(), "值要真的没了");
        assert!(state.names("probe").expect("名册").is_empty(), "名册要跟着删");
        // 名册文件删掉之后再来一次：无事发生，不报错。
        state.forget_with(&source, "probe").expect("重复卸载");
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn broken_roster_errors_instead_of_looking_empty() {
        let app = temp_app_data("broken");
        let state = SecretState::new(app.clone());
        let path = state.names_path("probe");
        std::fs::create_dir_all(path.parent().expect("有父目录")).expect("建目录");
        std::fs::write(&path, "{ not json").expect("写坏名册");
        let err = state.names("probe").expect_err("坏名册该报错");
        assert!(err.contains("坏了"), "{err}");
        let _ = std::fs::remove_dir_all(&app);
    }
}
