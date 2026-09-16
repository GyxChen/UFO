//! QuickJS 沙盒执行器：把用户脚本关进一个有内存/时间上限的运行时里跑，取出返回的字段。
//!
//! ## 隔离与限流三件套
//! 1. **内存帽**：`Runtime::set_memory_limit`。QuickJS 自己的分配器在超限时抛 JS 异常
//!    （不是 Rust panic），脚本表现为 `out of memory`。
//! 2. **时间帽**：`Runtime::set_interrupt_handler`。解释器每执行一段字节码回调一次，回调返回
//!    `true` 即抛一个「不可捕获」异常，兜住 `while(true){}` 这类死循环。截止时刻由 `deadline`
//!    记录，中断触发后 `timed_out` 置位，供上层区分「超时」与「脚本自己报错」。
//!    ⚠️ 中断只在 JS 字节码间触发，宿主同步调用（`http` 阻塞最坏 15s）期间不触发，
//!    因此整体还叠了一层 `budget` 墙钟兜底。
//! 3. **无模块加载器**：刻意不给 `loader`/`dyn-load` feature，脚本 `import`/`require` 都不可用，
//!    因此没有读文件、访问网络的隐式路径 —— 出网只可能经过我们注入的 `http`。
//!
//! ## 为什么用同步宿主 + 轮询 promise 状态，而不是 async 引擎
//! 脚本写在 async 函数里、能 `await`，但宿主 API（`http`/`secrets`/`cache`）全部是**同步阻塞**实现：
//! 它们跑在专用 worker 线程上，阻塞无害。`eval` 返回 async IIFE 的 promise，我们驱动 QuickJS 的
//! job 队列（`execute_pending_job`）把 continuation 跑完，再读 `Promise::state` 取结果。
//! 这样避开了 `futures`/`AsyncRuntime` 那一整套跨线程 spawn 管线，代码路径短得多，也不引入 tokio 依赖。
//!
//! ## panic-free 约定（重要）
//! 见 `api.rs` 顶部说明：release 是 `panic = "abort"`，渲染进程宿主闭包里一次 panic 会带走整个 App。
//! 本文件所有 QuickJS 宿主闭包：
//! - 返回值一律 `rquickjs::Result<Value>`（失败用 `Exception::throw_message` 造 JS 异常再上抛）；
//! - 不 `unwrap` / `expect` / 索引 / slice，读参数用 `args.get::<_, Option<T>>(..)`。

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use rquickjs::{
    context::EvalOptions,
    promise::PromiseState,
    prelude::{Opt, Rest},
    Ctx, Exception, Function, Object, Runtime, String as JsString, Value,
};use serde_json::Value as JsonValue;

use super::api::{
    CacheStore, HttpFetch, HttpRequest, HttpResponse, SecretSource, validate_http_url,
    MAX_HTTP_TIMEOUT_MS, MIN_HTTP_TIMEOUT_MS,
};

/// 沙盒内存上限：拓展字段是轻量取值，2MB 足够，也远小于任何合理机器余量。
const MEMORY_LIMIT_BYTES: usize = 2 * 1024 * 1024;
/// 脚本源码字符上限：防手滑粘进整个仓库；编辑器另有提示。
const MAX_SCRIPT_CHARS: usize = 64 * 1024;
/// 默认 HTTP 超时（脚本没传 timeoutMs 时）。
const DEFAULT_HTTP_TIMEOUT_MS: u64 = 5_000;

/// 一次沙盒运行的成功产物。
#[derive(Debug)]
pub struct RunOutcome {
    /// 脚本 `return` 的字段（已清洗为 `(label, value, color)` 三元组）。
    pub fields: Vec<FlowField>,
    /// `console.log` 缓冲（仅「运行测试」回显，不广播）。
    pub logs: Vec<String>,
    /// 是否因超时被中断（`true` 时 `fields` 不可信）。
    pub timed_out: bool,
    /// 脚本 `return` 的原始 JSON（清洗前），供下次运行作 `ctx.lastResult`。
    pub raw_result: Option<JsonValue>,
}

/// 传给 JS 上下文的会话摘要（`ctx.session`）；快照的只读投影，字段皆 owned 便于拷进调度 job。
///
/// 只给拓展脚本可能用得上的四项，绝不包含 session id / pid / transcript 路径等标识与本地路径，
/// 避免脚本被拿去定位其它会话文件或做跨会话关联。
#[derive(Debug, Clone)]
pub struct SessionContext {
    /// provider 字符串（`claude` / `codex`）。
    pub provider: String,
    /// 会话阶段字符串。
    pub phase: String,
    /// 最近一次提问（可能已被截断）。
    pub prompt: Option<String>,
    /// 累计 token 总量。
    pub tokens_total: u64,
}

/// 沙盒运行环境的宿主依赖：HTTP、密钥、缓存都从这里注入，全部 `Arc` 便于被 `'static` 闭包捕获。
pub struct FlowHost {
    /// HTTP 执行后端。
    pub http: Arc<dyn HttpFetch>,
    /// 密钥来源。
    pub secrets: Arc<dyn SecretSource>,
    /// 进程内 KV 缓存。
    pub cache: Arc<CacheStore>,
    /// 该 endpoint 的凭据 service 名（由 `KeyringSecrets::service_for` 派生）。
    pub secrets_service: String,
    /// endpoint 去重键（`host:port`）。
    pub endpoint_id: String,
    /// endpoint 首次登记的原始地址。
    pub endpoint_url: String,
    /// 当前存活会话摘要；无存活会话时 None（`ctx.session` 为 null）。
    pub session: Option<SessionContext>,
    /// 上次成功的返回值（`ctx.lastResult`），供脚本做增量。
    pub last_result: Option<JsonValue>,
}

/// 用户脚本返回的单个拓展字段（已清洗）。
#[derive(Debug, Clone)]
pub struct FlowField {
    /// 展示标签。
    pub label: String,
    /// 展示值。
    pub value: String,
    /// 可选语义色（`green|yellow|red|muted`），非法值清洗为 None。
    pub color: Option<String>,
}

/// 字段数硬上限：tooltip 固定高度装不下太多行，超出截断并在 UI 提示。
/// ⚠️ 与 `tauri.conf.json` 的 tooltip 高度绑定，改这里要同步看窗口尺寸。
const MAX_FIELDS: usize = 4;
/// 标签字符上限。
const MAX_LABEL_CHARS: usize = 24;
/// 值字符上限。
const MAX_VALUE_CHARS: usize = 120;
/// console 单次运行日志行数上限。
const MAX_LOG_LINES: usize = 50;
/// 单条日志字符上限。
const MAX_LOG_CHARS: usize = 500;

/// 运行一段用户脚本，返回清洗后的字段与日志。
///
/// `script` 是用户源码；`timeout_ms` 单次运行上限。错误统一以 `Err(String)` 返回（人类可读，
/// 含 QuickJS 的 `Error: msg`），调用方据此决定是标 stale 还是清除缓存。
pub fn run_flow(host: FlowHost, script: &str, timeout_ms: u64) -> Result<RunOutcome, String> {
    if script.trim().is_empty() {
        return Ok(RunOutcome {
            fields: Vec::new(),
            logs: Vec::new(),
            timed_out: false,
            raw_result: None,
        });
    }
    if script.chars().count() > MAX_SCRIPT_CHARS {
        return Err(format!("脚本过长（上限 {MAX_SCRIPT_CHARS} 字符）"));
    }
    let timeout_ms = timeout_ms.clamp(MIN_HTTP_TIMEOUT_MS, MAX_HTTP_TIMEOUT_MS);

    let runtime = Runtime::new().map_err(|error| format!("运行时创建失败：{error}"))?;
    runtime.set_memory_limit(MEMORY_LIMIT_BYTES);
    // 栈限制收紧到默认的一半：脚本没有深递归需求，先炸栈比炸进程好。
    runtime.set_max_stack_size(1024 * 1024);

    // 中断触发时刻（相对起始毫秒）与超时标志：interrupt handler 每轮字节码比较一下单调时钟。
    let start_ms = Arc::new(AtomicU64::new(0));
    let timed_out = Arc::new(AtomicBool::new(false));
    {
        let start_ms = Arc::clone(&start_ms);
        let timed_out = Arc::clone(&timed_out);
        // 用 AtomicU64 存「起始 epoch 毫秒」，避免把 `Instant` 塞进 FnMut（跨闭包复制不友好）。
        // handler 在 Context::full 之后随 eval 一起生效；trampoline 由 set_interrupt_handler 注册。
        runtime.set_interrupt_handler(Some(Box::new(move || {
            let base = start_ms.load(Ordering::Relaxed);
            if base == 0 {
                return false;
            }
            if now_millis().saturating_sub(base) >= timeout_ms {
                timed_out.store(true, Ordering::Relaxed);
                return true;
            }
            false
        })));
    }

    // 全量标准内建：拓展脚本要能用 Date/RegExp/JSON/Map/Proxy 这些常见工具。
    // 真正决定「能做什么」的是我们注入了哪些宿主 API，内建本身不出网、不碰文件系统。
    let context = rquickjs::Context::full(&runtime).map_err(|e| format!("上下文创建失败：{e}"))?;

    let logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    // 结果 JSON 与脚本错误信息：drive_script 落在这里，供锁外读取。
    let outcome: Arc<Mutex<Option<Result<JsonValue, String>>>> = Arc::new(Mutex::new(None));

    context.with(|ctx| {
        if let Err(error) = install_globals(&ctx, &host, &logs) {
            store_outcome(&outcome, Err(error));
            return;
        }
        // 记录起始时刻给 interrupt handler（eval 之前，涵盖同步执行的第一段）。
        start_ms.store(now_millis(), Ordering::Relaxed);
        drive_script(&ctx, script, timeout_ms, &outcome);
    });

    let raw_result = match outcome.lock().map(|guard| guard.clone()) {
        Ok(Some(Ok(value))) => value,
        // 脚本抛异常/超时：drive_script 已把人类可读原因写进来，直接上抛。
        Ok(Some(Err(message))) => return Err(message),
        // 没写结果（中断兜底路径）：视为超时错误。
        Ok(None) => return Err("脚本执行超时".to_string()),
        Err(_) => return Err("沙盒状态异常".to_string()),
    };

    let fields = parse_fields(&raw_result);

    let collected_logs = logs
        .lock()
        .map(|logs| logs.clone())
        .unwrap_or_default();

    Ok(RunOutcome {
        fields,
        logs: collected_logs,
        timed_out: timed_out.load(Ordering::Relaxed),
        raw_result: Some(raw_result),
    })
}

/// 解析脚本返回值 JSON 为清洗后的字段列表（独立函数便于单测，无需真跑 QuickJS）。
///
/// 接受三种形态：`{fields:[...]}`、直接一个数组、单个对象 `{label,value}`；其余形态按空处理。
fn parse_fields(result: &JsonValue) -> Vec<FlowField> {
    let array = match result {
        JsonValue::Object(map) => map.get("fields").and_then(|v| v.as_array()),
        JsonValue::Array(array) => Some(array),
        _ => None,
    };

    let candidates: Vec<&JsonValue> = match array {
        Some(items) => items.iter().collect(),
        // 单个 {label,value} 也兼容成一条字段。
        None => {
            if result.get("label").is_some() && result.get("value").is_some() {
                vec![result]
            } else {
                Vec::new()
            }
        }
    };

    candidates
        .into_iter()
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?;
            let value = item.get("value")?;
            // value 允许是数字/布尔，统一 stringify 成文本再截断。
            let value_text = match value {
                JsonValue::String(text) => text.clone(),
                JsonValue::Number(num) => num.to_string(),
                JsonValue::Bool(flag) => flag.to_string(),
                JsonValue::Null => "—".to_string(),
                other => other.to_string(),
            };
            let color = item
                .get("color")
                .and_then(|c| c.as_str())
                .map(|c| c.to_ascii_lowercase())
                .filter(|c| matches!(c.as_str(), "green" | "yellow" | "red" | "muted"));
            Some(FlowField {
                label: truncate_chars(label, MAX_LABEL_CHARS),
                value: truncate_chars(&value_text, MAX_VALUE_CHARS),
                color,
            })
        })
        .take(MAX_FIELDS)
        .collect()
}

/// 安装宿主全局对象到 JS 上下文。所有闭包 panic-free（见文件头约定）。
fn install_globals<'js>(
    ctx: &Ctx<'js>,
    host: &FlowHost,
    logs: &Arc<Mutex<Vec<String>>>,
) -> Result<(), String> {
    let globals = ctx.globals();

    // ---- ctx：只读上下文快照 ----
    let ctx_obj = Object::new(ctx.clone()).map_err(|e| e.to_string())?;
    let endpoint_obj = Object::new(ctx.clone()).map_err(|e| e.to_string())?;
    let _ = endpoint_obj.set("id", host.endpoint_id.as_str());
    let _ = endpoint_obj.set("url", host.endpoint_url.as_str());
    let _ = ctx_obj.set("endpoint", endpoint_obj);
    match &host.session {
        Some(session) => {
            let session_obj = Object::new(ctx.clone()).map_err(|e| e.to_string())?;
            let _ = session_obj.set("provider", session.provider.as_str());
            let _ = session_obj.set("phase", session.phase.as_str());
            match &session.prompt {
                Some(prompt) => {
                    let _ = session_obj.set("prompt", prompt.as_str());
                }
                None => {
                    let _ = session_obj.set("prompt", ());
                }
            }
            let _ = session_obj.set("tokens", session.tokens_total as f64);
            let _ = ctx_obj.set("session", session_obj);
        }
        None => {
            let _ = ctx_obj.set("session", ());
        }
    }
    // lastResult：把 JSON 原样塞成 JS 值（复用已安装的 JSON）。
    if let Some(value) = &host.last_result {
        if let Some(json) = serde_json::to_string(value).ok() {
            if let Ok(parsed) = ctx.json_parse(json) {
                let _ = ctx_obj.set("lastResult", parsed);
            }
        }
    } else {
        let _ = ctx_obj.set("lastResult", ());
    }
    let _ = globals.set("ctx", ctx_obj);

    // ---- console：仅 log/warn/error/info，写进本运行缓冲，不广播 ----
    let console = Object::new(ctx.clone()).map_err(|e| e.to_string())?;
    let logs_for_console = Arc::clone(logs);
    let logger = move |ctx: Ctx<'js>, args: Rest<Value<'js>>| {
        let text = args
            .0
            .iter()
            .map(|value| js_to_log_string(&ctx, value))
            .collect::<Vec<_>>()
            .join(" ");
        push_log(&logs_for_console, &text);
    };
    // Function::new 的闭包返回 ()（IntoJs for () 满足）。
    match Function::new(ctx.clone(), logger) {
        Ok(log_fn) => {
            let _ = console.set("log", log_fn.clone());
            let _ = console.set("info", log_fn.clone());
            let _ = console.set("warn", log_fn.clone());
            let _ = console.set("error", log_fn);
        }
        Err(_) => {}
    }
    let _ = globals.set("console", console);

    // ---- http：get/post/request，走 host.http（Rust 代发）----
    let http = Object::new(ctx.clone()).map_err(|e| e.to_string())?;
    install_http(ctx, &http, Arc::clone(&host.http));
    let _ = globals.set("http", http);

    // ---- secrets：只有 get（无列举、无写入），值不落任何持久层 ----
    let secrets = Object::new(ctx.clone()).map_err(|e| e.to_string())?;
    install_secrets(ctx, &secrets, Arc::clone(&host.secrets), host.secrets_service.clone());
    let _ = globals.set("secrets", secrets);

    // ---- cache：进程内 KV，按 endpoint 分组 ----
    let cache = Object::new(ctx.clone()).map_err(|e| e.to_string())?;
    install_cache(ctx, &cache, Arc::clone(&host.cache), host.endpoint_id.clone());
    let _ = globals.set("cache", cache);

    // ---- format：数字/百分比/字节小工具 ----
    let format = Object::new(ctx.clone()).map_err(|e| e.to_string())?;
    install_format(ctx, &format);
    let _ = globals.set("format", format);

    Ok(())
}

/// 宿主方法 options 参数：`get(url, opts?)` / `post(url, opts?)` / `request(method, url, opts?)`。
///
/// ⚠️ 两点闭包写法约定（都是踩过的坑）：
/// 1. 生命周期必须写成外层具名 `'js`，不能用 `'_` —— `'_` 在闭包参数位会各自推成**互相独立**的
///    匿名生命周期，于是「输入 `Ctx` 里造出的 `Object`/`Value`」与返回类型对不上（E0106 /
///    lifetime may not live long enough）。
/// 2. 可选参数必须是 [`Opt`] 而非 `Option` —— `Option<T>` 只实现了 `FromJs` 没实现 `FromParam`，
///    用它会变成**必填**参数，脚本 `http.get(url)` 少传一个就报「while 2 where expected」。
fn install_http<'js>(ctx: &Ctx<'js>, http: &Object<'js>, backend: Arc<dyn HttpFetch>) {
    let get_backend = Arc::clone(&backend);
    if let Ok(func) = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, url: String, opts: Opt<Object<'js>>| -> rquickjs::Result<Value<'js>> {
            execute_http(&ctx, &get_backend, "GET", &url, opts.0.as_ref())
        },
    ) {
        let _ = http.set("get", func);
    }

    let post_backend = Arc::clone(&backend);
    if let Ok(func) = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, url: String, opts: Opt<Object<'js>>| -> rquickjs::Result<Value<'js>> {
            execute_http(&ctx, &post_backend, "POST", &url, opts.0.as_ref())
        },
    ) {
        let _ = http.set("post", func);
    }

    if let Ok(func) = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>,
              method: String,
              url: String,
              opts: Opt<Object<'js>>|
              -> rquickjs::Result<Value<'js>> {
            execute_http(&ctx, &backend, &method, &url, opts.0.as_ref())
        },
    ) {
        let _ = http.set("request", func);
    }
}

/// 组装并发起一次 HTTP 请求，把响应塞回 JS；URL 非法或后端失败时抛 JS 异常。
fn execute_http<'js>(
    ctx: &Ctx<'js>,
    backend: &Arc<dyn HttpFetch>,
    method: &str,
    url: &str,
    opts: Option<&Object<'js>>,
) -> rquickjs::Result<Value<'js>> {
    if let Err(reason) = validate_http_url(url) {
        return Err(Exception::throw_message(ctx, &reason));
    }
    let timeout_ms = read_opt_u64(ctx, opts, "timeoutMs").unwrap_or(DEFAULT_HTTP_TIMEOUT_MS);
    let headers = read_opt_headers(opts);
    let body = read_opt_body(ctx, opts);
    let request = HttpRequest {
        method: method.to_ascii_uppercase(),
        url: url.to_string(),
        headers,
        body,
        timeout_ms: timeout_ms.clamp(MIN_HTTP_TIMEOUT_MS, MAX_HTTP_TIMEOUT_MS),
    };
    match backend.fetch(&request) {
        Ok(response) => build_response_value(ctx, response),
        Err(reason) => Err(Exception::throw_message(ctx, &reason)),
    }
}

/// `secrets.get(name)`：按名字取值；未配置时抛异常（错误信息不含值）。
///
/// 同 [`install_http`]：闭包参数用外层具名 `'js`，不用 `'_`（否则输入输出生命周期各推各的）。
fn install_secrets<'js>(
    ctx: &Ctx<'js>,
    secrets: &Object<'js>,
    backend: Arc<dyn SecretSource>,
    service: String,
) {
    let getter = move |ctx: Ctx<'js>, name: String| -> rquickjs::Result<Value<'js>> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(Exception::throw_message(&ctx, "密钥名不能为空"));
        }
        match backend.get(&service, trimmed) {
            Some(value) => js_string_value(&ctx, &value),
            None => Err(Exception::throw_message(
                &ctx,
                &format!("未配置密钥：{trimmed}"),
            )),
        }
    };
    if let Ok(func) = Function::new(ctx.clone(), getter) {
        let _ = secrets.set("get", func);
    }
}

/// `cache.get(name)` / `cache.set(name, value, ttlMs?)`。
fn install_cache<'js>(
    ctx: &Ctx<'js>,
    cache: &Object<'js>,
    backend: Arc<CacheStore>,
    scope: String,
) {
    let getter_backend = Arc::clone(&backend);
    let getter_scope = scope.clone();
    let getter = move |ctx: Ctx<'js>, name: String| -> rquickjs::Result<Value<'js>> {
        match getter_backend.get(&getter_scope, &name, now_millis() as i64) {
            Some(raw) => ctx
                .json_parse(raw)
                .map_err(|_| Exception::throw_message(&ctx, "缓存内容非合法 JSON")),
            None => Ok(Value::new_undefined(ctx.clone())),
        }
    };
    let setter_backend = Arc::clone(&backend);
    let setter_scope = scope.clone();
    let setter = move |ctx: Ctx<'js>,
                       name: String,
                       value: Value<'js>,
                       ttl: Opt<f64>|
          -> rquickjs::Result<()> {
        let json_text =
            json_string_from_value(&ctx, &value).unwrap_or_else(|| "null".to_string());
        let ttl_ms = ttl.0.unwrap_or(0.0).max(0.0) as i64;
        setter_backend.set(&setter_scope, &name, json_text, ttl_ms, now_millis() as i64);
        Ok(())
    };

    if let Ok(func) = Function::new(ctx.clone(), getter) {
        let _ = cache.set("get", func);
    }
    if let Ok(func) = Function::new(ctx.clone(), setter) {
        let _ = cache.set("set", func);
    }
}

/// `format.num(n)` 千分位、`format.pct(0.42)`→"42%"、`format.bytes(n)`。
fn install_format<'js>(ctx: &Ctx<'js>, format: &Object<'js>) {
    let num = move |value: f64| -> String { format_thousands(value) };
    let pct = move |value: f64| -> String { format!("{}", (value * 100.0).round()) + "%" };
    let bytes = move |value: f64| -> String { format_bytes(value) };
    if let Ok(func) = Function::new(ctx.clone(), num) {
        let _ = format.set("num", func);
    }
    if let Ok(func) = Function::new(ctx.clone(), pct) {
        let _ = format.set("pct", func);
    }
    if let Ok(func) = Function::new(ctx.clone(), bytes) {
        let _ = format.set("bytes", func);
    }
}

/// 把一次脚本求值驱动到完成（同步宿主 + 轮询 promise 状态），结果写进 `outcome`。
///
/// 正常路径：async IIFE 返回 pending promise → 驱动 job 队列直到 settle → 按 resolved/rejected
/// 落 `Ok(json)` 或 `Err(message)`。超时：interrupt handler 在字节码间抛不可捕获异常，eval 或
/// execute_pending_job 返回 Err → `timed_out` 已置位，落 `Err(超时)`。
fn drive_script(
    ctx: &Ctx<'_>,
    script: &str,
    timeout_ms: u64,
    outcome: &Arc<Mutex<Option<Result<JsonValue, String>>>>,
) {
    // async IIFE：用户脚本体可自然 await；宿主 API 同步，promise 由 job 队列拍平。
    let source = format!("(async function __flowMain__() {{\n{script}\n}})()");
    // EvalOptions 是 #[non_exhaustive]，不能 struct 表达式；default 即 global + strict，符合需求。
    let options = EvalOptions::default();

    let evaluated: Value<'_> = match ctx.eval_with_options(source, options) {
        Ok(value) => value,
        Err(error) => {
            store_outcome(outcome, Err(exception_message(ctx, &error)));
            return;
        }
    };

    // async 函数恒返回 promise；防御性地也支持脚本体被改写返回非 promise 的情况。
    // try_into_promise 在失败时把原值还回来（into_promise 会直接吞掉，非 promise 分支就没了引用）。
    let promise = match evaluated.try_into_promise() {
        Ok(promise) => promise,
        Err(value) => {
            let json = json_string_from_value(ctx, &value).unwrap_or_else(|| "null".to_string());
            store_outcome(
                outcome,
                Ok(serde_json::from_str::<JsonValue>(&json).unwrap_or(JsonValue::Null)),
            );
            return;
        }
    };

    // 驱动 job 队列直到 promise settle；budget 是中断之外的墙钟兜底（宿主阻塞期间中断不触发）。
    let start = Instant::now();
    let budget = Duration::from_millis(timeout_ms + 500);
    while promise.state() == PromiseState::Pending {
        if !ctx.execute_pending_job() {
            // 队列空但 promise 仍 pending：同步宿主模型下不该发生（无外部异步源）。再判一次超时。
            if start.elapsed() > budget {
                store_outcome(outcome, Err("脚本执行超时".to_string()));
                return;
            }
            // 空转保护：避免极端情况下死循环占住 worker 线程。
            std::thread::sleep(Duration::from_millis(1));
            if start.elapsed() > budget {
                store_outcome(outcome, Err("脚本执行超时".to_string()));
                return;
            }
            continue;
        }
        if start.elapsed() > budget {
            break;
        }
    }

    match promise.state() {
        PromiseState::Resolved => {
            match promise.result::<Value>() {
                Some(Ok(value)) => {
                    let json =
                        json_string_from_value(ctx, &value).unwrap_or_else(|| "null".to_string());
                    match serde_json::from_str::<JsonValue>(&json) {
                        Ok(parsed) => store_outcome(outcome, Ok(parsed)),
                        Err(_) => store_outcome(outcome, Ok(JsonValue::Null)),
                    }
                }
                // resolved 但 result 取不到（理论不该发生）：按 null 处理。
                _ => store_outcome(outcome, Ok(JsonValue::Null)),
            }
        }
        PromiseState::Rejected => {
            // result() 会把 rejected 值 JS_Throw 出来，随后 ctx.catch() 能取到它。
            let _ = promise.result::<Value>();
            let thrown = ctx.catch();
            let message = rejected_message(ctx, &thrown);
            store_outcome(outcome, Err(message));
        }
        // 轮询退出但 promise 仍 pending ⇒ 超时。
        PromiseState::Pending => store_outcome(outcome, Err("脚本执行超时".to_string())),
    }
}

/// 从 rejected 的值里提取给用户看的错误文案。
///
/// 不能直接 JSON.stringify：`Error` 的 `message`/`stack` 是**不可枚举**属性，stringify 出来恒为 `{}`。
/// 因此按「字符串 → Error.message → 对象上的 message 字段 → 类型名」逐级降级。
fn rejected_message<'js>(ctx: &Ctx<'js>, thrown: &Value<'js>) -> String {
    if thrown.type_of() == rquickjs::Type::String {
        let text = thrown
            .as_string()
            .and_then(|string| string.to_string().ok())
            .unwrap_or_default();
        if !text.is_empty() {
            return text;
        }
    }
    if let Some(message) = thrown.as_exception().and_then(|e| e.message()) {
        if !message.is_empty() {
            return message;
        }
    }
    // 非 Error 的普通对象：用户自己 throw 一个带 message 的对象。
    if let Some(object) = thrown.as_object() {
        if let Ok(message) = object.get::<_, String>("message") {
            if !message.is_empty() {
                return message;
            }
        }
    }
    let text = js_to_log_string(ctx, thrown);
    if text.is_empty() || text == "{}" {
        format!("脚本抛出了非 Error 值（{}）", thrown.type_name())
    } else {
        text
    }
}

/// 把结果写入共享槽（只在锁成功时写；锁失败说明有 panic，静默丢弃好过 panic）。
fn store_outcome(
    outcome: &Arc<Mutex<Option<Result<JsonValue, String>>>>,
    value: Result<JsonValue, String>,
) {
    if let Ok(mut guard) = outcome.lock() {
        *guard = Some(value);
    }
}

/// 取宿主方法 options 参数里的整数字段（缺失/非法返回 None，由调用方给默认）。
fn read_opt_u64<'js>(_ctx: &Ctx<'js>, opts: Option<&Object<'js>>, key: &str) -> Option<u64> {
    let object = opts?;
    let value: Value<'js> = object.get(key).ok()?;
    coerce_f64(&value).map(|number| number.max(0.0) as u64)
}

/// 取 options.headers 为键值对（只认字符串值，其余跳过）。
fn read_opt_headers<'js>(opts: Option<&Object<'js>>) -> Vec<(String, String)> {
    let mut headers = Vec::new();
    let Some(object) = opts else { return headers };
    let Ok(value) = object.get::<_, Value<'js>>("headers") else {
        return headers;
    };
    let Some(target) = value.as_object() else {
        return headers;
    };
    for key in target.keys::<String>() {
        if let Ok(name) = key {
            if let Ok(val) = target.get::<_, String>(&name) {
                headers.push((name, val));
            }
        }
    }
    headers
}

/// 取 options.body：字符串直接用，对象按 JSON 序列化。
fn read_opt_body<'js>(ctx: &Ctx<'js>, opts: Option<&Object<'js>>) -> Option<String> {
    let object = opts?;
    let value: Value<'js> = object.get("body").ok()?;
    if let Some(text) = value.as_string() {
        return text.to_string().ok();
    }
    json_string_from_value(ctx, &value)
}

/// 把 ureq 响应组装成 JS 对象 `{status, ok, headers, text(), json()}`。
///
/// 同 [`install_http`]：内层 `text()`/`json()` 闭包用外层具名 `'js`，让返回值与被调用的 `Ctx`
/// 同源（写成 `'_` 会让闭包参数的生命周期各推各的，E0106）。
fn build_response_value<'js>(
    ctx: &Ctx<'js>,
    response: HttpResponse,
) -> rquickjs::Result<Value<'js>> {
    let object = Object::new(ctx.clone())?;
    let _ = object.set("status", response.status as i32);
    let _ = object.set("ok", (200..300).contains(&response.status));
    let headers = Object::new(ctx.clone())?;
    for (name, value) in &response.headers {
        let _ = headers.set(name.clone(), value.clone());
    }
    let _ = object.set("headers", headers);
    let body = response.body;

    // text()：闭包捕获 body 克隆，每次调用返回同一份文本。
    let body_for_text = body.clone();
    let text_fn = move |ctx: Ctx<'js>| -> rquickjs::Result<Value<'js>> {
        js_string_value(&ctx, &body_for_text)
    };
    if let Ok(func) = Function::new(ctx.clone(), text_fn) {
        let _ = object.set("text", func);
    }
    // json()：解析失败抛异常（让脚本 try/catch 得到「响应非 JSON」）。
    let body_for_json = body.clone();
    let json_fn = move |ctx: Ctx<'js>| -> rquickjs::Result<Value<'js>> {
        let text = body_for_json.clone();
        ctx.json_parse(text)
            .map_err(|_| Exception::throw_message(&ctx, "响应不是合法 JSON"))
    };
    if let Ok(func) = Function::new(ctx.clone(), json_fn) {
        let _ = object.set("json", func);
    }
    Ok(object.into_value())
}

/// 构造一个 JS 字符串值（`Value::new_string` 在 rquickjs 0.13 不存在，只能走 `String::from_str`）。
fn js_string_value<'js>(ctx: &Ctx<'js>, text: &str) -> rquickjs::Result<Value<'js>> {
    JsString::from_str(ctx.clone(), text).map(|string| string.into_value())
}

/// 把 QuickJS 错误转成给 UI 看的简短消息（尽量带上 `Error: msg` 文本）。
fn exception_message(ctx: &Ctx<'_>, error: &rquickjs::Error) -> String {
    match error {
        rquickjs::Error::Exception => {
            let thrown = ctx.catch();
            let text = js_to_log_string(ctx, &thrown);
            if text.is_empty() {
                "脚本抛出异常".to_string()
            } else {
                text
            }
        }
        rquickjs::Error::WouldBlock => "脚本执行超时（宿主被阻塞）".to_string(),
        other => format!("脚本执行失败：{other}"),
    }
}

/// 把任意 JS 值转成日志用的字符串（对象走 JSON.stringify，异常兜底到 Display）。
fn js_to_log_string<'js>(ctx: &Ctx<'js>, value: &Value<'js>) -> String {
    match value.type_of() {
        rquickjs::Type::Undefined => "undefined".to_string(),
        rquickjs::Type::Null => "null".to_string(),
        rquickjs::Type::String => value
            .as_string()
            .and_then(|string| string.to_string().ok())
            .unwrap_or_default(),
        _ => json_string_from_value(ctx, value).unwrap_or_else(|| String::from("?")),
    }
}

/// JSON.stringify 一个 Value（返回 None 表示不可序列化）。
///
/// `ctx` 与 `value` 必须同一生命周期 `'js`（写成两个 `'_` 会被推成各自独立的匿名生命周期，
/// clone 后进 `json_stringify` 就要求二者互相 outlive）。
fn json_string_from_value<'js>(ctx: &Ctx<'js>, value: &Value<'js>) -> Option<String> {
    let json = ctx.json_stringify(value.clone()).ok()??;
    json.to_string().ok()
}

/// 把 JS 数字/可转数字的值取成 f64。
fn coerce_f64(value: &Value<'_>) -> Option<f64> {
    match value.type_of() {
        rquickjs::Type::Int => value.as_int().map(|number| number as f64),
        rquickjs::Type::Float => value.as_float(),
        rquickjs::Type::Bool => value.as_bool().map(|flag| if flag { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// console 追加一条（截断到上限，超限只保留最新的 MAX_LOG_LINES 条）。
fn push_log(logs: &Arc<Mutex<Vec<String>>>, text: &str) {
    let mut clipped: String = text.chars().take(MAX_LOG_CHARS).collect();
    if text.chars().count() > MAX_LOG_CHARS {
        clipped.push('…');
    }
    if let Ok(mut guard) = logs.lock() {
        guard.push(clipped);
        if guard.len() > MAX_LOG_LINES {
            let overflow = guard.len() - MAX_LOG_LINES;
            guard.drain(0..overflow);
        }
    }
}

/// 按字符（非字节）截断，附省略号。
fn truncate_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(max.saturating_sub(1)).collect();
    clipped.push('…');
    clipped
}

/// 数字千分位（整数值才有分组意义；小数只取整数部分分组）。
fn format_thousands(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let negative = value < 0.0;
    let integer = value.trunc().abs() as i64;
    let digits = integer.to_string();
    let mut grouped = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    if negative {
        format!("-{grouped}")
    } else {
        grouped
    }
}

/// 字节数转人类可读（B/KB/MB/GB，保留两位）。
fn format_bytes(value: f64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    if !value.is_finite() || value < 0.0 {
        return "0 B".to_string();
    }
    let mut number = value;
    let mut unit_index = 0usize;
    while number >= 1024.0 && unit_index < UNITS.len() - 1 {
        number /= 1024.0;
        unit_index += 1;
    }
    if unit_index == 0 {
        format!("{} {}", number as i64, UNITS[0])
    } else {
        format!("{:.2} {}", number, UNITS[unit_index])
    }
}

/// 进程内单调毫秒（用于超时比较；用 wall clock 的 steady 毫秒偏移，避免绝对时间跳变）。
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// 记录被请求过的 URL、从不真正出网的假 HTTP 后端。
    struct RecordingHttp {
        calls: StdMutex<Vec<String>>,
    }
    impl RecordingHttp {
        fn new() -> Self {
            Self {
                calls: StdMutex::new(Vec::new()),
            }
        }
    }
    impl HttpFetch for RecordingHttp {
        fn fetch(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push(request.url.clone());
            }
            Ok(HttpResponse {
                status: 200,
                body: "{\"balance\":42}".to_string(),
                headers: Vec::new(),
            })
        }
    }

    /// 只按名字返回固定值的假密钥后端；`SECRET_` 前缀之外的都算未配置。
    struct FakeSecrets;
    impl SecretSource for FakeSecrets {
        fn get(&self, _service: &str, name: &str) -> Option<String> {
            if name.starts_with("SECRET_") {
                Some("hunter2".to_string())
            } else {
                None
            }
        }
        fn set(&self, _service: &str, _name: &str, _value: &str) -> Result<(), String> {
            Ok(())
        }
        fn delete(&self, _service: &str, _name: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn host_with(http: Arc<dyn HttpFetch>) -> FlowHost {
        FlowHost {
            http,
            secrets: Arc::new(FakeSecrets),
            cache: Arc::new(CacheStore::new()),
            secrets_service: "svc".to_string(),
            endpoint_id: "api.example.com:443".to_string(),
            endpoint_url: "https://api.example.com".to_string(),
            session: Some(SessionContext {
                provider: "claude".to_string(),
                phase: "running".to_string(),
                prompt: Some("你好".to_string()),
                tokens_total: 1234,
            }),
            last_result: None,
        }
    }

    fn run(script: &str, timeout_ms: u64) -> Result<RunOutcome, String> {
        run_flow(host_with(Arc::new(RecordingHttp::new())), script, timeout_ms)
    }

    #[test]
    fn returns_fields_from_array_literal() {
        let outcome = run(
            "return [{label:'余额', value:'¥12.30', color:'green'}]",
            2000,
        )
        .expect("脚本应成功");
        assert_eq!(outcome.fields.len(), 1);
        assert_eq!(outcome.fields[0].label, "余额");
        assert_eq!(outcome.fields[0].color.as_deref(), Some("green"));
    }

    #[test]
    fn awaits_resolved_promise() {
        // 覆盖 PromiseState::Resolved 路径：await 一个已 resolved 的 promise 能取到值。
        let outcome = run(
            "const v = await Promise.resolve(7); return [{label:'n', value:v}]",
            2000,
        )
        .expect("脚本应成功");
        assert_eq!(outcome.fields[0].value, "7");
    }

    #[test]
    fn infinite_loop_times_out_not_hangs() {
        // 中断超时唯一回归：死循环必须在 timeout 内返回 Err，不能永久占住 worker。
        let started = Instant::now();
        let result = run("while (true) {}", 500);
        let elapsed = started.elapsed();
        assert!(result.is_err(), "死循环应被判超时");
        assert!(elapsed < Duration::from_secs(5), "应在预算内返回，实际 {elapsed:?}");
    }

    #[test]
    fn thrown_error_becomes_err_without_panic() {
        let result = run("throw new Error('boom')", 2000);
        let message = result.expect_err("抛异常应返回 Err");
        assert!(message.contains("boom"), "错误信息应含原始文案，实际 {message}");
    }

    #[test]
    fn http_blocks_private_addresses() {
        let http = Arc::new(RecordingHttp::new());
        let calls = Arc::clone(&http);
        let result = run_flow(
            host_with(http),
            "return await http.get('http://127.0.0.1:9/').json()",
            2000,
        );
        assert!(result.is_err(), "本机地址应被拦截");
        // 关键：拦截在校验层，后端根本没被调用（没发出请求）。
        assert!(
            calls.calls.lock().map(|c| c.is_empty()).unwrap_or(false),
            "拦截后不应触达 HTTP 后端"
        );
    }

    #[test]
    fn http_get_parses_json_response() {
        let outcome = run(
            "const res = await http.get('https://api.example.com/v1'); return [{label:'b', value:res.json().balance}]",
            2000,
        )
        .expect("http.get 应可用");
        assert_eq!(outcome.fields[0].value, "42");
    }

    #[test]
    fn http_get_with_and_without_opts() {
        // 覆盖 Opt<Object> 可选参数：带 opts 与不带 opts 都能调（回归 Option 写成必填参数的坑）。
        let with = run(
            "return await http.get('https://api.example.com', {timeoutMs: 3000, headers: {X: '1'}}).json().balance",
            2000,
        );
        assert!(with.is_ok(), "带 opts 调用应成功： {:?}", with.err());
        let without = run(
            "return await http.get('https://api.example.com').json().balance",
            2000,
        );
        assert!(without.is_ok(), "不带 opts 调用应成功： {:?}", without.err());
    }

    #[test]
    fn cache_roundtrip_with_ttl() {
        let outcome = run(
            "cache.set('k', {n: 1}, 5000); const v = cache.get('k'); return [{label:'k', value: v.n}]",
            2000,
        )
        .expect("cache 读写应可用");
        assert_eq!(outcome.fields[0].value, "1");
    }

    #[test]
    fn http_request_method_positional() {
        let outcome = run(
            "const res = await http.request('POST','https://api.example.com', {body: JSON.stringify({a:1})}); return [{label:'ok', value: res.ok}]",
            2000,
        )
        .expect("request(method,url,opts) 应可用");
        assert_eq!(outcome.fields[0].value, "true");
    }

    #[test]
    fn module_loader_unavailable() {
        // 沙盒没有 require / import 口子：feature 不含 loader。
        let outcome = run(
            "return [{label:'hasRequire', value: typeof require}]",
            2000,
        )
        .expect("脚本应成功");
        assert_eq!(outcome.fields[0].value, "undefined");
    }

    #[test]
    fn secret_get_returns_value_missing_throws() {
        let ok = run(
            "return [{label:'k', value: secrets.get('SECRET_A')}]",
            2000,
        )
        .expect("已配置密钥应取到值");
        assert_eq!(ok.fields[0].value, "hunter2");

        let missing = run("secrets.get('NOPE')", 2000);
        let message = missing.expect_err("未配置密钥应抛异常");
        // 错误串只含名字，绝不泄漏任何值。
        assert!(message.contains("NOPE"));
        assert!(!message.contains("hunter2"));
    }

    #[test]
    fn ctx_session_and_endpoint_visible() {
        let outcome = run(
            "return [{label:'p', value: ctx.session.provider}, {label:'e', value: ctx.endpoint.id}]",
            2000,
        )
        .expect("上下文应可见");
        assert_eq!(outcome.fields[0].value, "claude");
        assert_eq!(outcome.fields[1].value, "api.example.com:443");
    }

    #[test]
    fn format_helpers_work() {
        let outcome = run(
            "return [{label:'n', value: format.num(1234567)}, {label:'b', value: format.bytes(2048)}]",
            2000,
        )
        .expect("format 应可用");
        assert_eq!(outcome.fields[0].value, "1,234,567");
        assert_eq!(outcome.fields[1].value, "2.00 KB");
    }

    #[test]
    fn empty_script_returns_no_fields() {
        let outcome = run("   ", 2000).expect("空脚本不算错");
        assert!(outcome.fields.is_empty());
    }

    #[test]
    fn parse_fields_accepts_three_shapes() {
        let array: JsonValue = serde_json::from_str(r#"[{"label":"a","value":1}]"#).unwrap();
        assert_eq!(parse_fields(&array).len(), 1);

        let wrapped: JsonValue =
            serde_json::from_str(r#"{"fields":[{"label":"a","value":"b"}]}"#).unwrap();
        assert_eq!(parse_fields(&wrapped).len(), 1);

        let single: JsonValue = serde_json::from_str(r#"{"label":"a","value":true}"#).unwrap();
        let fields = parse_fields(&single);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].value, "true");
    }

    #[test]
    fn parse_fields_drops_illegal_color_and_caps_count() {
        let items: Vec<String> = (0..10)
            .map(|i| format!(r#"{{"label":"l{i}","value":"v{i}","color":"purple"}}"#))
            .collect();
        let json: JsonValue =
            serde_json::from_str(&format!("[{}]", items.join(","))).unwrap();
        let fields = parse_fields(&json);
        assert_eq!(fields.len(), MAX_FIELDS);
        assert!(fields.iter().all(|f| f.color.is_none()), "非法色值应清洗为 None");
    }

    #[test]
    fn parse_fields_truncates_by_chars() {
        let long = "汉".repeat(MAX_VALUE_CHARS + 20);
        let json: JsonValue = serde_json::from_str(&format!(
            r#"[{{"label":"k","value":"{long}"}}]"#
        ))
        .unwrap();
        let fields = parse_fields(&json);
        // 按字符截断不劈叉：末尾是省略号，且不超过上限。
        assert!(fields[0].value.chars().count() <= MAX_VALUE_CHARS);
        assert!(fields[0].value.chars().last() == Some('…'));
    }
}
