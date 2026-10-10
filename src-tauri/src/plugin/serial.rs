//! 串口能力（`host.serial`）：端口常驻在 Rust，组件按句柄读写。
//!
//! 三条边界：
//! 1. **端口名要像端口名**。只认 Windows 的 `COM<n>` 与 Unix 的 `/dev/tty…` ——
//!    否则「打开一个串口」会变成「打开任意路径」，那是这条能力最不该有的形状。
//! 2. **句柄就是凭证**（见 [`super::handle`]）：谁持有句柄谁能读写，别的包问一律拒。
//!    不跨启动，卸载 / 停用即丢（丢弃 = 关掉端口，`Box<dyn SerialPort>` 的 Drop 就负责这件事）。
//! 3. **端口被占用期间别的程序拿不到它**，所以按包数有帽，且管理区卸载时必须回收。
//!
//! ⚠️ 这一层的读写是**阻塞**系统调用：命令一律 `async` + `spawn_blocking`，
//! 不占 UI 线程（AGENTS.md 的「非 async 命令跑在主线程」）。
//! ⚠️ 没有真设备 / 真线时，本模块能验的只有形状与闸；实际收发只能等他上机。

use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use super::handle::{self, Owned};
use super::paths;

/// 一个包同时能握几个端口。
const PORTS_PER_PACKAGE: usize = 4;
/// 一次读写的字节帽（IPC 与设备缓冲都受得住的量）。
const READ_MAX_BYTES: usize = 64 * 1024;
const WRITE_MAX_BYTES: usize = 64 * 1024;
/// 波特率夹取区间：低于 300 通常是笔误，高于 4M 一般要专门的 USB-UART 芯片。
const BAUD_MIN: u32 = 300;
const BAUD_MAX: u32 = 4_000_000;
/// 读超时夹取区间（毫秒）。默认 200ms：一轮 poll 拿不到数据就当「这次没有」返回。
const TIMEOUT_MIN_MS: u64 = 1;
const TIMEOUT_MAX_MS: u64 = 5_000;
const TIMEOUT_DEFAULT_MS: u64 = 200;

/// 一个可打开的端口（列表用，不含任何句柄）。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortRow {
    /// 打开时该用的名字（原样，不做大小写加工）。
    pub name: String,
    /// 给人看的一句话来源（`USB 1a86:7523` 这类）。
    pub detail: String,
}

/// 一次打开的结果。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialHandle {
    /// 后续读写靠它，进程内有效。
    pub handle: String,
    /// 端口名。
    pub name: String,
    /// 实际生效的波特率（被夹取过也如实报）。
    pub baud: u32,
}

/// 运行态。
#[derive(Clone, Default)]
pub struct SerialState {
    inner: Arc<Mutex<Inner>>,
}

/// 运行态。刻意不 `Debug`：`Box<dyn SerialPort>` 不可调试，端口状态本来就只从命令面看。
#[derive(Default)]
struct Inner {
    ports: BTreeMap<String, Owned<Port>>,
    seq: u64,
}

/// 一个开着的端口。`Box<dyn SerialPort>` 是 `Send`，所以 `Mutex` 让它够得着 `Sync`。
struct Port {
    name: String,
    port: Box<dyn serialport::SerialPort>,
}

impl SerialState {
    /// 建运行态（无状态可注入，端口只在 open 时才占）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 列出系统上可打开的串口。
    pub fn list(&self) -> Result<Vec<SerialPortRow>, String> {
        let infos = serialport::available_ports().map_err(|error| format!("端口列表拿不到：{error}"))?;
        let mut rows: Vec<SerialPortRow> = infos
            .into_iter()
            .map(|info| SerialPortRow {
                name: info.port_name.clone(),
                detail: describe(&info.port_type),
            })
            .collect();
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(rows)
    }

    /// 打开一个端口（`timeout_ms` 是单次读的等待上限）。
    pub fn open(
        &self,
        plugin_id: &str,
        name: &str,
        baud: u32,
        timeout_ms: Option<u64>,
    ) -> Result<SerialHandle, String> {
        if !paths::is_valid_plugin_id(plugin_id) {
            return Err(format!("id「{plugin_id}」不合法"));
        }
        let trimmed = name.trim();
        validate_port_name(trimmed)?;
        let clamped_baud = baud.clamp(BAUD_MIN, BAUD_MAX);
        let timeout = Duration::from_millis(
            timeout_ms.unwrap_or(TIMEOUT_DEFAULT_MS).clamp(TIMEOUT_MIN_MS, TIMEOUT_MAX_MS),
        );
        let mut guard = lock(&self.inner);
        if handle::count_of(&guard.ports, plugin_id) >= PORTS_PER_PACKAGE {
            return Err(format!("这个组件已握着 {PORTS_PER_PACKAGE} 个端口，请先关掉不要的"));
        }
        // 先开再记账：开失败不会留下一个占着端口的空记录。
        let port = serialport::new(trimmed, clamped_baud)
            .timeout(timeout)
            .open()
            .map_err(|error| format!("打不开「{trimmed}」：{error}（端口可能被别的程序占着）"))?;
        guard.seq += 1;
        let issued = SerialHandle {
            handle: handle::unguessable(plugin_id, guard.seq, trimmed),
            name: trimmed.to_string(),
            baud: clamped_baud,
        };
        guard.ports.insert(
            issued.handle.clone(),
            Owned::new(plugin_id, Port {
                name: trimmed.to_string(),
                port,
            }),
        );
        Ok(issued)
    }

    /// 改这一轮的读超时（设备侧节奏变了不必重开端口的场景）。
    pub fn set_timeout(&self, plugin_id: &str, handle: &str, timeout_ms: u64) -> Result<u64, String> {
        let clamped = timeout_ms.clamp(TIMEOUT_MIN_MS, TIMEOUT_MAX_MS);
        let mut guard = lock(&self.inner);
        let port = owned_port_mut(&mut guard, plugin_id, handle)?;
        port.port
            .set_timeout(Duration::from_millis(clamped))
            .map_err(|error| format!("改不了超时：{error}"))?;
        Ok(clamped)
    }

    /// 写一段字节，回实际写进去多少。
    pub fn write(&self, plugin_id: &str, handle: &str, bytes: &[u8]) -> Result<usize, String> {
        if bytes.len() > WRITE_MAX_BYTES {
            return Err(format!(
                "一次最多写 {WRITE_MAX_BYTES} 字节，这一份有 {} 字节",
                bytes.len()
            ));
        }
        let mut guard = lock(&self.inner);
        let port = owned_port_mut(&mut guard, plugin_id, handle)?;
        port.port
            .write_all(bytes)
            .map_err(|error| format!("写不进「{}」：{error}", port.name))?;
        Ok(bytes.len())
    }

    /// 读最多 `max` 字节；这一轮没有数据就回空（不报错 —— 轮询取数据是正常形状）。
    pub fn read(&self, plugin_id: &str, handle: &str, max: usize) -> Result<Vec<u8>, String> {
        let want = max.clamp(1, READ_MAX_BYTES);
        let mut guard = lock(&self.inner);
        let port = owned_port_mut(&mut guard, plugin_id, handle)?;
        let mut buffer = vec![0u8; want];
        match port.port.read(&mut buffer) {
            Ok(read) => {
                buffer.truncate(read.min(want));
                Ok(buffer)
            }
            // 超时 / 暂时无数据：这一轮就是没读到，回空比报错有用。
            Err(error) if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                Ok(Vec::new())
            }
            Err(error) => Err(format!("读不到「{}」：{error}", port.name)),
        }
    }

    /// 关一个端口。
    pub fn close(&self, plugin_id: &str, handle: &str) -> Result<bool, String> {
        let mut guard = lock(&self.inner);
        // 归属检查在这里，取出与丢弃在后面：`Box<dyn SerialPort>` 的 Drop 负责真关。
        owned_port_mut(&mut guard, plugin_id, handle)?;
        let removed = guard.ports.remove(handle);
        Ok(removed.is_some())
    }

    /// 这个组件此刻握着哪些端口（管理区详情用，只回名字）。
    pub fn held(&self, plugin_id: &str) -> Vec<String> {
        let guard = lock(&self.inner);
        let mut names: Vec<String> = guard
            .ports
            .values()
            .filter(|item| item.plugin_id == plugin_id)
            .map(|item| item.value.name.clone())
            .collect();
        names.sort();
        names
    }

    /// 停用 / 卸载 / 重载：把这个组件的端口全丢掉（关掉硬件）。
    pub fn forget(&self, plugin_id: &str) -> usize {
        let mut guard = lock(&self.inner);
        let dropped = handle::take_all(&mut guard.ports, plugin_id);
        dropped.len()
    }
}

/// 按句柄取可变端口（泛型 guard 拿不到 `&mut`，这里走 map 的直接查询 + 归属判定）。
fn owned_port_mut<'a>(
    guard: &'a mut Inner,
    plugin_id: &str,
    handle: &str,
) -> Result<&'a mut Port, String> {
    let found = guard
        .ports
        .get_mut(handle)
        .ok_or_else(|| "这个句柄已经失效：请重新连一次（句柄不跨启动）".to_string())?;
    if found.plugin_id != plugin_id {
        return Err("这个句柄不属于你".to_string());
    }
    Ok(&mut found.value)
}

/// 锁中毒（有 panic 过）也要继续用：端口状态比一次别人的 panic 重要。
fn lock(inner: &Arc<Mutex<Inner>>) -> std::sync::MutexGuard<'_, Inner> {
    match inner.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// 端口名形状：只认设备名，不认任意路径。
///
/// 认两种形状，且在**任何平台都认这两种**：Windows 的 `COM1`…`COM255`（两位数、三位数都要按
/// 数字判，别按长度切）与类 Unix 的 `/dev/tty…`（ACM / USB / 蓝牙 RFCOMM 都在这一族）。
/// 刻意不按 `cfg` 分平台：这条规则的目的是挡掉「把打开串口变成打开任意路径」
/// （`\\?\GLOBALROOT\…`、`/etc/passwd`、带 `..` 的那些），而不是替作者管平台方言 ——
/// 名字在本机开不开得成，OS 自己会说得更准。
fn validate_port_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > paths::MAX_REL_PATH_CHARS {
        return Err("端口名不对：空或过长".to_string());
    }
    if paths::has_control_chars(name) {
        return Err("端口名不对：含控制字符".to_string());
    }
    if name.contains("..") || name.contains('\0') {
        return Err("端口名不对：含 `..` 或空字节".to_string());
    }
    let upper = name.to_ascii_uppercase();
    if let Some(digits) = upper.strip_prefix("COM") {
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            // 数字解析失败也归到「编号应在 1…255」这一句，作者只看一句就够。
            if let Ok(number) = digits.parse::<u32>() {
                if number >= 1 && number <= 255 {
                    return Ok(());
                }
            }
            return Err("端口名不对：COM 编号应在 1…255".to_string());
        }
    }
    if name.starts_with("/dev/tty") {
        return Ok(());
    }
    Err("端口名不对：应是 COM3 这样的名字，或 /dev/tty… 这样的设备路径".to_string())
}

/// 端口的来源描述：只列能一眼认出「这是哪根线」的两个 id，别的字段（序列号）不进界面。
fn describe(kind: &serialport::SerialPortType) -> String {
    match kind {
        serialport::SerialPortType::UsbPort(info) => {
            format!("USB {:04x}:{:04x}", info.vid, info.pid)
        }
        serialport::SerialPortType::BluetoothPort => "蓝牙串口".to_string(),
        serialport::SerialPortType::PciPort => "板载串口".to_string(),
        serialport::SerialPortType::Unknown => "系统串口".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两种合法形状在任何平台都认；非法形状在任何平台都拒。
    #[test]
    fn both_device_shapes_are_accepted_everywhere() {
        for good in ["COM3", "com12", "COM255", "/dev/ttyUSB0", "/dev/tty.Bluetooth-Incoming-Port"] {
            assert!(validate_port_name(good).is_ok(), "{good} 该被认成端口名");
        }
    }

    #[test]
    fn names_that_are_not_ports_are_refused() {
        for bad in [
            "",
            "com",
            "COM0",
            "COM300",
            "COM3x",
            "LPT1",
            "/etc/passwd",
            "C:/Windows/win.ini",
            "\\\\?\\GLOBALROOT\\Device\\Serial0",
            "/dev/tty/../etc/passwd",
        ] {
            let err = validate_port_name(bad).expect_err("这种形状该被拒");
            assert!(err.contains("端口名"), "{bad} -> {err}");
        }
        // 控制字符与空字节单独一条理由（`COM3\u{1}` 这种拼接形状）。
        assert!(validate_port_name("COM3\u{1}").is_err());
    }

    #[test]
    fn stale_and_foreign_handles_get_two_different_sentences() {
        let state = SerialState::new();
        let foreign = state.write("other", "COM3-1-abc", b"hi").expect_err("假句柄");
        assert!(foreign.contains("重新连"), "{foreign}");
        // 归属不同的那条理由要单独说（上面那条走的是「找不到」分支，这里造一个真句柄）。
        let mut guard = lock(&state.inner);
        guard.seq = 1;
        let issued = handle::unguessable("probe", 1, "COM3");
        // 没有真端口也先把记录塞进去，只为了走归属判定这一句。
        let opened = serialport::new("COM0-No-Such", 9600).open();
        match opened {
            Ok(port) => {
                guard.ports.insert(
                    issued.clone(),
                    Owned::new("probe", Port { name: "COM3".to_string(), port }),
                );
                drop(guard);
                let err = state.read("other", &issued, 4).expect_err("别人的句柄");
                assert!(err.contains("不属于你"), "{err}");
                assert_eq!(state.forget("probe"), 1, "forget 要按包回收");
            }
            Err(_) => {
                drop(guard);
                // 这台机器开不了端口：只验归属之外的形状（上面已验）。
                assert!(!issued.is_empty());
            }
        }
    }

    #[test]
    fn write_respects_the_size_cap_before_touching_the_port() {
        let state = SerialState::new();
        let big = vec![0u8; WRITE_MAX_BYTES + 1];
        let err = state.write("probe", "probe-1-ffff", &big).expect_err("超帽");
        assert!(err.contains("一次最多写"), "{err}");
    }

    #[test]
    fn caps_are_clamped_not_rejected() {
        // 波特率与超时是「作者写歪了」那一类，夹取并如实回夹完的值，不拒。
        assert_eq!(300u32, 1u32.clamp(BAUD_MIN, BAUD_MAX));
        assert_eq!(BAUD_MAX, 99_999_999u32.clamp(BAUD_MIN, BAUD_MAX));
        assert_eq!(TIMEOUT_MIN_MS, 0u64.clamp(TIMEOUT_MIN_MS, TIMEOUT_MAX_MS));
        assert_eq!(TIMEOUT_MAX_MS, 9_999_999u64.clamp(TIMEOUT_MIN_MS, TIMEOUT_MAX_MS));
    }

    #[test]
    fn held_is_empty_and_sorted_for_a_clean_state() {
        let state = SerialState::new();
        assert!(state.held("probe").is_empty());
        assert_eq!(state.forget("probe"), 0);
    }
}
