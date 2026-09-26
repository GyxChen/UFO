//! 推送调度层的单元测试。
//!
//! 网络走 `testing` 里的假件，所以这些用例不打网；覆盖的都是真会出事的地方：
//! 两层开关、每主题一条、退避、正文内容与「默认不外泄」。

use super::config::{ChannelConfig, ProviderConfig};
use super::*;
use crate::modules::agent_status::flow::{FlowState, SecretSource};
use crate::modules::agent_status::status::{
    AgentEventKind, Provider, SessionPhase, SessionSnapshot, StatusSource,
};
use testing::FakeFetch;

/// flow 状态要一个密钥后端才能建，但推送用例从不写密钥 —— 这个空壳只为满足装配。
#[derive(Default)]
struct UnusedSecrets;

impl SecretSource for UnusedSecrets {
    fn get(&self, _service: &str, _name: &str) -> Option<String> {
        None
    }

    fn set(&self, _service: &str, _name: &str, _value: &str) -> Result<(), String> {
        Err("推送测试里不该写密钥".to_string())
    }

    fn delete(&self, _service: &str, _name: &str) -> Result<(), String> {
        Ok(())
    }
}

/// 一盏「在等你」的 claude 会话灯（提问里故意放一串假密钥，验证它不会外泄）。
fn session() -> SessionSnapshot {
    SessionSnapshot {
        key: "claude:8f21c3a4-0000".to_string(),
        provider: Provider::Claude,
        session_id: "8f21c3a4-0000".to_string(),
        phase: SessionPhase::Waiting,
        source: StatusSource::Hook,
        confidence: 100,
        started_at_ms: 1,
        last_seen_at_ms: 1,
        completed_at_ms: None,
        remove_at_ms: None,
        is_flashing: false,
        pid: None,
        error_code: Some("overloaded".to_string()),
        base_url: None,
        upstream_url: None,
        prompt: Some("把密钥 sk-SECRET 贴上去".to_string()),
        waiting_reason: Some("允许访问官网？".to_string()),
        tokens: None,
        last_sequence: None,
        termination_confirmed: false,
    }
}

fn snapshot_of(sessions: Vec<SessionSnapshot>) -> StatusSnapshot {
    StatusSnapshot {
        schema_version: 1,
        revision: 1,
        generated_at_ms: 1,
        sessions,
        counts: Default::default(),
        source_health: "ok".to_string(),
    }
}

/// 一行主题。
fn topic(id: &str, name: &str, value: &str) -> ChannelConfig {
    ChannelConfig {
        id: id.to_string(),
        provider: "ntfy".to_string(),
        name: name.to_string(),
        topic: value.to_string(),
    }
}

/// 一份开了 ntfy + 两行主题的配置（服务开关单独可控）。
fn config_with(provider_enabled: bool) -> PushConfig {
    PushConfig {
        enabled: true,
        events: vec![
            PushEvent::TurnFinished,
            PushEvent::WaitingForUser,
            PushEvent::Failed,
        ],
        content: Vec::new(),
        providers: [(
            "ntfy".to_string(),
            ProviderConfig {
                enabled: provider_enabled,
                server: "https://ntfy.sh".to_string(),
            },
        )]
        .into_iter()
        .collect(),
        channels: vec![
            topic("ch1", "家里电脑", "ufo"),
            topic("ch2", "公司群", "ufo-work"),
        ],
    }
}

/// 一个空转的 flow 状态：只有勾了「脚本内容」时推送才会去读它。
fn flow() -> FlowState {
    FlowState::new(Arc::new(FakeFetch::ok()), Arc::new(UnusedSecrets)).0
}

/// 建一套可断言的推送状态（配置已生效）。
fn state_with(config: PushConfig, fetch: Arc<FakeFetch>) -> (PushState, ReceiverHolder) {
    let (state, rx) = PushState::new(fetch);
    state.configure(config);
    (state, rx)
}

/// 组一个待投递任务。
fn job(channel_id: &str, provider_id: &str) -> PushJob {
    PushJob {
        channel_id: channel_id.to_string(),
        provider_id: provider_id.to_string(),
        message: PushMessage {
            title: "UFO · 一轮跑完".to_string(),
            body: "claude 一轮跑完".to_string(),
        },
    }
}

#[test]
fn master_switch_off_produces_no_signal() {
    let (state, rx) = state_with(PushConfig::default(), Arc::new(FakeFetch::ok()));
    state.note_change("claude:8f21c3a4-0000", AgentEventKind::TurnFinished);
    state.tick(&snapshot_of(vec![session()]), &flow(), 1_000);
    assert!(
        matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "总开关关着不该入队"
    );
}

#[test]
fn running_and_subagent_kinds_never_push() {
    let (state, rx) = state_with(config_with(true), Arc::new(FakeFetch::ok()));
    for kind in [
        AgentEventKind::Running,
        AgentEventKind::SubagentStarted,
        AgentEventKind::SubagentStopped,
        AgentEventKind::Unknown,
    ] {
        state.note_change("claude:8f21c3a4-0000", kind);
    }
    state.tick(&snapshot_of(vec![session()]), &flow(), 1_000);
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)), "这几种语义不该推");
}

#[test]
fn one_light_change_pushes_once_per_topic() {
    // 一次状态变化 × 两行主题 = 两条任务，且都带同一份文案。
    let (state, rx) = state_with(config_with(true), Arc::new(FakeFetch::ok()));
    state.note_change("claude:8f21c3a4-0000", AgentEventKind::TurnFinished);
    state.tick(&snapshot_of(vec![session()]), &flow(), 1_000);
    let first = rx.try_recv().expect("应入队第一条");
    let second = rx.try_recv().expect("应入队第二条");
    assert_eq!(first.message.body, "claude 一轮跑完");
    assert_eq!(second.message.body, first.message.body);
    assert_eq!(first.message.title, "UFO · 一轮跑完");
    let ids: Vec<&str> = vec![first.channel_id.as_str(), second.channel_id.as_str()]
        .into_iter()
        .collect();
    assert_eq!(ids, vec!["ch1", "ch2"], "顺序即配置顺序");
    // ⚠️ 提问原文绝不进正文：ntfy 主题是公开可订阅的。
    assert!(!first.message.body.contains("sk-SECRET"), "{}", first.message.body);
}

#[test]
fn disabled_provider_sends_nothing() {
    let (state, rx) = state_with(config_with(false), Arc::new(FakeFetch::ok()));
    state.note_change("claude:8f21c3a4-0000", AgentEventKind::TurnFinished);
    state.tick(&snapshot_of(vec![session()]), &flow(), 1_000);
    assert!(
        matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "服务关掉时它下面所有主题都不发"
    );
}

#[test]
fn unregistered_provider_sends_nothing() {
    // 配置里写了一个后端没登记的服务（手改配置 / 旧版本残留）：只能不发，不能 panic。
    let mut config = config_with(true);
    config.channels.push(ChannelConfig {
        id: "ch9".to_string(),
        provider: "serverchan".to_string(),
        name: "没登记".to_string(),
        topic: "x".to_string(),
    });
    let fetch = Arc::new(FakeFetch::ok());
    let (state, _rx) = state_with(config, fetch.clone());
    let snapshot = state.handle_job(&job("ch9", "serverchan"));
    assert_eq!(fetch.len(), 0, "解析不出 provider 就不该发请求");
    assert!(snapshot.channels.iter().any(|item| item.id == "ch9"));
}

#[test]
fn no_live_sessions_means_zero_jobs() {
    // 「agent 全停 = 零出网」这条不变量的落点。
    let (state, rx) = state_with(config_with(true), Arc::new(FakeFetch::ok()));
    state.note_change("claude:gone", AgentEventKind::Failed);
    state.tick(&snapshot_of(Vec::new()), &flow(), 1_000);
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
}

#[test]
fn failed_delivery_backs_off_the_next_tick() {
    let fetch = Arc::new(FakeFetch::failing(401, "unauthorized"));
    let (state, rx) = state_with(config_with(true), fetch.clone());
    state.handle_job(&job("ch1", "ntfy"));
    assert_eq!(fetch.len(), 1, "投递要真的走一次 send");

    // 退避生效：紧接着的下一轮，两行主题里被退避的那条不再入队。
    state.note_change("claude:8f21c3a4-0000", AgentEventKind::Failed);
    state.tick(&snapshot_of(vec![session()]), &flow(), now_ms() + 1_000);
    let queued = rx.try_recv().expect("另一行主题仍应发");
    assert_eq!(queued.channel_id, "ch2", "退避只按失败的那一行算");
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)), "失败的那行要退避");
}

#[test]
fn configure_prunes_backoff_of_deleted_topics() {
    let fetch = Arc::new(FakeFetch::failing(403, "denied"));
    let (state, _rx) = state_with(config_with(true), fetch);
    state.handle_job(&job("ch1", "ntfy"));
    // 换成只剩 ch9 的配置：删掉的行的退避账要跟着清，不能只增不减。
    let mut next = config_with(true);
    next.channels = vec![topic("ch9", "新主题", "ufo-new")];
    state.configure(next);
    state.note_change("claude:8f21c3a4-0000", AgentEventKind::Failed);
    state.tick(&snapshot_of(vec![session()]), &flow(), now_ms());
    assert!(state.snapshot().channels.iter().all(|item| item.id == "ch9"));
}

#[test]
fn test_provider_sends_one_message_per_topic() {
    let fetch = Arc::new(FakeFetch::ok());
    let (state, _rx) = state_with(config_with(false), fetch.clone());
    let outcomes = state.test_provider("ntfy").expect("测试应成功");
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().all(|item| item.ok));
    assert_eq!(
        outcomes.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(),
        vec!["家里电脑", "公司群"]
    );
    // 每个主题各一条，且 URL 里是它自己的主题串。
    assert_eq!(fetch.urls(), vec!["https://ntfy.sh/ufo", "https://ntfy.sh/ufo-work"]);
    // ⚠️ 测试刻意不看服务开关：上面用的是 provider_enabled = false 的配置，照样发得出去。
}

#[test]
fn test_provider_reports_the_same_readable_error_as_real_sends() {
    let fetch = Arc::new(FakeFetch::failing(404, "not found"));
    let (state, _rx) = state_with(config_with(true), fetch);
    let outcomes = state.test_provider("ntfy").expect("命令本身不失败");
    assert!(outcomes.iter().all(|item| !item.ok));
    let error = outcomes.first().and_then(|item| item.error.clone()).unwrap_or_default();
    assert!(error.contains("404"), "{error}");
    assert!(error.contains("主题"), "{error}");
}

#[test]
fn test_provider_without_topics_explains_why() {
    let (state, _rx) = state_with(config_with(true), Arc::new(FakeFetch::ok()));
    assert!(state.test_provider("ghost").is_err(), "没登记的服务要给出原因");
    let mut empty = config_with(true);
    empty.channels.clear();
    state.configure(empty);
    let error = state.test_provider("ntfy").expect_err("没有主题要报错");
    assert!(error.contains("先加一行"), "{error}");
}

#[test]
fn snapshot_publishes_topics_in_plaintext() {
    // 「主题不是密钥」这条口径的锁：快照里就得能读到主题串，也不再有任何密钥状态字段。
    let (state, _rx) = state_with(config_with(true), Arc::new(FakeFetch::ok()));
    let snapshot = state.snapshot();
    let text = serde_json::to_string(&snapshot).expect("快照可序列化");
    assert!(text.contains("\"topic\":\"ufo-work\""), "{text}");
    assert!(text.contains("\"server\":\"https://ntfy.sh\""), "{text}");
    assert!(!text.contains("secret"), "密钥那套已经整体删掉：{text}");
    assert!(!text.contains("records"), "投递记录已删：{text}");
}

#[test]
fn upstream_wins_over_local_base_url_for_the_endpoint_line() {
    // 与 tooltip 的 displayEndpoint 同一口径：反查出的上游优先，没有才回落本地地址。
    let mut config = config_with(true);
    config.content = vec![PushContent::Endpoint];
    let mut session = session();
    session.base_url = Some("http://127.0.0.1:15721".to_string());
    session.upstream_url = Some("https://api.upstream.example".to_string());
    let message = build_message(&config, &session, PushEvent::Failed, None);
    assert!(
        message.body.contains("endpoint: https://api.upstream.example"),
        "{message:?}"
    );
    assert!(!message.body.contains("15721"), "{message:?}");
    // 只有本地地址时也要有一行可读的地址（没开解析器的常态）。
    session.upstream_url = None;
    let message = build_message(&config, &session, PushEvent::Failed, None);
    assert!(message.body.contains("endpoint: http://127.0.0.1:15721"), "{message:?}");
}

#[test]
fn body_always_carries_the_agent_status_and_nothing_else_by_default() {
    // 默认 content 为空：正文只有「平台 + 事件」，一条可选内容都不带。
    let (state, rx) = state_with(config_with(true), Arc::new(FakeFetch::ok()));
    state.note_change("claude:8f21c3a4-0000", AgentEventKind::Failed);
    state.tick(&snapshot_of(vec![session()]), &flow(), 1_000);
    let queued = rx.try_recv().expect("应入队一条");
    assert_eq!(queued.message.body, "claude 本轮出错");
    assert!(!queued.message.body.contains("sk-SECRET"), "默认不能把提问带出去");
}

#[test]
fn each_selected_content_item_appends_its_own_line() {
    let mut config = config_with(true);
    config.content = vec![
        PushContent::Endpoint,
        PushContent::Prompt,
        PushContent::SessionId,
        PushContent::Script,
    ];
    let (state, rx) = state_with(config, Arc::new(FakeFetch::ok()));
    let mut session = session();
    session.upstream_url = Some("https://api.upstream.example".to_string());
    state.note_change("claude:8f21c3a4-0000", AgentEventKind::WaitingForUser);
    state.tick(&snapshot_of(vec![session]), &flow(), 1_000);
    let queued = rx.try_recv().expect("应入队一条");
    let body = queued.message.body;
    assert!(body.starts_with("claude 在等你"), "{body}");
    assert!(body.contains("endpoint: https://api.upstream.example"), "{body}");
    assert!(body.contains("提问: 把密钥"), "{body}");
    assert!(body.contains("会话: claude:8f21c3a4-0000"), "{body}");
    // 没跑过脚本 → 该整行不出现，而不是写个「脚本: 无」。
    assert!(!body.contains("脚本:"), "{body}");
}

#[test]
fn script_content_is_rendered_from_flow_fields() {
    // 直接测组装：flow 结果的产出另有 flow 模块自己的测试覆盖。
    let config = config_with(true);
    let fields = vec![
        FlowField { label: "余额".to_string(), value: "¥12.30".to_string(), color: None },
        FlowField {
            label: "用量".to_string(),
            value: "80%".to_string(),
            color: Some("yellow".to_string()),
        },
    ];
    let message = build_message(&config, &session(), PushEvent::TurnFinished, Some(&fields));
    // 默认不勾 → 不带脚本。
    assert_eq!(message.body, "claude 一轮跑完");
    let mut with_script = config.clone();
    with_script.content = vec![PushContent::Script];
    let message = build_message(&with_script, &session(), PushEvent::TurnFinished, Some(&fields));
    assert!(message.body.contains("脚本: 余额=¥12.30、用量=80%"), "{}", message.body);
    // 只勾了脚本但这次没结果：整行消失，状态行仍在。
    let empty = build_message(&with_script, &session(), PushEvent::TurnFinished, None);
    assert_eq!(empty.body, "claude 一轮跑完");
}

#[test]
fn long_prompt_is_clipped_to_one_line() {
    let mut config = config_with(true);
    config.content = vec![PushContent::Prompt];
    let mut session = session();
    session.prompt = Some(format!("第一行\n{}", "汉".repeat(400)));
    let message = build_message(&config, &session, PushEvent::Failed, None);
    let prompt_line = message.body.lines().last().unwrap_or_default();
    assert!(prompt_line.starts_with("提问: 第一行 汉"), "{prompt_line}");
    // 换行被压成空格（一行一项），并按字符数截断加省略号。
    assert_eq!(message.body.lines().count(), 2, "正文只该两行：状态 + 提问");
    assert!(prompt_line.ends_with('…'), "{prompt_line}");
    assert!(prompt_line.chars().count() < 320);
}
