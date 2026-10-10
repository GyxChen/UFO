//! 外设类能力（蓝牙 / 串口）共用的句柄形状：造一个不可猜的句柄，并按归属取回它。
//!
//! 为什么单独一层：`fs` 的选件句柄、`ble` 的连接句柄、`serial` 的端口句柄说的是同一件事 ——
//! **「这条授权是我发的、只发给这个包、凭这一串字认」**。三处各写一遍的话，
//! 三处的归属判断迟早分叉，而分叉的表现是「A 包能动 B 包的设备」。
//!
//! ⚠️ 协议请求与命令调用都带不上「哪个包在调」的可靠身份（所有组件包与宿主同窗），
//! 所以句柄就是凭证：必须**不可猜**（`<id>-<序号>-<随机段>`，随机段来自每进程随机的
//! `RandomState`），且**只在这次启动里活着**（不写台账、重启即失效）。

use std::collections::BTreeMap;
use std::hash::{BuildHasher, RandomState};
use std::time::SystemTime;

/// 一条句柄的持有者信息（泛型：值是 `Pick`、`Connection` 还是 `Port` 都行）。
#[derive(Debug)]
pub struct Owned<T> {
    /// 发句柄时绑定的包 id；只有它来问才认。
    pub plugin_id: String,
    /// 实际的东西。
    pub value: T,
}

impl<T> Owned<T> {
    /// 包上归属，交给 map 保管。
    pub fn new(plugin_id: &str, value: T) -> Self {
        Self {
            plugin_id: plugin_id.to_string(),
            value,
        }
    }
}

/// 造一个不可猜的句柄。
///
/// ⚠️ 为什么不能只用序号：`<id>-1` 这种连号让别的包猜得出，于是能读到 / 动到
/// 别人刚被用户授权的东西。`RandomState` 每进程带随机种子，句柄因此**进程内唯一、跨进程不可预测** ——
/// 而句柄本来就不需要跨启动活着。
pub fn unguessable(plugin_id: &str, seq: u64, extra: &str) -> String {
    let token = RandomState::new().hash_one((plugin_id, seq, extra, SystemTime::now()));
    format!("{plugin_id}-{seq:x}-{:016x}", token)
}

/// 按句柄取回属于这个包的东西；三种失败分别给三句能看懂的话。
///
/// 「找不到」与「不是你的」必须分开说：前者让作者去重连 / 重选，
/// 后者说明有人在拿别人的句柄试，两者都不该被写成同一句「参数不对」。
pub fn owned<'a, T>(map: &'a BTreeMap<String, Owned<T>>, plugin_id: &str, handle: &str) -> Result<&'a T, String> {
    let found = map
        .get(handle)
        .ok_or_else(|| "这个句柄已经失效：请重新连一次（句柄不跨启动）".to_string())?;
    if found.plugin_id != plugin_id {
        return Err("这个句柄不属于你".to_string());
    }
    Ok(&found.value)
}

/// 这个包握着几个句柄（按包算的帽用它，别用总数）。
pub fn count_of<T>(map: &BTreeMap<String, Owned<T>>, plugin_id: &str) -> usize {
    map.values().filter(|item| item.plugin_id == plugin_id).count()
}

/// 丢掉这个包的全部句柄，返回被丢的那些（调用方决定要不要顺手关连接）。
pub fn take_all<T>(map: &mut BTreeMap<String, Owned<T>>, plugin_id: &str) -> Vec<(String, T)> {
    let stale: Vec<String> = map
        .iter()
        .filter(|(_, item)| item.plugin_id == plugin_id)
        .map(|(handle, _)| handle.clone())
        .collect();
    let mut out = Vec::new();
    for handle in stale {
        if let Some(removed) = map.remove(&handle) {
            out.push((handle, removed.value));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_are_unique_and_end_with_an_unguessable_segment() {
        let first = unguessable("probe", 1, "COM3");
        let second = unguessable("probe", 2, "COM3");
        assert_ne!(first, second);
        // 关键不是「长」，是序号之外还有一段没人猜得到的东西（16 位十六进制）。
        assert!(first.starts_with("probe-1-"), "{first}");
        let last = first.rsplit('-').next().unwrap_or_default();
        assert_eq!(last.len(), 16, "随机段太短就还能猜：{first}");
        assert!(last.bytes().all(|b| b.is_ascii_hexdigit()), "{first}");
        // 包 id 自己带短横线也不能把段数当成判据。
        assert!(unguessable("a-b", 1, "x").starts_with("a-b-1-"));
    }

    #[test]
    fn ownership_errors_are_two_different_sentences() {
        let mut map: BTreeMap<String, Owned<u32>> = BTreeMap::new();
        let handle = unguessable("probe", 1, "x");
        map.insert(handle.clone(), Owned::new("probe", 7u32));
        assert_eq!(owned(&map, "probe", &handle).expect("自己的"), &7u32);
        let foreign = owned(&map, "other", &handle).expect_err("别人的句柄");
        assert!(foreign.contains("不属于你"), "{foreign}");
        let gone = owned(&map, "probe", "probe-1-deadbeefdeadbeef").expect_err("假句柄");
        assert!(gone.contains("重新连"), "{gone}");
    }

    #[test]
    fn counts_and_take_all_are_per_package() {
        let mut map: BTreeMap<String, Owned<u32>> = BTreeMap::new();
        map.insert(unguessable("a", 1, "x"), Owned::new("a", 1u32));
        map.insert(unguessable("a", 2, "x"), Owned::new("a", 2u32));
        map.insert(unguessable("b", 1, "x"), Owned::new("b", 3u32));
        assert_eq!(count_of(&map, "a"), 2);
        assert_eq!(count_of(&map, "b"), 1);
        let taken = take_all(&mut map, "a");
        assert_eq!(taken.len(), 2, "只该拿走 a 的那两条");
        assert_eq!(map.len(), 1);
        assert_eq!(count_of(&map, "b"), 1);
    }
}
