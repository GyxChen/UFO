use std::{fs, path::Path};

/// 从仓库根目录的 `.env` 中读取 `UFO_STRIP_TAGS`，以编译期常量注入二进制。
///
/// 之所以在构建期读取而非运行期：捕获提问的 hook 是一个独立的短命进程
/// （`ufo --status-hook`），它的 cwd 是**被监测 agent 的项目目录**，不是 UFO 仓库，
/// 运行时按相对路径找不到仓库里的 `.env`。注入成常量后 hook 无需任何文件 IO，
/// 也满足「hook 必须快进快出」的约束。修改 `.env` 后需重新构建才会生效。
fn main() {
    // .env 变化时重跑本脚本，重新注入列表。
    println!("cargo:rerun-if-changed=../.env");

    let value = read_strip_tags("../.env").unwrap_or_default();
    // 值为空时 hook.rs 会回落到内置默认列表，因此这里始终注入、由使用方判空。
    println!("cargo:rustc-env=UFO_STRIP_TAGS={value}");

    tauri_build::build()
}

/// 解析 `.env`，取 `UFO_STRIP_TAGS` 的值（`=` 右侧原样保留，含尖括号）。
///
/// 只做最小解析：跳过空行与 `#` 注释，忽略行尾 `\r`（Windows）。找不到返回 None。
fn read_strip_tags(path: &str) -> Option<String> {
    let raw = fs::read_to_string(Path::new(path)).ok()?;
    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        let Some(rest) = line.strip_prefix("UFO_STRIP_TAGS=") else {
            continue;
        };
        // 允许 `export UFO_STRIP_TAGS=...` 之外的写法这里不处理；仅去首尾空白。
        let value = rest.trim();
        if value.is_empty() {
            return None;
        }
        return Some(value.to_string());
    }
    None
}
