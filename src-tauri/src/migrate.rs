//! App 标识（`tauri.conf.json` 的 `identifier`）改名后的一次性数据搬迁。
//!
//! 配置、会话存档、endpoint 档案、转灯日志都落在 `%APPDATA%/<identifier>` 下，标识一改这些会
//! 集体「看不见」—— 表现成全新安装。这里在启动最早期把旧目录里**新目录还没有**的条目复制过来：
//! 只补缺、绝不覆盖，也**不删源**（万一要退回旧版本，旧目录仍然完整）。
//!
//! ⚠️ 必须在第一次读 `app_data_dir()` 之前调用（见 `lib.rs` setup 的第一行），否则后面所有状态
//! 都按空目录加载，搬过来的东西这一轮就用不上了。
//! ⚠️ 凭据不在文件系统里：OS 凭据管理器按 service 名存，而 service 名的前缀就是这个标识。
//! 机制由 [`copy_credential`] 提供，具体搬哪几条由各组件自己说（组合根点名），外壳不认识。

use std::{
    fs,
    path::{Path, PathBuf},
};

use tauri::{AppHandle, Manager, Runtime};

use crate::modules::agent_status::status::LEGACY_APP_IDENTIFIER;

/// 把旧标识下的数据目录补缺式地复制进当前数据目录，返回复制的文件数。
///
/// 没有旧目录（全新机器、或已经搬过一次并清掉）时静默返回 0。
pub fn copy_app_data<R: Runtime>(app: &AppHandle<R>) -> usize {
    let Ok(current) = app.path().app_data_dir() else {
        return 0;
    };
    let Some(legacy) = legacy_dir(&current) else {
        return 0;
    };
    let mut copied = 0;
    copy_missing(&legacy, &current, &mut copied);
    copied
}

/// 旧标识目录 = 当前数据目录的**同级**兄弟（同一个 AppData 根，只是标识名不同）。
fn legacy_dir(current: &Path) -> Option<PathBuf> {
    let legacy = current.parent()?.join(LEGACY_APP_IDENTIFIER);
    (legacy != current && legacy.is_dir()).then_some(legacy)
}

/// 递归补缺：目录逐层进去（缺就建），文件只在目标不存在时才复制。
///
/// 任何一步失败都只跳过那一项 —— 搬迁属于启动期的附加动作，不该让 App 起不来。
fn copy_missing(from: &Path, to: &Path, copied: &mut usize) {
    if from.is_file() {
        // 已存在的一律不动：新目录里的更新，迁移只负责「补缺」。
        if to.exists() {
            return;
        }
        if fs::copy(from, to).is_ok() {
            *copied += 1;
        }
        return;
    }
    let Ok(entries) = fs::read_dir(from) else {
        return;
    };
    // 先保证目标目录建得出来，子项才有地方落。
    if fs::create_dir_all(to).is_err() {
        return;
    }
    for entry in entries.flatten() {
        copy_missing(&entry.path(), &to.join(entry.file_name()), copied);
    }
}

/// 把一条凭据从旧 service 复制到新 service，返回是否真的写了。
///
/// keyring 不能按 service 列举，所以只能由知道 `(service 后缀, 条目名)` 的组件逐条调用。
/// ⚠️ 只复制不删：退回旧版本时旧条目还在 —— 代价是旧前缀那批可能成为孤儿，
/// 要清得去「控制面板 → 凭据管理器 → Windows 凭据」手工删 `com.tauri-app.my/…`。
pub fn copy_credential(legacy_service: &str, service: &str, name: &str) -> bool {
    let Ok(value) = keyring::Entry::new(legacy_service, name).and_then(|entry| entry.get_password())
    else {
        return false;
    };
    let Ok(entry) = keyring::Entry::new(service, name) else {
        return false;
    };
    // 新前缀下已经配过值：不覆盖用户后来填的（重复启动也必须无事发生）。
    if entry.get_password().is_ok() {
        return false;
    }
    entry.set_password(&value).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::modules::agent_status::status::test_support::TempDir;

    /// 读回一个文件；不存在当空串（断言失败时看得清是「没搬过来」还是「搬错了」）。
    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn copies_missing_files_without_touching_existing_ones() {
        let root = TempDir::new("migrate");
        root.write("legacy/floating-modules.json", "old-config");
        root.write("legacy/status/log/2026-09-28.jsonl", "log-line");
        root.write("current/floating-modules.json", "newer-config");

        let mut copied = 0;
        copy_missing(
            &root.path().join("legacy"),
            &root.path().join("current"),
            &mut copied,
        );

        assert_eq!(copied, 1, "已存在的条目不得重抄");
        assert_eq!(
            read(&root.path().join("current/floating-modules.json")),
            "newer-config",
            "⚠️ 目标已存在时覆盖会吃掉用户改过的新配置"
        );
        assert_eq!(
            read(&root.path().join("current/status/log/2026-09-28.jsonl")),
            "log-line",
            "深层目录要连着层级一起补出来"
        );
    }

    #[test]
    fn finds_the_legacy_dir_as_a_sibling_only() {
        let root = TempDir::new("migrate-legacy");
        let current = root.path().join("com.ufo.app");
        // 旧目录还不存在：没有东西要搬。
        assert_eq!(legacy_dir(&current), None);

        fs::create_dir_all(root.path().join(LEGACY_APP_IDENTIFIER)).expect("create legacy");
        assert_eq!(
            legacy_dir(&current),
            Some(root.path().join(LEGACY_APP_IDENTIFIER)),
            "旧标识目录是当前数据目录的同级兄弟"
        );
    }
}
