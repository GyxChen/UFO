use std::{fs, path::Path};

/// 从仓库根目录的 `.env` 中读出需要的键，以编译期常量注入二进制。
///
/// `UFO_TAG_RULES_CLAUDE` / `UFO_TAG_RULES_QODER`：提问标签规则表，**一个平台一个键**，
/// 改一家不会波及另一家的行为（语法与缺省见 `status/compat/mod.rs` 的 `build_tag_rules`）。
/// 捕获提问的 hook 是一个独立的短命进程（`ufo --status-hook`），
/// 它的 cwd 是**被监测 agent 的项目目录**，不是 UFO 仓库，运行时按相对路径找不到仓库里的
/// `.env`。注入成常量后 hook 无需任何文件 IO，也满足「hook 必须快进快出」的约束。
/// 修改 `.env` 后需重新构建才会生效。
///
/// `UFO_DEVTOOLS`：同样注入一份，让写在 `.env` 里的窗口 devtools 开关在没有设置进程环境
/// 变量时也能生效。它是兜底值 —— 运行时环境变量优先级更高（见 `src/devtools.rs`）。
fn main() {
    // .env 变化时重跑本脚本，重新注入列表。
    println!("cargo:rerun-if-changed=../.env");

    // 值为空时各适配器会回落到自己那份内置默认，因此这里始终注入、由使用方判空。
    for key in ["UFO_TAG_RULES_CLAUDE", "UFO_TAG_RULES_QODER"] {
        let rules = read_env_value("../.env", key).unwrap_or_default();
        println!("cargo:rustc-env={key}={rules}");
    }

    // 同样始终注入：空值即「全关」，正是 devtools 的默认行为。
    let devtools = read_env_value("../.env", "UFO_DEVTOOLS").unwrap_or_default();
    println!("cargo:rustc-env=UFO_DEVTOOLS={devtools}");

    tauri_build::build()
}

/// 解析 `.env`，取 `key=` 右侧的值（原样保留，含尖括号）。
///
/// 只做最小解析：跳过空行与 `#` 注释，忽略行尾 `\r`（Windows）。找不到或值为空返回 None。
fn read_env_value(path: &str, key: &str) -> Option<String> {
    let raw = fs::read_to_string(Path::new(path)).ok()?;
    let prefix = format!("{key}=");
    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        // 只认 `KEY=value`；`export KEY=value` 之类的写法这里不处理，仅去首尾空白。
        let Some(rest) = line.strip_prefix(&prefix) else {
            continue;
        };
        let value = rest.trim();
        if value.is_empty() {
            return None;
        }
        return Some(value.to_string());
    }
    None
}
