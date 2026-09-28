//! 滴答 API Token 的存取：只进 OS 凭据管理器（Windows = 凭据管理器），不落盘、不进日志。
//!
//! 与天气模块同构：service 名带模块后缀，同一个 App 的多个组件各存各的，互不踩。

use crate::modules::agent_status::status::{APP_IDENTIFIER, LEGACY_APP_IDENTIFIER};

/// 凭据 service 后缀（与前端模块 id 一致）。
const TODO_SERVICE_SUFFIX: &str = "todo";
/// 凭据条目名。
const API_TOKEN_NAME: &str = "api_token";

/// 按 App 标识拼 service 名（当前标识与旧标识只差这个前缀）。
fn service_name(prefix: &str) -> String {
    format!("{prefix}/{TODO_SERVICE_SUFFIX}")
}

/// 本模块在 OS 凭据管理器里的 service 名称。
pub fn get_service_name() -> String {
    service_name(APP_IDENTIFIER)
}

/// 把 Token 从旧 App 标识的 service 复制到当前 service（新值已存在或旧值都没有都无事发生）。
///
/// App 标识改过一次名，凭据按 service 名前缀寻址，不搬就等于用户重填一遍。
pub fn migrate_from_legacy() {
    crate::migrate::copy_credential(
        &service_name(LEGACY_APP_IDENTIFIER),
        &get_service_name(),
        API_TOKEN_NAME,
    );
}

/// 从 OS 凭据管理器读取 API Token（空串视为未配置）。
pub fn get_token() -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(&get_service_name(), API_TOKEN_NAME)
        .map_err(|e| format!("创建凭据条目失败: {e}"))?;

    match entry.get_password() {
        Ok(token) => {
            let trimmed = token.trim().to_string();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed))
            }
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("读取系统凭据失败: {e}")),
    }
}

/// 把 API Token 写入 OS 凭据管理器（空串等于删除）。
pub fn set_token(token: &str) -> Result<(), String> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return delete_token();
    }

    let entry = keyring::Entry::new(&get_service_name(), API_TOKEN_NAME)
        .map_err(|e| format!("创建凭据条目失败: {e}"))?;
    entry
        .set_password(trimmed)
        .map_err(|e| format!("保存系统凭据失败: {e}"))
}

/// 从 OS 凭据管理器删除 API Token（没有也算成功）。
pub fn delete_token() -> Result<(), String> {
    let entry = keyring::Entry::new(&get_service_name(), API_TOKEN_NAME)
        .map_err(|e| format!("创建凭据条目失败: {e}"))?;

    match entry.delete_credential() {
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除系统凭据失败: {e}")),
    }
}

#[cfg(test)]
mod tests {
    /// 令牌读写依赖真实凭据管理器，单测不碰真机凭据；只锁 service 命名口径。
    #[test]
    fn service_name_carries_module_suffix() {
        assert!(super::get_service_name().ends_with("/todo"));
    }
}
