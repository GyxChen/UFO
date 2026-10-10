//! 插件声明的宿主能力与挂载点：一张表说了算。
//!
//! 这张表是三件事的唯一来源：
//! ① 装前的逐条披露（`preview` / 确认弹窗要把 `disclosure` 原文摊给用户）；
//! ② 管理区的权限徽标（`icon` + `tone` 决定配色档）；
//! ③ 装载时的注入闸门 —— **清单里没写的名字，连全局名都不给**（不是给个会抛的 stub，
//!    那样作者会以为自己有权）。
//!
//! ⚠️ 表里的 `label` / `tone` / `disclosure` 与前端 `src/lib/plugin-capabilities.ts` 逐字对撞
//!    （`plugin-capabilities.spec.ts`）。改文案必须两边一起改：披露文案是给用户看的承诺，
//!    两边漂移就等于界面上一套、弹窗另一套。

/// 一项声明的完整描述。`kind` 区分它是「宿主能力」还是「界面挂载点」。
#[derive(Debug, Clone, Copy)]
pub struct Capability {
    /// 清单里写的名字（`permissions[]` 的元素，或 `surfaces` 的键路径）。
    pub id: &'static str,
    /// 它是「宿主能力」还是「挂载点」。
    pub kind: &'static str,
    /// 徽标上的一行文字。
    pub label: &'static str,
    /// 图标键：真正的 SVG 在前端（`PluginCapabilityIcon.vue`），这里只登记名字。
    pub icon: &'static str,
    /// 配色档：`danger` 红 / `caution` 黄 / `local` 灰。
    pub tone: &'static str,
    /// 装前确认弹窗里的逐条披露原文。
    pub disclosure: &'static str,
}

/// 宿主能力的条数上限校验用：表若与前端对不上，`plugin-capabilities.spec.ts` 会红。
pub const KIND_PERMISSION: &str = "permission";
/// 界面挂载点（不授权任何东西，只声明「我要插在哪」）。
pub const KIND_SURFACE: &str = "surface";

/// 配色档：需要用户点头、且出去就不再受控的能力。
pub const TONE_DANGER: &str = "danger";
/// 配色档：会碰外部世界（网络 / 外设 / 凭据）的能力。
pub const TONE_CAUTION: &str = "caution";
/// 配色档：只影响这个包自己或宿主界面。
pub const TONE_LOCAL: &str = "local";

/// 全部「宿主能力」，顺序 = 徽标与披露列表的渲染顺序（稳定的危险度递减排列）。
pub const PERMISSIONS: &[Capability] = &[
    Capability {
        id: "events:agent",
        kind: KIND_PERMISSION,
        label: "订阅转灯事件",
        icon: "bell",
        tone: TONE_DANGER,
        disclosure: "可读取你发给 AI 的提问原文，其中可能含密钥或私有代码；组件可把它发往外网，发出后不受控。",
    },
    Capability {
        id: "fs",
        kind: KIND_PERMISSION,
        label: "读写文件",
        icon: "folder",
        tone: TONE_DANGER,
        disclosure: "读你亲手在对话框里选中的文件（每次都要重新选），并可在组件私有目录里写任意字节；选中的若是压缩包，能读到里面每一项。",
    },
    Capability {
        id: "http",
        kind: KIND_PERMISSION,
        label: "ufo.http 出网",
        icon: "globe",
        tone: TONE_CAUTION,
        disclosure: "由宿主代发网络请求，只能访问清单 hosts 里列出的域名；请求体与响应内容都在组件手里。",
    },
    Capability {
        id: "secrets",
        kind: KIND_PERMISSION,
        label: "读取密钥",
        icon: "key",
        tone: TONE_CAUTION,
        disclosure: "从系统凭据管理器按名字取值；值不落盘、不进日志、界面上不回显。",
    },
    Capability {
        id: "ble",
        kind: KIND_PERMISSION,
        label: "蓝牙连接与收发",
        icon: "bluetooth",
        tone: TONE_CAUTION,
        disclosure: "扫描并连接蓝牙设备、收发数据；连接会一直保留到你在管理区断开或卸载组件，期间占用无线电与电量。",
    },
    Capability {
        id: "serial",
        kind: KIND_PERMISSION,
        label: "串口读写",
        icon: "plug",
        tone: TONE_CAUTION,
        disclosure: "打开串口并读写；端口被占用期间其它程序拿不到它。",
    },
    Capability {
        id: "frame",
        kind: KIND_PERMISSION,
        label: "内嵌网页",
        icon: "frame",
        tone: TONE_CAUTION,
        disclosure: "在组件界面里嵌入网页（只嵌 https 页面）。嵌入的页面看起来像真站，但可能不是它；多数站点禁止被嵌，显示不出来是正常现象。",
    },
    Capability {
        id: "storage",
        kind: KIND_PERMISSION,
        label: "ufo.storage 按 id 隔离",
        icon: "database",
        tone: TONE_LOCAL,
        disclosure: "在自己的分区里存数据（按包 id 隔离，卸载即删），有容量上限。",
    },
];

/// 全部「界面挂载点」：不是权限，只是「我要插在哪」的声明，一律 local 档。
pub const SURFACES: &[Capability] = &[
    Capability {
        id: "cell",
        kind: KIND_SURFACE,
        label: "悬浮窗格子",
        icon: "grid",
        tone: TONE_LOCAL,
        disclosure: "在悬浮窗里申请格子，与内置组件同级共享同一块窗。",
    },
    Capability {
        id: "tooltip-own",
        kind: KIND_SURFACE,
        label: "独立信息卡（tooltip）",
        icon: "card",
        tone: TONE_LOCAL,
        disclosure: "悬停它自己的格子时弹出这个组件自己的信息卡。",
    },
    Capability {
        id: "tooltip-extension",
        kind: KIND_SURFACE,
        label: "信息卡插入块",
        icon: "insert",
        tone: TONE_LOCAL,
        disclosure: "往别的组件（如状态灯）的信息卡尾部追加一块，最多 3 行。",
    },
    Capability {
        id: "settings",
        kind: KIND_SURFACE,
        label: "主窗设置页",
        icon: "gear",
        tone: TONE_LOCAL,
        disclosure: "在主窗口一级页签里拥有自己的设置页。",
    },
];

/// 按 id 找一条声明（先查能力、再查挂载点）。
#[cfg(test)]
pub fn find(id: &str) -> Option<&'static Capability> {
    PERMISSIONS.iter().chain(SURFACES.iter()).find(|item| item.id == id)
}

/// 该名字是不是一个已登记的宿主能力（清单校验用它：未知名直接拒装）。
pub fn is_known_permission(id: &str) -> bool {
    PERMISSIONS.iter().any(|item| item.id == id)
}

/// 已登记能力的 id 列表（错误文案里要能提示作者可选的名字）。
pub fn permission_ids() -> Vec<&'static str> {
    PERMISSIONS.iter().map(|item| item.id).collect()
}

/// 挂载点的 id 列表。
#[cfg(test)]
pub fn surface_ids() -> Vec<&'static str> {
    SURFACES.iter().map(|item| item.id).collect()
}

/// 把一条声明序列化成前端要的扁平形状（管理区徽标与披露列表共用）。
pub fn to_json(item: &Capability) -> serde_json::Value {
    serde_json::json!({
        "id": item.id,
        "kind": item.kind,
        "label": item.label,
        "icon": item.icon,
        "tone": item.tone,
        "disclosure": item.disclosure,
    })
}

/// 全部声明的 JSON 形式：给 `get_plugin_registry` 命令，前端因此不必再抄一份语义。
///
/// ⚠️ 前端仍然抄了 `label` / `tone` / `disclosure` 的字面量（对撞用），这是刻意的：
/// 手写第二份才能证词「两边真的同源」，只从这份 JSON 反推等于 `x === x`。
pub fn facts_json() -> Vec<serde_json::Value> {
    PERMISSIONS
        .iter()
        .chain(SURFACES.iter())
        .map(to_json)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_across_permissions_and_surfaces() {
        // 撞名的后果是徽标渲染出两条一样的披露，且清单校验会认错。
        let mut seen: Vec<&str> = vec![];
        for item in PERMISSIONS.iter().chain(SURFACES.iter()) {
            assert!(!seen.contains(&item.id), "id「{}」重复登记", item.id);
            seen.push(item.id);
        }
    }

    #[test]
    fn every_entry_carries_a_usable_badge_and_disclosure() {
        for item in PERMISSIONS.iter().chain(SURFACES.iter()) {
            assert!(!item.label.is_empty(), "{} 没有徽标文字", item.id);
            assert!(!item.icon.is_empty(), "{} 没有图标键", item.id);
            assert!(
                matches!(item.tone, TONE_DANGER | TONE_CAUTION | TONE_LOCAL),
                "{} 的 tone 不在三档里",
                item.id
            );
            // 披露是给用户看的承诺，空串等于没披露。
            assert!(item.disclosure.chars().count() > 10, "{} 的披露太短", item.id);
        }
    }

    #[test]
    fn kind_matches_where_the_name_is_declared() {
        assert_eq!(find("http").unwrap().kind, KIND_PERMISSION);
        assert_eq!(find("cell").unwrap().kind, KIND_SURFACE);
        // `ufo.log` 是需求里点名的特例：它有 API 但没有徽标（不授权任何东西）。
        assert!(find("log").is_none());
    }

    #[test]
    fn unknown_names_are_not_known_permissions() {
        assert!(!is_known_permission("fs "));
        assert!(!is_known_permission("cell")); // 挂载点不是能力
        assert!(is_known_permission("events:agent"));
        assert_eq!(permission_ids().len(), PERMISSIONS.len());
        assert_eq!(surface_ids().len(), SURFACES.len());
    }

    #[test]
    fn facts_json_covers_both_tables_in_order() {
        let facts = facts_json();
        assert_eq!(facts.len(), PERMISSIONS.len() + SURFACES.len());
        assert_eq!(facts[0]["id"].as_str(), Some("events:agent"));
        assert_eq!(facts.last().unwrap()["id"].as_str(), Some("settings"));
    }
}
