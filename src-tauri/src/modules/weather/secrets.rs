use crate::modules::agent_status::status::APP_IDENTIFIER;

const WEATHER_SERVICE_SUFFIX: &str = "weather";
const API_KEY_NAME: &str = "api_key";

/// 获取天气模块在 OS 凭据管理器中的 service 名称
fn get_service_name() -> String {
    format!("{APP_IDENTIFIER}/{WEATHER_SERVICE_SUFFIX}")
}

/// 从 OS 凭据管理器读取和风 API Key
pub fn get_api_key() -> Result<Option<String>, String> {
    let service = get_service_name();
    let entry = keyring::Entry::new(&service, API_KEY_NAME)
        .map_err(|e| format!("创建凭据条目失败: {e}"))?;

    match entry.get_password() {
        Ok(key) => {
            let trimmed = key.trim().to_string();
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

/// 将和风 API Key 保存到 OS 凭据管理器
pub fn set_api_key(api_key: &str) -> Result<(), String> {
    let service = get_service_name();
    let entry = keyring::Entry::new(&service, API_KEY_NAME)
        .map_err(|e| format!("创建凭据条目失败: {e}"))?;

    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return delete_api_key();
    }

    entry
        .set_password(trimmed)
        .map_err(|e| format!("保存系统凭据失败: {e}"))
}

/// 从 OS 凭据管理器删除和风 API Key
pub fn delete_api_key() -> Result<(), String> {
    let service = get_service_name();
    let entry = keyring::Entry::new(&service, API_KEY_NAME)
        .map_err(|e| format!("创建凭据条目失败: {e}"))?;

    match entry.delete_credential() {
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除系统凭据失败: {e}")),
    }
}
