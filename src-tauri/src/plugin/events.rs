//! 灯态事件总线：让组件包能订阅「哪盏灯从什么变成什么」。
//!
//! 为什么不 `app.emit` 广播：所有组件包的 UI 代码与宿主**在同一个窗口里**，
//! 一次全局广播等于把提问原文发给每个包 —— 那正好绕过「这个包声明过 `events:agent` 才拿得到」这条闸。
//! 所以这里只有**按包拉取**：`plugin_events_poll(pluginId, since)` 在命令层查清单，
//! 没声明的包连"有哪些事件"都问不到（闸在 Rust，不在前端）。
//!
//! 三道口径写死在这里：
//! 1. **载荷按主题白名单逐字段构造**，不整串序列化会话快照 —— 快照以后加什么字段都不该自动流向第三方；
//! 2. **每包一个环形**（`EVENT_RING_LINES`），各包自己的游标互不影响（A 包卡住不会让 B 包丢事件）；
//! 3. 提问原文会进这里。它进 ring 只有一份，取走要有 `events:agent`；这条是需求点名的取舍。

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use super::paths;

/// 灯从一种状态变成另一种。
pub const TOPIC_LIGHT_CHANGED: &str = "light_changed";
/// 这条会话的提问原文变了。
pub const TOPIC_PROMPT_UPDATED: &str = "prompt_updated";
/// 这条会话从屏幕上消失了（结束并被清理）。
pub const TOPIC_SESSION_REMOVED: &str = "session_removed";

/// 本模块认的主题全集（清单里没声明 `events:agent` 的包一律拿不到，见 commands 那道闸）。
pub const TOPICS: &[&str] = &[TOPIC_LIGHT_CHANGED, TOPIC_PROMPT_UPDATED, TOPIC_SESSION_REMOVED];

/// 每包环形缓冲的行数。
pub const EVENT_RING_LINES: usize = paths::EVENT_RING_LINES;

/// 一盏灯此刻的读数（由组合根的巡检线程从状态快照换算出来，本模块不认识 agent_status）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LightSample {
    /// 会话的稳定 key（`provider:session_id`）。
    pub key: String,
    /// 平台名（`claude` / `codex` / `qoder` …）。
    pub provider: String,
    /// 会话 id（不含平台前缀）。
    pub session_id: String,
    /// 当前灯态：`running` / `waiting` / `error` / `completed` / `unknown`。
    pub phase: String,
    /// 这个 endpoint（上游优先，未解析到时为 `base_url`）。
    pub endpoint: Option<String>,
    /// 最近一次提问原文。
    pub prompt: Option<String>,
    /// 本轮开始时间（毫秒）。
    pub started_at_ms: i64,
    /// 最后一次看到它的时间（毫秒）。
    pub last_seen_at_ms: i64,
    /// 出错码（仅 error 态有）。
    pub error_code: Option<String>,
    /// 「正在等你操作」的原因原文（仅 waiting 态有）。
    pub waiting_reason: Option<String>,
}

/// 一条对外事件。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    /// 全局单调递增游标（所有包共用一条序列，各包按自己的 `since` 取增量）。
    pub seq: u64,
    /// 事件产生时间（毫秒）。
    pub ts: i64,
    /// 主题：`light_changed` / `prompt_updated` / `session_removed`。
    pub topic: String,
    /// 会话 key。
    pub key: String,
    /// 平台名。
    pub provider: String,
    /// 会话 id。
    pub session_id: String,
    /// 现在的灯态；`session_removed` 时是消失前那个状态。
    pub phase: String,
    /// 变化前的灯态；首见或移除时为 `None`。
    pub previous_phase: Option<String>,
    /// endpoint（可能为 `None`：还没采到上游信息）。
    pub endpoint: Option<String>,
    /// 提问原文（可能为 `None`）。⚠️ 这是敏感字段，只有 `events:agent` 拿得到。
    pub prompt: Option<String>,
    /// 本轮开始时间。
    pub started_at_ms: i64,
    /// 最后一次看到的时间。
    pub last_seen_at_ms: i64,
    /// 出错码。
    pub error_code: Option<String>,
    /// 等待原因。
    pub waiting_reason: Option<String>,
}

/// 一次拉取的返回。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventBatch {
    /// 本批事件（按 seq 升序）。
    pub events: Vec<AgentEvent>,
    /// 下次拉取该用的游标。
    pub next_seq: u64,
    /// 这个包的环形缓冲当前容量（读数用：溢出时 oldest 会跳）。
    pub ring_size: usize,
    /// 环形里最早一条的 seq；`since` 比它小 = 中间有丢（溢出的旧事件）。
    pub oldest_seq: Option<u64>,
}

/// 运行态。
#[derive(Clone, Default)]
pub struct EventState(Arc<Mutex<Inner>>);

#[derive(Debug, Default)]
struct Inner {
    /// 全局游标。
    seq: u64,
    /// 每包自己的环形（按 seq 升序）。
    rings: BTreeMap<String, VecDeque<AgentEvent>>,
    /// 每包当前有哪些窗口在盯（最后一个撤了才丢环）。
    viewers: BTreeMap<String, std::collections::BTreeSet<String>>,
    /// 上一轮读数，用于算「变了没有」。
    last: BTreeMap<String, LightSample>,
}

impl EventState {
    /// 建运行态。
    pub fn new() -> Self {
        Self::default()
    }

    /// 有没有人还盯着（有环 = 有人订阅过）。
    pub fn has_readers(&self) -> bool {
        !lock(&self.0).rings.is_empty()
    }

    /// 给某个包开一条环形并记下「哪个窗口在盯它」（幂等）。
    ///
    /// ⚠️ 调用前命令层必须已经查过清单里有 `events:agent` —— 环的存在就是那道闸的凭据。
    pub fn attach(&self, plugin_id: &str, window: &str) {
        let mut guard = lock(&self.0);
        let ring = guard.rings.entry(plugin_id.to_string()).or_default();
        let _ = ring;
        guard
            .viewers
            .entry(plugin_id.to_string())
            .or_default()
            .insert(window.to_string());
    }

    /// 某个窗口不再盯这个包；最后一个窗口撤了就丢掉它的环与读数。
    pub fn detach(&self, plugin_id: &str, window: &str) {
        let mut guard = lock(&self.0);
        let leave = match guard.viewers.get_mut(plugin_id) {
            Some(viewers) => {
                viewers.remove(window);
                viewers.is_empty()
            }
            None => false,
        };
        if leave {
            guard.rings.remove(plugin_id);
            guard.viewers.remove(plugin_id);
        }
    }

    /// 关掉某个包的环形（卸载 / 停用时无条件收）。
    pub fn close(&self, plugin_id: &str) {
        let mut guard = lock(&self.0);
        guard.rings.remove(plugin_id);
        guard.viewers.remove(plugin_id);
    }

    /// 巡检一轮：把「这一轮所有灯的读数」与上一轮比，产出事件并按包投递。
    ///
    /// ⚠️ 返回的是本轮新增的事件数；0 表示什么都没变（调用方因此可以完全不广播）。
    pub fn observe(&self, samples: &[LightSample], ts: i64) -> usize {
        let mut guard = lock(&self.0);
        if guard.rings.is_empty() {
            // 没人订阅就别留任何痕迹：否则上一轮读数会一直占着内存。
            guard.last.clear();
            return 0;
        }

        let mut fresh: Vec<AgentEvent> = Vec::new();
        let mut seen: Vec<String> = Vec::with_capacity(samples.len());

        for sample in samples {
            seen.push(sample.key.clone());
            // 取一份 owned 的上一轮读数：闭包要可变借用 guard，留着引用就编译不过。
            let previous = guard.last.get(&sample.key).cloned();
            let mut push = |topic: &str, previous_phase: Option<String>| {
                guard.seq += 1;
                fresh.push(AgentEvent {
                    seq: guard.seq,
                    ts,
                    topic: topic.to_string(),
                    key: sample.key.clone(),
                    provider: sample.provider.clone(),
                    session_id: sample.session_id.clone(),
                    phase: sample.phase.clone(),
                    previous_phase,
                    endpoint: sample.endpoint.clone(),
                    prompt: sample.prompt.clone(),
                    started_at_ms: sample.started_at_ms,
                    last_seen_at_ms: sample.last_seen_at_ms,
                    error_code: sample.error_code.clone(),
                    waiting_reason: sample.waiting_reason.clone(),
                });
            };
            match previous {
                // 首轮只把「已经在屏幕上的灯」记成基线，不因此造一批假事件。
                None => {
                    guard.last.insert(sample.key.clone(), sample.clone());
                }
                Some(before) => {
                    if before.phase != sample.phase {
                        push(TOPIC_LIGHT_CHANGED, Some(before.phase.clone()));
                    }
                    if before.prompt != sample.prompt && sample.prompt.is_some() {
                        // 提问被清空（离开等待态）不发这条：组件要的是「用户新问了什么」。
                        push(TOPIC_PROMPT_UPDATED, Some(sample.phase.clone()));
                    }
                    guard.last.insert(sample.key.clone(), sample.clone());
                }
            }
        }

        // 消失的会话：先产出移除事件，再从读数里删掉。
        let gone: Vec<LightSample> = guard
            .last
            .iter()
            .filter(|(key, _)| !seen.iter().any(|seen_key| seen_key == *key))
            .map(|(_, sample)| sample.clone())
            .collect();
        for sample in gone {
            guard.seq += 1;
            fresh.push(AgentEvent {
                seq: guard.seq,
                ts,
                topic: TOPIC_SESSION_REMOVED.to_string(),
                key: sample.key.clone(),
                provider: sample.provider.clone(),
                session_id: sample.session_id.clone(),
                phase: sample.phase.clone(),
                previous_phase: None,
                endpoint: sample.endpoint.clone(),
                prompt: sample.prompt.clone(),
                started_at_ms: sample.started_at_ms,
                last_seen_at_ms: sample.last_seen_at_ms,
                error_code: sample.error_code.clone(),
                waiting_reason: sample.waiting_reason.clone(),
            });
            guard.last.remove(&sample.key);
        }

        if fresh.is_empty() {
            return 0;
        }
        for ring in guard.rings.values_mut() {
            for event in &fresh {
                ring.push_back(event.clone());
            }
            while ring.len() > EVENT_RING_LINES {
                ring.pop_front();
            }
        }
        fresh.len()
    }

    /// 取某个包 `since` 之后的事件（升序）。没开过环形的包在这里也会被拒（返回 None）。
    pub fn poll(&self, plugin_id: &str, since: u64) -> Option<EventBatch> {
        let guard = lock(&self.0);
        let ring = guard.rings.get(plugin_id)?;
        let events: Vec<AgentEvent> = ring.iter().filter(|event| event.seq > since).cloned().collect();
        Some(EventBatch {
            next_seq: ring.back().map(|event| event.seq).unwrap_or(since),
            ring_size: ring.len(),
            oldest_seq: ring.front().map(|event| event.seq),
            events,
        })
    }
}

/// 起一条巡检线程：按固定间隔问一次「现在所有灯的读数」，把变化投进各包的环形。
///
/// ⚠️ 读数怎么来由调用方给（组合根负责把状态快照换算成 `LightSample`），本模块因此
/// 不认识 agent_status —— 它只管差分与投递。没人订阅时整轮跳过：一秒一趟快照是有成本的，
/// 不该为「一个包都没装」而付。
pub fn spawn_loop<F>(bus: EventState, mut read: F)
where
    F: FnMut() -> (Vec<LightSample>, i64) + Send + 'static,
{
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(paths::EVENT_POLL_INTERVAL_MS));
            if !bus.has_readers() {
                continue;
            }
            let (samples, ts) = read();
            bus.observe(&samples, ts);
        }
    });
}

/// 取一把锁（中毒也照用；`panic = abort` 下不会有残留）。
fn lock<T>(value: &Arc<Mutex<T>>) -> MutexGuard<'_, T> {
    match value.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一盏灯。
    fn sample(key: &str, phase: &str, prompt: Option<&str>) -> LightSample {
        LightSample {
            key: key.to_string(),
            provider: "claude".to_string(),
            session_id: key.to_string(),
            phase: phase.to_string(),
            endpoint: Some("https://api.example.com".to_string()),
            prompt: prompt.map(str::to_string),
            started_at_ms: 1000,
            last_seen_at_ms: 2000,
            error_code: None,
            waiting_reason: None,
        }
    }

    /// 开一个订阅者。
    fn opened(id: &str) -> EventState {
        let state = EventState::new();
        state.attach(id, "floating");
        state
    }

    #[test]
    fn first_round_is_a_baseline_and_makes_no_events() {
        // 首轮就把已经在跑的灯发成事件 = 组件一装就收到一串「它没经历的过去」。
        let state = opened("p");
        assert_eq!(state.observe(&[sample("a", "running", Some("问"))], 10), 0);
        let batch = state.poll("p", 0).expect("开过环形就该有");
        assert!(batch.events.is_empty());
    }

    #[test]
    fn phase_change_produces_one_event_with_the_thick_payload() {
        let state = opened("p");
        state.observe(&[sample("a", "running", Some("第一问"))], 10);
        let changed = LightSample {
            phase: "waiting".to_string(),
            waiting_reason: Some("要审批".to_string()),
            ..sample("a", "waiting", Some("第一问"))
        };
        assert_eq!(state.observe(&[changed], 20), 1);
        let batch = state.poll("p", 0).expect("有环");
        assert_eq!(batch.events.len(), 1);
        let event = &batch.events[0];
        assert_eq!(event.topic, TOPIC_LIGHT_CHANGED);
        assert_eq!(event.phase, "waiting");
        assert_eq!(event.previous_phase.as_deref(), Some("running"));
        // 需求 1 点名的四样：endpoint / 提问 / id / 时间，都必须在同一条里。
        assert_eq!(event.endpoint.as_deref(), Some("https://api.example.com"));
        assert_eq!(event.prompt.as_deref(), Some("第一问"));
        assert_eq!(event.session_id, "a");
        assert_eq!((event.started_at_ms, event.last_seen_at_ms), (1000, 2000));
        assert_eq!(event.waiting_reason.as_deref(), Some("要审批"));
        assert_eq!(event.seq, 1);
        assert_eq!(batch.next_seq, 1);
    }

    #[test]
    fn prompt_change_is_its_own_topic_and_clearing_does_not_emit() {
        let state = opened("p");
        state.observe(&[sample("a", "running", Some("旧问"))], 10);
        assert_eq!(state.observe(&[sample("a", "running", Some("新问"))], 20), 1);
        let batch = state.poll("p", 0).expect("有环");
        assert_eq!(batch.events[0].topic, TOPIC_PROMPT_UPDATED);
        assert_eq!(batch.events[0].prompt.as_deref(), Some("新问"));
        // 提问被清空（离开等待态）不该伪装成「用户又问了一句」。
        assert_eq!(state.observe(&[sample("a", "running", None)], 30), 0);
    }

    #[test]
    fn disappeared_session_becomes_a_removal_event() {
        let state = opened("p");
        state.observe(&[sample("a", "completed", None), sample("b", "running", None)], 10);
        assert_eq!(state.observe(&[sample("b", "running", None)], 20), 1);
        let batch = state.poll("p", 0).expect("有环");
        assert_eq!(batch.events[0].topic, TOPIC_SESSION_REMOVED);
        assert_eq!(batch.events[0].key, "a");
        // 移除事件的 phase 是消失前那个状态，previousPhase 没有意义。
        assert_eq!(batch.events[0].phase, "completed");
        assert_eq!(batch.events[0].previous_phase, None);
        // 彻底消失的读数不能再报一次。
        assert_eq!(state.observe(&[sample("b", "running", None)], 30), 0);
    }

    #[test]
    fn cursor_only_returns_what_a_plugin_has_not_seen() {
        let state = opened("p");
        state.observe(&[sample("a", "running", None)], 10);
        state.observe(&[sample("a", "waiting", None)], 20);
        state.observe(&[sample("a", "error", None)], 30);
        let first = state.poll("p", 0).expect("有环");
        assert_eq!(first.events.len(), 2, "基线那轮不算，两次变化各一条");
        let second = state.poll("p", first.next_seq).expect("有环");
        assert!(second.events.is_empty(), "同一游标不该重复投");
        assert_eq!(second.next_seq, first.next_seq);
    }

    #[test]
    fn ring_is_per_package_and_capped() {
        let state = EventState::new();
        state.attach("slow", "main");
        state.attach("fast", "main");
        // 一轮里同时变两盏灯，制造一批事件。
        state.observe(&[sample("a", "running", None), sample("b", "running", None)], 10);
        for round in 1..=(EVENT_RING_LINES as i64) {
            let phase = if round % 2 == 0 { "waiting" } else { "running" };
            state.observe(&[sample("a", phase, None), sample("b", phase, None)], 20 + round);
        }
        let fast = state.poll("fast", 0).expect("有环");
        assert_eq!(fast.ring_size, EVENT_RING_LINES, "环不许涨过头");
        assert!(fast.oldest_seq.is_some());
        // 慢读者不动游标，也不能把快读者的环挤掉。
        let slow = state.poll("slow", 0).expect("另一包也有自己的环");
        assert_eq!(slow.ring_size, fast.ring_size);
        // 溢出会被明说：oldest 跳过了没取走的旧事件。
        assert!(fast.oldest_seq.unwrap_or(0) > 1, "旧事件应被溢出丢掉：{:?}", fast.oldest_seq);
    }

    #[test]
    fn closing_a_reader_drops_its_ring_and_stops_caring() {
        let state = opened("p");
        state.observe(&[sample("a", "running", None)], 10);
        state.close("p");
        assert!(state.poll("p", 0).is_none(), "关掉的包不该还能取事件");
        assert!(!state.has_readers());
        // 巡检在没人订阅时直接不动：连基线都不建。
        assert_eq!(state.observe(&[sample("a", "waiting", None)], 20), 0);
    }

    #[test]
    fn unknown_package_gets_nothing() {
        let state = opened("p");
        assert!(state.poll("nobody", 0).is_none());
    }
}
