//! 蓝牙能力（`host.ble`）：适配器、连接与通知队列都常驻在 Rust，组件按句柄收发。
//!
//! 为什么状态留在宿主而不是交给组件：一次 BLE 连接是一条活的系统资源（无线电 + 设备句柄），
//! 组件的 UI 会随热重载 / 窗口隐藏而重建，「重建就断连」是不可接受的。所以：
//! 扫描结果、连接、通知环形都在这里，组件走的是一条**按句柄拉取**的命令面
//! （与灯态事件同一套做法：不广播，谁拿着句柄谁取得走）。
//!
//! 四条边界：
//! 1. 句柄就是凭证（见 [`super::handle]`）：不可猜、不跨启动，卸载 / 停用即断连。
//! 2. 一次写不超过 MTU 量级（[`WRITE_MAX_BYTES`]），通知环形固定容量，**溢出丢最旧的并如实说明**。
//! 3. UUID 只认标准 128 位写法（带或不带连字符都收），短UUID（`0x2A37` 那种）不在这一层做翻译 ——
//!    翻译表会随规范版本变，交给作者显式写全更能对上设备的实际 GATT。
//! 4. 适配器是**懒取**的：没蓝牙的机器上不该在建运行态时就失败或卡住。
//!
//! ⚠️ 没有真设备时本模块能验的只有形状与闸（纯函数 + 归属 + 环形溢出）；
//! 实际收发、断连重连只能上机验。`panic = "abort"`：这里不许 unwrap / expect。

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use base64::Engine as _;
use serde::Serialize;

use btleplug::api::{
    AddressType, Central, Characteristic, Manager as ManagerApi, Peripheral as PeripheralApi,
    PeripheralProperties, ScanFilter, Service, ValueNotification, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use uuid::Uuid;

use super::handle::{self, Owned};
use super::paths;

/// 一次扫描的时长夹取（毫秒）。太短扫不到、太长占着无线电。
const SCAN_MIN_MS: u64 = 1_000;
const SCAN_MAX_MS: u64 = 20_000;
const SCAN_DEFAULT_MS: u64 = 6_000;
/// 扫描结果最多留几台。
const DEVICES_MAX: usize = 64;
/// 一个组件同时能连几个外设。
const CONNECTIONS_PER_PACKAGE: usize = 2;
/// 单次连接 / 发现的等待上限（秒）：不让一个设备的挂死拖住整条命令。
const IO_TIMEOUT: Duration = Duration::from_secs(12);
/// 一次写的字节帽（BLE 单包上限按 MTU 走，超过 512 需要作者自己分包）。
const WRITE_MAX_BYTES: usize = 512;
/// 每个外设有几个通知留在环形里（拉得慢会丢最旧的）。
const NOTIFY_RING: usize = 64;

/// 扫描到的一台外设。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BleDevice {
    /// 连接用它；只在这次启动里有效。
    pub handle: String,
    /// MAC / 平台地址（给人看的）。
    pub address: String,
    /// 广播名；没有广播名时回空串（界面自己决定显示什么）。
    pub name: String,
    /// 信号强度；平台没给就回 null。
    pub rssi: Option<i16>,
    /// 广播包里解析出来的字段，一个不丢（见 [`BleAdvertisement`]）。
    pub advert: BleAdvertisement,
}

/// 一次扫描里从广播包解析出的字段。
///
/// 这些值是平台**累积合并**后的最新快照：同一台设备的多个包会合并成一份，不是逐个包给，
/// 所以不同字段可能来自不同时刻的包。宿主不筛选也不截断 —— 组件要认设备种类、解厂商私有负载
/// （iBeacon、米家这类）全靠这里，丢一个字段作者就得重扫或另起一条扫描路径。
///
/// ⚠️ 拿不到的仍是**原样字节**：btleplug 只翻译它认识的 AD 类型，其余段落丢掉，
/// 公开 API 没有任何一处给 `[len][type][data]` 那一串。要真原始包得绕开 btleplug，
/// 自己用 `windows` crate 起 `BluetoothLEAdvertisementWatcher` 逐段读 `DataSections`。
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BleAdvertisement {
    /// 平台给的广播名（Windows 侧来自 WinRT 的 `LocalName()`）。
    pub advertisement_name: Option<String>,
    /// AD 结构里的完整名 / 简短名：与上面是两个来源，可以不一样。
    pub local_name: Option<String>,
    /// GAP appearance（蓝牙规范里给设备分类的 16 位数）。
    pub appearance: Option<u16>,
    /// 广播里的发射功率（dBm）：组件拿它和 rssi 一起折算距离。
    pub tx_power: Option<i16>,
    /// 地址类型：`public` / `random`。随机地址重启就变，别拿它当设备身份。
    pub address_type: Option<String>,
    /// 经典蓝牙的设备类别。只有 BlueZ 会填，Windows 恒为 null。
    pub device_class: Option<u32>,
    /// 广播里声明的服务 UUID（升序、小写、带连字符）。
    pub service_uuids: Vec<String>,
    /// 厂商自定义数据（AD 类型 0xFF），按 company id 升序。
    pub manufacturer_data: Vec<BleManufacturerData>,
    /// 按服务带的数据（AD 类型 0x16 / 0x20 / 0x21），按 UUID 升序。
    pub service_data: Vec<BleServiceData>,
}

/// 一条厂商自定义数据。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BleManufacturerData {
    /// 公司编号（蓝牙联盟分配）。
    pub company_id: u16,
    /// 原始字节（base64，与通知值同一套编码）。
    pub data: String,
}

/// 一条按服务带的数据。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BleServiceData {
    /// 服务 UUID（小写、带连字符的标准写法）。
    pub uuid: String,
    /// 原始字节（base64）。
    pub data: String,
}

impl BleAdvertisement {
    /// 把平台解析好的广播换成本模块的形状。
    ///
    /// 排序是**必须的**不是美观：`manufacturer_data` / `service_data` 是 HashMap、
    /// Windows 侧的 `services` 来自 HashSet，迭代顺序每次都可能不同。不排序的话同一台设备
    /// 两次扫描回来的数组下标会对不上，组件按「第 N 段负载」取值就会时好时坏。
    fn of(properties: &PeripheralProperties) -> Self {
        let mut manufacturer_data: Vec<BleManufacturerData> = properties
            .manufacturer_data
            .iter()
            .map(|row| BleManufacturerData {
                company_id: *row.0,
                data: base64_of(row.1),
            })
            .collect();
        manufacturer_data.sort_by_key(|row| row.company_id);

        let mut service_data: Vec<BleServiceData> = properties
            .service_data
            .iter()
            .map(|row| BleServiceData {
                uuid: row.0.to_string(),
                data: base64_of(row.1),
            })
            .collect();
        service_data.sort_by(|a, b| a.uuid.cmp(&b.uuid));

        let mut service_uuids: Vec<String> =
            properties.services.iter().map(|uuid| uuid.to_string()).collect();
        service_uuids.sort();

        Self {
            advertisement_name: properties.advertisement_name.clone(),
            local_name: properties.local_name.clone(),
            appearance: properties.appearance,
            tx_power: properties.tx_power_level,
            address_type: properties.address_type.map(|kind| match kind {
                AddressType::Public => "public".to_string(),
                AddressType::Random => "random".to_string(),
            }),
            device_class: properties.class,
            service_uuids,
            manufacturer_data,
            service_data,
        }
    }
}

/// 一条通知（组件拉走的就是这个形状）。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BleNotification {
    /// 特征 UUID（小写、带连字符的标准写法）。
    pub uuid: String,
    /// 所属服务 UUID。
    pub service_uuid: String,
    /// 值（base64，与 `host.fs` 的读同一套编码）。
    pub value: String,
    /// 这条之前环形里还压着几条没取走的（溢出时作者能看见自己在丢）。
    pub queued_before: usize,
}

/// GATT 树里的一层（连上之后回给作者，让他挑要订阅哪个特征）。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BleServiceRow {
    /// 服务 UUID。
    pub uuid: String,
    /// 是否主服务。
    pub primary: bool,
    /// 这个服务下的特征（含属性位）。
    pub characteristics: Vec<BleCharacteristicRow>,
}

/// GATT 树里的特征一行。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BleCharacteristicRow {
    /// 特征 UUID。
    pub uuid: String,
    /// 属性位（`read` / `write` / `notify` 等），作者据此决定能不能读写。
    pub properties: Vec<String>,
}

/// 一次连接成功的返回。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BleConnection {
    /// 后续读写 / 订阅 / 取通知都靠它。
    pub handle: String,
    /// 外设地址。
    pub address: String,
    /// 广播名。
    pub name: String,
    /// 协商到的 MTU（平台没给就 null）。
    pub mtu: Option<u16>,
    /// GATT 树。
    pub services: Vec<BleServiceRow>,
}

/// 运行态。
#[derive(Clone, Default)]
pub struct BleState {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug, Default)]
struct Inner {
    /// 扫描句柄 → 外设（每次扫描整体换掉这一包的）。
    found: BTreeMap<String, Owned<Found>>,
    /// 连接句柄 → 活的连接。
    connections: BTreeMap<String, Owned<Connection>>,
    seq: u64,
}

/// 扫到过、还没连的外设。
#[derive(Debug, Clone)]
struct Found {
    peripheral: Peripheral,
    /// 连接成功后要回给界面的广播名（`BleConnection.name`）。
    address: String,
    name: String,
}

/// 一条活的连接：外设在手，通知环形由后台任务喂。
#[derive(Debug)]
struct Connection {
    peripheral: Peripheral,
    address: String,
    ring: Arc<Mutex<VecDeque<BleNotification>>>,
    /// 置位后后台任务收工（断连 / 卸载都走这里）。
    stop: Arc<AtomicBool>,
}

impl BleState {
    /// 建运行态（不碰适配器）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 取本机第一个蓝牙适配器（懒取：没蓝牙的机器不该在建运行态时就炸）。
    async fn central() -> Result<Adapter, String> {
        let manager = Manager::new().await.map_err(|error| format!("蓝牙管理器起不来：{error}"))?;
        let adapters = manager
            .adapters()
            .await
            .map_err(|error| format!("蓝牙适配器列不出：{error}"))?;
        let Some(adapter) = adapters.into_iter().next() else {
            return Err("这台机器上没有可用的蓝牙适配器（或系统里蓝牙被关掉了）".to_string());
        };
        Ok(adapter)
    }

    /// 扫一会儿，返回扫到的外设（并把这一包的扫描句柄整体换成这一批）。
    pub async fn scan(&self, plugin_id: &str, ms: Option<u64>) -> Result<Vec<BleDevice>, String> {
        if !paths::is_valid_plugin_id(plugin_id) {
            return Err(format!("id「{plugin_id}」不合法"));
        }
        let duration = clamp_scan_ms(ms.unwrap_or(SCAN_DEFAULT_MS));
        let central = Self::central().await?;
        central
            .start_scan(ScanFilter::default())
            .await
            .map_err(|error| format!("扫描起不来：{error}（系统蓝牙权限可能被拒）"))?;
        // 扫完一定要停：继续扫会一直占着无线电，也是别人看不到设备的原因。
        tokio::time::sleep(Duration::from_millis(duration)).await;
        // 无论停没停下来都要收这一批结果：停不下来只说明无线电可能被占着，
        // 它不该让已经扫到的设备整批消失（那会让作者以为「扫不到」）。
        let _ = central.stop_scan().await;
        let peripherals = central
            .peripherals()
            .await
            .map_err(|error| format!("扫描结果取不到：{error}"))?;
        let mut rows: Vec<BleDevice> = Vec::new();
        let mut found: BTreeMap<String, Owned<Found>> = BTreeMap::new();
        for peripheral in peripherals.into_iter().take(DEVICES_MAX) {
            let address = peripheral.address().to_string();
            let properties = peripheral.properties().await.unwrap_or(None);
            // 广播里解析出来的字段整份带出：不筛选、不截断（平台没给就整份留默认值）。
            let advert = properties
                .as_ref()
                .map(BleAdvertisement::of)
                .unwrap_or_default();
            // 名字有两种来源：广播里的完整名优先，退化用本地名（两者都可能没有）。
            let name = advert
                .advertisement_name
                .clone()
                .or_else(|| advert.local_name.clone())
                .unwrap_or_default();
            let rssi = properties.as_ref().and_then(|value| value.rssi);
            let issued = {
                let mut guard = lock(&self.inner);
                guard.seq += 1;
                handle::unguessable(plugin_id, guard.seq, &address)
            };
            found.insert(
                issued.clone(),
                Owned::new(
                    plugin_id,
                    Found {
                        peripheral,
                        address: address.clone(),
                        name: name.clone(),
                    },
                ),
            );
            rows.push(BleDevice {
                handle: issued,
                address,
                name,
                rssi,
                advert,
            });
        }
        let mut guard = lock(&self.inner);
        let stale: Vec<String> = guard
            .found
            .iter()
            .filter(|(_, item)| item.plugin_id == plugin_id)
            .map(|(key, _)| key.clone())
            .collect();
        for key in stale {
            guard.found.remove(&key);
        }
        for (key, value) in found {
            guard.found.insert(key, value);
        }
        rows.sort_by(|a, b| b.rssi.unwrap_or(i16::MIN).cmp(&a.rssi.unwrap_or(i16::MIN)));
        Ok(rows)
    }

    /// 连上它并顺手把 GATT 树读回来。
    pub async fn connect(&self, plugin_id: &str, scan_handle: &str) -> Result<BleConnection, String> {
        let found = {
            let guard = lock(&self.inner);
            handle::owned(&guard.found, plugin_id, scan_handle).map(Found::clone)?
        };
        let capacity = {
            let guard = lock(&self.inner);
            handle::count_of(&guard.connections, plugin_id)
        };
        if capacity >= CONNECTIONS_PER_PACKAGE {
            return Err(format!("这个组件已连着 {CONNECTIONS_PER_PACKAGE} 个外设，请先断开不要的"));
        }
        let peripheral = found.peripheral.clone();
        peripheral
            .connect_with_timeout(IO_TIMEOUT)
            .await
            .map_err(|error| format!("连不上「{}」：{error}", found.address))?;
        if let Err(error) = peripheral.discover_services().await {
            // 发现服务失败要顺手断掉：挂着一条连不上也用不了的联系最容易漏。
            let _ = peripheral.disconnect().await;
            return Err(format!("连上了但读不到服务：{error}"));
        }
        let services = gatt_rows(&peripheral.services());
        let mtu = Some(peripheral.mtu());
        let ring: Arc<Mutex<VecDeque<BleNotification>>> = Arc::new(Mutex::new(VecDeque::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let issued = {
            let mut guard = lock(&self.inner);
            guard.seq += 1;
            let key = handle::unguessable(plugin_id, guard.seq, &found.address);
            guard.connections.insert(
                key.clone(),
                Owned::new(
                    plugin_id,
                    Connection {
                        peripheral: peripheral.clone(),
                        address: found.address.clone(),
                        ring: ring.clone(),
                        stop: stop.clone(),
                    },
                ),
            );
            key
        };
        // 通知由后台任务喂进环形：组件走 `notifications()` 按句柄取，不靠广播。
        spawn_notification_drain(peripheral, ring, stop);
        Ok(BleConnection {
            handle: issued,
            address: found.address,
            name: found.name,
            mtu,
            services,
        })
    }

    /// 写一个特征。`with_response` 决定走「要回执」还是「命令」。
    pub async fn write(
        &self,
        plugin_id: &str,
        handle: &str,
        service: &str,
        characteristic: &str,
        bytes: &[u8],
        with_response: bool,
    ) -> Result<usize, String> {
        if bytes.len() > WRITE_MAX_BYTES {
            return Err(format!(
                "一次最多写 {WRITE_MAX_BYTES} 字节（BLE 单包按 MTU 走），这一份有 {} 字节；要发长数据请自己分包",
                bytes.len()
            ));
        }
        let peripheral = self.peripheral_of(plugin_id, handle)?;
        let target = find_characteristic(&peripheral.characteristics(), service, characteristic)?;
        let kind = if with_response { WriteType::WithResponse } else { WriteType::WithoutResponse };
        peripheral
            .write(&target, bytes, kind)
            .await
            .map_err(|error| format!("写不进这个特征：{error}"))?;
        Ok(bytes.len())
    }

    /// 读一个特征。
    pub async fn read(
        &self,
        plugin_id: &str,
        handle: &str,
        service: &str,
        characteristic: &str,
    ) -> Result<Vec<u8>, String> {
        let peripheral = self.peripheral_of(plugin_id, handle)?;
        let target = find_characteristic(&peripheral.characteristics(), service, characteristic)?;
        peripheral
            .read(&target)
            .await
            .map_err(|error| format!("读不到这个特征：{error}"))
    }

    /// 开 / 关某一条通知（关之后组件仍可 `read`）。
    pub async fn subscribe(&self, plugin_id: &str, handle: &str, characteristic: &str) -> Result<(), String> {
        let peripheral = self.peripheral_of(plugin_id, handle)?;
        let target = find_characteristic(&peripheral.characteristics(), "", characteristic)?;
        peripheral
            .subscribe(&target)
            .await
            .map_err(|error| format!("订阅失败：{error}"))
    }

    /// 关一条通知订阅。
    pub async fn unsubscribe(&self, plugin_id: &str, handle: &str, characteristic: &str) -> Result<(), String> {
        let peripheral = self.peripheral_of(plugin_id, handle)?;
        let target = find_characteristic(&peripheral.characteristics(), "", characteristic)?;
        peripheral
            .unsubscribe(&target)
            .await
            .map_err(|error| format!("取消订阅失败：{error}"))
    }

    /// 取走最多 `limit` 条通知（最旧的先给）。
    pub fn notifications(&self, plugin_id: &str, handle: &str, limit: usize) -> Result<Vec<BleNotification>, String> {
        let guard = lock(&self.inner);
        let connection = handle::owned(&guard.connections, plugin_id, handle)?;
        let mut ring = match connection.ring.lock() {
            Ok(ring) => ring,
            Err(poisoned) => poisoned.into_inner(),
        };
        let take = limit.clamp(1, NOTIFY_RING).min(ring.len());
        let mut out: Vec<BleNotification> = Vec::with_capacity(take);
        for _ in 0..take {
            if let Some(row) = ring.pop_front() {
                out.push(row);
            }
        }
        Ok(out)
    }

    /// 这个组件此刻连着哪些外设地址（管理区详情用）。
    pub fn held(&self, plugin_id: &str) -> Vec<String> {
        let guard = lock(&self.inner);
        let mut out: Vec<String> = guard
            .connections
            .values()
            .filter(|item| item.plugin_id == plugin_id)
            .map(|item| item.value.address.clone())
            .collect();
        out.sort();
        out
    }

    /// 摘掉一条连接记录：句柄立刻失效，后台通知任务收工，把外设交给调用方去断开。
    ///
    /// 单独拆成同步函数是**必须的**，不是洁癖：`MutexGuard` 不是 `Send`，
    /// 只要它有可能活过 `disconnect().await`，整条命令的 future 就不再 `Send`，
    /// tauri 注册命令时会直接编译失败（「future cannot be sent between threads safely」）。
    fn take_connection(&self, plugin_id: &str, handle: &str) -> Result<Peripheral, String> {
        let mut guard = lock(&self.inner);
        let found = handle::owned(&guard.connections, plugin_id, handle)?;
        let peripheral = found.peripheral.clone();
        found.stop.store(true, Ordering::Relaxed);
        guard.connections.remove(handle);
        Ok(peripheral)
    }

    /// 断开一条连接（句柄作废）。
    pub async fn disconnect(&self, plugin_id: &str, handle: &str) -> Result<bool, String> {
        // 先摘记录（界面不会看到「还连着但已不可用」），再发断开命令。
        let peripheral = self.take_connection(plugin_id, handle)?;
        peripheral
            .disconnect()
            .await
            .map_err(|error| format!("断开时设备回了错：{error}"))?;
        Ok(true)
    }

    /// 停用 / 卸载 / 重载：这一包的连接全断、扫描句柄全丢。
    ///
    /// 断开的动作交给后台任务发（这里是同步调用点，不能 await）；
    /// 环形与句柄立刻失效，组件再取只会得到「重新连一次」那句。
    pub fn forget(&self, plugin_id: &str) -> usize {
        let mut guard = lock(&self.inner);
        let stale: Vec<String> = guard
            .found
            .iter()
            .filter(|(_, item)| item.plugin_id == plugin_id)
            .map(|(key, _)| key.clone())
            .collect();
        for key in stale {
            guard.found.remove(&key);
        }
        let connections = handle::take_all(&mut guard.connections, plugin_id);
        for (_, connection) in &connections {
            connection.stop.store(true, Ordering::Relaxed);
        }
        let peripherals: Vec<Peripheral> = connections
            .iter()
            .map(|(_, connection)| connection.peripheral.clone())
            .collect();
        drop(guard);
        for peripheral in peripherals {
            tauri::async_runtime::spawn(async move {
                let _ = peripheral.disconnect().await;
            });
        }
        connections.len()
    }

    /// 按句柄拿到活外设（同时校验归属）。
    fn peripheral_of(&self, plugin_id: &str, handle: &str) -> Result<Peripheral, String> {
        let guard = lock(&self.inner);
        Ok(handle::owned(&guard.connections, plugin_id, handle)?.peripheral.clone())
    }
}

/// 把通知流喂进环形：溢出丢最旧的，`stop` 置位或流结束就收工。
fn spawn_notification_drain(
    peripheral: Peripheral,
    ring: Arc<Mutex<VecDeque<BleNotification>>>,
    stop: Arc<AtomicBool>,
) {
    tauri::async_runtime::spawn(async move {
        let Ok(mut stream) = peripheral.notifications().await else {
            return;
        };
        while !stop.load(Ordering::Relaxed) {
            let Some(note) = stream.next().await else {
                break;
            };
            // ⚠️ 顺序要紧：先算好这一行（它要读环长），再锁环去压。
            // 两条写在同一个表达式里会自锁 —— std 的 Mutex 不可重入。
            let row = row_of(&note, &ring);
            push_ring(&mut lock_ring(&ring), row);
        }
    });
}

/// 环形的锁（中毒也继续）。
fn lock_ring(ring: &Arc<Mutex<VecDeque<BleNotification>>>) -> MutexGuard<'_, VecDeque<BleNotification>> {
    match ring.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// 一条通知的形状（`queued_before` 用当前环长，让作者看见自己在丢）。
fn row_of(note: &ValueNotification, ring: &Arc<Mutex<VecDeque<BleNotification>>>) -> BleNotification {
    let queued = ring.lock().map(|guard| guard.len()).unwrap_or(0);
    BleNotification {
        uuid: note.uuid.to_string(),
        service_uuid: note.service_uuid.to_string(),
        value: base64_of(&note.value),
        queued_before: queued,
    }
}

/// 压入一条并守容量（超了先丢最旧的）。
fn push_ring(ring: &mut VecDeque<BleNotification>, row: BleNotification) {
    if ring.len() >= NOTIFY_RING {
        ring.pop_front();
    }
    ring.push_back(row);
}

/// base64（值走 IPC 的形状与 `host.fs` 一致）。
fn base64_of(bytes: &[u8]) -> String {
    base64::prelude::BASE64_STANDARD.encode(bytes)
}

/// 扫描时长夹取。
fn clamp_scan_ms(ms: u64) -> u64 {
    ms.clamp(SCAN_MIN_MS, SCAN_MAX_MS)
}

/// 把 GATT 树压成给作者看的形状（只留 UUID 与属性名，别的一律不带）。
fn gatt_rows(services: &BTreeSet<Service>) -> Vec<BleServiceRow> {
    services
        .iter()
        .map(|service| BleServiceRow {
            uuid: service.uuid.to_string(),
            primary: service.primary,
            characteristics: service
                .characteristics
                .iter()
                .map(|item| BleCharacteristicRow {
                    uuid: item.uuid.to_string(),
                    properties: property_names(item),
                })
                .collect(),
        })
        .collect()
}

/// 属性位翻成名字（作者据此判断能不能读写 / 该不该订阅）。
fn property_names(characteristic: &Characteristic) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (name, flag) in [
        ("read", btleplug::api::CharPropFlags::READ),
        ("write", btleplug::api::CharPropFlags::WRITE),
        ("writeWithoutResponse", btleplug::api::CharPropFlags::WRITE_WITHOUT_RESPONSE),
        ("notify", btleplug::api::CharPropFlags::NOTIFY),
        ("indicate", btleplug::api::CharPropFlags::INDICATE),
        ("broadcast", btleplug::api::CharPropFlags::BROADCAST),
    ] {
        if characteristic.properties.contains(flag) {
            out.push(name.to_string());
        }
    }
    out
}

/// 按 UUID 找特征。
///
/// `service` 给空串时只按特征 UUID 找（订阅 / 取消订阅的接口只给特征 UUID）。
/// 三种失败给三句不同的话：UUID 写法不对 / 这个设备没有这个服务 / 服务里没有这个特征 ——
/// 作者对着设备手册时能立刻知道是哪一层错了。
fn find_characteristic(
    characteristics: &BTreeSet<Characteristic>,
    service: &str,
    characteristic: &str,
) -> Result<Characteristic, String> {
    let wanted = parse_uuid(characteristic).map_err(|reason| format!("特征 UUID 不对：{reason}"))?;
    let wanted_service = if service.trim().is_empty() {
        None
    } else {
        Some(parse_uuid(service).map_err(|reason| format!("服务 UUID 不对：{reason}"))?)
    };
    if let Some(target_service) = wanted_service {
        if !characteristics.iter().any(|item| item.service_uuid == target_service) {
            return Err("这个设备没有这个服务；请先在 connect 返回的 GATT 树里核对 UUID".to_string());
        }
    }
    characteristics
        .iter()
        .find(|item| {
            item.uuid == wanted
                && wanted_service.map_or(true, |target| item.service_uuid == target)
        })
        .cloned()
        .ok_or_else(|| "这个设备没有这个特征；请先在 connect 返回的 GATT 树里核对 UUID".to_string())
}

/// 解析 UUID：标准带连字符写法与 32 位的「simple」写法都收（大小写不敏感）。
fn parse_uuid(text: &str) -> Result<Uuid, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("空".to_string());
    }
    if let Ok(parsed) = Uuid::parse_str(trimmed) {
        return Ok(parsed);
    }
    let plain = trimmed.replace('-', "");
    if plain.len() == 32 {
        let raw = u128::from_str_radix(&plain, 16).map_err(|_| "不是十六进制".to_string())?;
        return Ok(Uuid::from_u128(raw));
    }
    Err(format!("「{trimmed}」不是 128 位 UUID（形如 6e400001-b5a3-f393-e0a9-e50e24dcca9e）"))
}

/// 运行态的锁（中毒也继续用：一个包的 panic 不该让别的外设失联）。
fn lock(inner: &Arc<Mutex<Inner>>) -> MutexGuard<'_, Inner> {
    match inner.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个特征（不需要无线电：`Characteristic` 是普通数据）。
    fn characteristic(service_text: &str, char_text: &str, flags: btleplug::api::CharPropFlags) -> Characteristic {
        Characteristic {
            uuid: parse_uuid(char_text).expect("特征 UUID"),
            service_uuid: parse_uuid(service_text).expect("服务 UUID"),
            properties: flags,
            descriptors: BTreeSet::new(),
        }
    }

    /// 一个标准 UUID 与它的 simple 写法。
    const SERVICE: &str = "6e400001-b5a3-f393-e0a9-e50e24dcca9e";
    const CHAR: &str = "6e400002-b5a3-f393-e0a9-e50e24dcca9e";

    #[test]
    fn uuid_accepts_both_writings_and_refuses_short_forms() {
        let hyphen = parse_uuid(SERVICE).expect("标准写法");
        let simple = parse_uuid("6e400001b5a3f393e0a9e50e24dcca9e").expect("simple 写法");
        assert_eq!(hyphen, simple, "两种写法必须解成同一个");
        // 16 位短 UUID（`0x2A37` / `2a37`）刻意不翻译：翻译表随规范版本变。
        for bad in ["2A37", "0x2a37", "", "   ", "zzzz"] {
            assert!(parse_uuid(bad).is_err(), "{bad} 不该被认成 128 位 UUID");
        }
    }

    #[test]
    fn characteristic_lookup_names_the_layer_that_is_wrong() {
        let mut set: BTreeSet<Characteristic> = BTreeSet::new();
        set.insert(characteristic(
            SERVICE,
            CHAR,
            btleplug::api::CharPropFlags::WRITE | btleplug::api::CharPropFlags::NOTIFY,
        ));
        let found = find_characteristic(&set, SERVICE, CHAR).expect("该找到");
        assert_eq!(found.uuid.to_string(), parse_uuid(CHAR).expect("同一个").to_string());
        // 服务对不上：说的是「没有这个服务」。
        let wrong_service = find_characteristic(
            &set,
            "0000180d-0000-1000-8000-00805f9b34fb",
            CHAR,
        )
        .expect_err("该拒");
        assert!(wrong_service.contains("没有这个服务"), "{wrong_service}");
        // 特征对不上：说的是「没有这个特征」。
        let wrong_char = find_characteristic(&set, SERVICE, "11111111-2222-3333-4444-555555555555")
            .expect_err("该拒");
        assert!(wrong_char.contains("没有这个特征"), "{wrong_char}");
        // UUID 写法不对：三句里最前面那一句。
        let bad = find_characteristic(&set, SERVICE, "2A37").expect_err("该拒");
        assert!(bad.contains("特征 UUID 不对"), "{bad}");
        // 订阅接口只给特征 UUID（service 传空）也要认得出。
        assert!(find_characteristic(&set, "", CHAR).is_ok());
    }

    #[test]
    fn scan_window_is_clamped_not_rejected() {
        assert_eq!(clamp_scan_ms(0), SCAN_MIN_MS);
        assert_eq!(clamp_scan_ms(9_999_999), SCAN_MAX_MS);
        assert_eq!(clamp_scan_ms(SCAN_DEFAULT_MS), SCAN_DEFAULT_MS);
    }

    #[test]
    fn notification_ring_drops_the_oldest_and_keeps_order() {
        let mut ring: VecDeque<BleNotification> = VecDeque::new();
        for index in 0..(NOTIFY_RING + 5) {
            push_ring(&mut ring, BleNotification {
                uuid: CHAR.to_string(),
                service_uuid: SERVICE.to_string(),
                value: index.to_string(),
                queued_before: 0,
            });
        }
        assert_eq!(ring.len(), NOTIFY_RING, "容量必须钉死");
        // 最旧的那五条被丢了：剩下的第一条应是第 5 条。
        assert_eq!(ring.front().map(|row| row.value.as_str()), Some("5"));
        assert_eq!(ring.back().map(|row| row.value.as_str()), Some((NOTIFY_RING + 4).to_string().as_str()));
    }

    #[test]
    fn values_travel_as_base64() {
        assert_eq!(base64_of(&[0u8, 1, 250, 255]), "AAH6/w==");
        assert_eq!(base64_of(&[]), "");
    }

    /// 广播字段一个都不能漏，且顺序必须稳定（HashMap / HashSet 的迭代顺序会变）。
    #[test]
    fn advertisement_carries_every_parsed_field_in_a_stable_order() {
        let mut props = PeripheralProperties::default();
        props.advertisement_name = Some("广播名".to_string());
        props.local_name = Some("本地名".to_string());
        props.appearance = Some(0x0340);
        props.tx_power_level = Some(-40);
        props.address_type = Some(AddressType::Random);
        props.class = Some(0x2a_0104);
        // 故意按 company id 逆序塞，逼排序这一步真的执行。
        props.manufacturer_data.insert(0x004c, vec![0x02, 0x15]);
        props.manufacturer_data.insert(0x0001, vec![0xff]);
        let heart = parse_uuid("0000180d-0000-1000-8000-00805f9b34fb").expect("服务 UUID");
        props.services.push(heart);
        props.services.push(parse_uuid(SERVICE).expect("服务 UUID"));
        props.service_data.insert(heart, vec![1, 2, 3]);
        let adv = BleAdvertisement::of(&props);
        assert_eq!(adv.advertisement_name.as_deref(), Some("广播名"));
        assert_eq!(adv.local_name.as_deref(), Some("本地名"));
        assert_eq!(adv.appearance, Some(0x0340));
        assert_eq!(adv.tx_power, Some(-40));
        assert_eq!(adv.address_type.as_deref(), Some("random"));
        assert_eq!(adv.device_class, Some(0x2a_0104));
        assert_eq!(
            adv.manufacturer_data.iter().map(|row| row.company_id).collect::<Vec<u16>>(),
            vec![0x0001, 0x004c],
            "厂商数据要按 company id 升序，作者按下标取值才不会时好时坏"
        );
        assert_eq!(adv.manufacturer_data[0].data, "/w==", "0x0001 那条的值是 ff");
        assert_eq!(adv.service_uuids.len(), 2);
        assert!(
            adv.service_uuids.windows(2).all(|pair| pair[0] <= pair[1]),
            "服务 UUID 必须升序：{:?}",
            adv.service_uuids
        );
        assert_eq!(adv.service_data.len(), 1);
        assert_eq!(adv.service_data[0].uuid, heart.to_string());
        assert_eq!(adv.service_data[0].data, "AQID");
    }

    /// 平台一份广播都没给时不能报错，也不能编出假值。
    #[test]
    fn advertisement_stays_empty_when_the_platform_has_no_snapshot() {
        let adv = BleAdvertisement::of(&PeripheralProperties::default());
        assert!(adv.advertisement_name.is_none() && adv.local_name.is_none());
        assert!(adv.appearance.is_none() && adv.tx_power.is_none());
        assert!(adv.address_type.is_none() && adv.device_class.is_none());
        assert!(adv.service_uuids.is_empty());
        assert!(adv.manufacturer_data.is_empty() && adv.service_data.is_empty());
    }

    /// 键名是 Rust 与前端唯一的接口，两边没有 `?raw` 对撞，只能在这里钉住。
    #[test]
    fn advertisement_serializes_with_the_names_the_host_interface_reads() {
        let mut props = PeripheralProperties::default();
        props.manufacturer_data.insert(0x004c, vec![2]);
        props.service_data.insert(parse_uuid(SERVICE).expect("服务 UUID"), vec![2]);
        let value = serde_json::to_value(BleDevice {
            handle: "probe-1-abc".to_string(),
            address: "AA:BB".to_string(),
            name: String::new(),
            rssi: None,
            advert: BleAdvertisement::of(&props),
        })
        .expect("可序列化");
        let adv = &value["advert"];
        for key in [
            "advertisementName",
            "localName",
            "appearance",
            "txPower",
            "addressType",
            "deviceClass",
            "serviceUuids",
            "manufacturerData",
            "serviceData",
        ] {
            assert!(adv.get(key).is_some(), "JSON 里没有 {key}：前端读的是这个名");
        }
        assert_eq!(adv["manufacturerData"][0]["companyId"].as_u64(), Some(0x004c));
        assert_eq!(adv["manufacturerData"][0]["data"], "Ag==");
        assert_eq!(adv["serviceData"][0]["uuid"], SERVICE);
    }

    #[test]
    fn gatt_rows_carry_only_uuids_and_property_names() {
        let flags = btleplug::api::CharPropFlags::READ | btleplug::api::CharPropFlags::NOTIFY;
        let mut characteristics: BTreeSet<Characteristic> = BTreeSet::new();
        characteristics.insert(characteristic(SERVICE, CHAR, flags));
        let mut services: BTreeSet<Service> = BTreeSet::new();
        services.insert(Service {
            uuid: parse_uuid(SERVICE).expect("服务 UUID"),
            primary: true,
            characteristics,
        });
        let rows = gatt_rows(&services);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].primary);
        assert_eq!(
            rows[0].characteristics[0].properties,
            vec!["read".to_string(), "notify".to_string()],
            "属性名要按固定顺序给出，界面才稳定"
        );
    }

    #[test]
    fn stale_and_foreign_handles_are_two_different_sentences() {
        let state = BleState::new();
        // 假句柄：先撞上「找不到」那一句。
        let gone = state
            .notifications("probe", "probe-1-deadbeefdeadbeef", 4)
            .expect_err("假句柄");
        assert!(gone.contains("重新连"), "{gone}");
        assert!(state.held("probe").is_empty(), "没有连接时读数要是空");
        assert_eq!(state.forget("probe"), 0, "空状态卸载不该报错");
    }

    #[test]
    fn write_rejects_oversized_payload_before_touching_the_device() {
        // 这条不碰无线电：超大包在查句柄之前就拒（否则「连不上」的错会盖住「该分包」这句）。
        let state = BleState::new();
        let big = vec![0u8; WRITE_MAX_BYTES + 1];
        let err = tauri::async_runtime::block_on(state.write("probe", "probe-1-ffff", SERVICE, CHAR, &big, true))
            .expect_err("超帽");
        assert!(err.contains("一次最多写"), "{err}");
    }
}
