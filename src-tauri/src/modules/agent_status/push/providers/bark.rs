//! Bark：iOS 推送服务。它的「主题」就是 App 首页那串设备密钥，**一串只对应一台设备**。
//!
//! 协议：`POST {server}/{topic}`，正文是 JSON（只带 `title` / `body`）。
//! 用 POST 而不是把标题正文塞进 URL 路径：路径长度受限、中文要编码、日志里还留一份明文 URL。
//!
//! 多台设备 = 同一服务下的多行主题（见规范 §1）：因此本文件不需要知道有几台设备，
//! 也不需要为 Bark 特判任何控制流。
//!
//! ⚠️ 设备密钥按用户要求**明文存在配置文件里**（不再进系统凭据管理器），与「提问明文落会话存档」
//! 同级的已知代价；`sound` / `level` / `group` 这类打扰度选项也一并删掉了。

use super::{encode_segment, normalize_server, PushProvider, SendRequest, ServerSpec};
use crate::modules::agent_status::flow::HttpRequest;

/// 单次推送的超时（毫秒），与 ntfy 同档。
const PUSH_TIMEOUT_MS: u64 = 10_000;

/// 官方服务器：用户把地址栏留空就用它。
const DEFAULT_SERVER: &str = "https://api.day.app";

/// Bark 适配器单例；由 `push::build_providers()` 注册。
pub struct BarkProvider;

impl PushProvider for BarkProvider {
    fn id(&self) -> &'static str {
        "bark"
    }

    fn label(&self) -> &'static str {
        "Bark"
    }

    fn server(&self) -> ServerSpec {
        ServerSpec {
            label: "服务器",
            default: DEFAULT_SERVER,
            help: "官方服务器留空即可；自建地址在 Bark App 的设置里能查到",
        }
    }

    /// 向 `{server}/{topic}` POST 一条 JSON；主题就是设备密钥。
    fn send(
        &self,
        request: &SendRequest<'_>,
        http: &dyn crate::modules::agent_status::flow::HttpFetch,
    ) -> Result<u16, String> {
        let topic = request.topic.trim();
        if topic.is_empty() {
            return Err("Bark 设备密钥（主题）为空，先在右侧列表填一行".to_string());
        }
        let server = normalize_server(request.server, DEFAULT_SERVER)?;
        // 密钥进路径也要编码（它本身是安全字符集，但用户可能粘进空格）。
        let url = format!("{}/{}", server, encode_segment(topic));

        // 交给 serde_json 做转义：标题/正文里出现引号或换行都不会把 JSON 拼坏。
        let mut payload = serde_json::Map::new();
        payload.insert("title".into(), request.message.title.clone().into());
        payload.insert("body".into(), request.message.body.clone().into());

        let response = http.fetch(&HttpRequest {
            method: "POST".to_string(),
            url,
            headers: vec![(
                "Content-Type".to_string(),
                "application/json; charset=utf-8".to_string(),
            )],
            body: Some(serde_json::Value::Object(payload).to_string()),
            timeout_ms: PUSH_TIMEOUT_MS,
        })?;
        if !(200..300).contains(&response.status) {
            return Err(describe_failure(response.status, &response.body));
        }
        Ok(response.status)
    }
}

/// 把非 2xx 翻译成「能照着修」的一句话。
fn describe_failure(status: u16, body: &str) -> String {
    let hint = match status {
        400 => "请求内容不合法（地址或主题串被服务端拒了）",
        403 => "该密钥没有推送权限",
        // Bark 最常见的失败就是用户在 App 里重置了密钥：直接说去哪拿新的。
        410 => "设备密钥已失效，请在 Bark App 里重新复制并填进主题栏",
        429 => "被限流了（发送太快）",
        _ => "服务端拒绝",
    };
    let detail: String = body
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(120)
        .collect();
    if detail.is_empty() {
        format!("Bark 返回 {status}：{hint}")
    } else {
        format!("Bark 返回 {status}：{hint}（{detail}）")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::agent_status::push::message::PushMessage;
    use crate::modules::agent_status::push::testing::FakeFetch;

    /// 组一份请求（地址与密钥都是借用水值）。
    fn request_with<'a>(server: &'a str, topic: &'a str, message: &'a PushMessage) -> SendRequest<'a> {
        SendRequest { server, topic, message }
    }

    fn message() -> PushMessage {
        PushMessage {
            title: "UFO · 在等你".to_string(),
            body: "qoder:6064d9a0 正在等授权".to_string(),
        }
    }

    #[test]
    fn posts_only_title_and_body_to_the_topic_path() {
        let message = message();
        let fetch = FakeFetch::ok();
        BarkProvider
            .send(&request_with("https://api.day.app", "k9abcdef", &message), &fetch)
            .expect("发送成功");
        let last = fetch.last();
        assert_eq!(last.url, "https://api.day.app/k9abcdef");
        assert_eq!(last.method, "POST");
        let body: serde_json::Value =
            serde_json::from_str(last.body.as_deref().expect("有 body")).expect("合法 JSON");
        assert_eq!(body["title"], "UFO · 在等你");
        assert_eq!(body["body"], "qoder:6064d9a0 正在等授权");
        // 打扰度相关的可选项已全部删除：请求体只该有这两个键。
        assert_eq!(body.as_object().map(|map| map.len()), Some(2));
    }

    #[test]
    fn quotes_and_newlines_in_text_cannot_break_the_payload() {
        // 正文里带引号与换行（甚至 `"} ` 这种）必须被 serde 转义，而不是拼坏 JSON。
        let message = PushMessage {
            title: "UFO · 一轮跑完".to_string(),
            body: "他说\"不\"\n{\"x\":1}".to_string(),
        };
        let fetch = FakeFetch::ok();
        BarkProvider
            .send(&request_with("https://api.day.app", "key", &message), &fetch)
            .expect("发送成功");
        let body: serde_json::Value =
            serde_json::from_str(fetch.last().body.as_deref().expect("有 body")).expect("仍是合法 JSON");
        assert_eq!(body["body"], message.body);
    }

    #[test]
    fn topic_is_the_device_key_and_lives_in_the_path_not_the_body() {
        // 密钥只进路径：它现在明文落配置，但绝不该被再抄进请求体（少一份复制面）。
        let message = message();
        let fetch = FakeFetch::ok();
        BarkProvider
            .send(&request_with("https://api.day.app", "topsecret", &message), &fetch)
            .expect("发送成功");
        let last = fetch.last();
        assert!(last.url.contains("topsecret"), "密钥要在路径里");
        assert!(
            !last.body.as_deref().unwrap_or_default().contains("topsecret"),
            "body 里不该出现密钥：{}",
            last.body.clone().unwrap_or_default()
        );
    }

    #[test]
    fn empty_topic_fails_before_any_request() {
        let message = message();
        let fetch = FakeFetch::ok();
        let error = BarkProvider
            .send(&request_with("https://api.day.app", "  ", &message), &fetch)
            .expect_err("没有主题必须失败");
        assert!(error.contains("主题"), "{error}");
        assert!(fetch.is_empty());
    }

    #[test]
    fn empty_server_falls_back_to_the_official_one() {
        let message = message();
        let fetch = FakeFetch::ok();
        BarkProvider
            .send(&request_with("", "key", &message), &fetch)
            .expect("留空等于用官方服务器");
        assert_eq!(fetch.last().url, "https://api.day.app/key");
    }

    #[test]
    fn expired_key_hint_names_the_recovery_step() {
        let message = message();
        let fetch = FakeFetch::failing(410, "gone");
        let error = BarkProvider
            .send(&request_with("https://api.day.app", "key", &message), &fetch)
            .expect_err("410 是失败");
        assert!(error.contains("重新复制"), "{error}");
    }

    #[test]
    fn server_spec_declares_an_https_default_and_help() {
        let spec = BarkProvider.server();
        assert_eq!(spec.label, "服务器");
        assert_eq!(spec.default, "https://api.day.app");
        assert!(!spec.help.is_empty(), "地址栏要有一句说明");
    }
}
