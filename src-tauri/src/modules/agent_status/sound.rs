//! 状态灯的提示音：安装路径下 `data/bgm/` 里**用户可自行替换**的三个 mp3。
//!
//! 为什么放在安装路径而不是前端资源里：开发目录 ≠ 安装目录，打包进前端 bundle 的音频
//! 用户改不了（想换一段音就得重新构建）。因此：
//!
//! - 首次读取时创建 `<安装路径>/data/bgm/`，并把**编进二进制的随包默认音效**补齐进去
//!   —— 装完就有声音，不用手动拷文件；
//! - 用户把同名的 mp3 覆盖进去即可换音（**重启后生效**：文件按需读一次就缓存住了）；
//! - 安装目录只读时（如装在 Program Files）自动回落二进制里的默认音效，功能不受影响，
//!   只是不能替换 —— 提示音不能因为「拷不进去」整个哑掉。
//!
//! ⚠️ 读取只接受**白名单里的三个文件名**，绝不接受任意路径：前端传来的名字先查表，
//! 查不到就直接当作不存在，连路径都不拼（否则就是一条任意文件读取）。

use std::fs;
use std::path::PathBuf;

use tauri::ipc::Response;

/// 音效目录（相对安装路径的子路径）：提示文案里就是这么写的，改这里要同步设置页的说明。
pub const SOUND_DIR_RELATIVE: &str = "data/bgm";

/// 三段音效的**文件名 → 随包默认字节**。
///
/// 文件名与前端 `lib/agent-sound.ts` 的 `LIGHT_SOUND_FILES` 一一对应，改名必须一起改。
/// 默认字节由 `include_bytes!` 编进二进制，源文件在 `src/assets/bgm/`
/// （仓库里必须保留这三个文件，否则**编译**就会失败）。
const SOUND_FILES: [(&str, &[u8]); 3] = [
    (
        "goError.mp3",
        include_bytes!("../../../../src/assets/bgm/goError.mp3"),
    ),
    (
        "goWarn.mp3",
        include_bytes!("../../../../src/assets/bgm/goWarn.mp3"),
    ),
    (
        "goSucc.mp3",
        include_bytes!("../../../../src/assets/bgm/goSucc.mp3"),
    ),
];

/// 安装路径下的音效目录（`<exe 所在目录>/data/bgm`）；取不到 exe 路径时为 None。
///
/// 逐级 `push` 而不是拼字符串：`SOUND_DIR_RELATIVE` 是给人和文档看的写法（正斜杠），
/// 真正落盘要用当前平台的分隔符。
fn sound_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?.to_path_buf();
    for part in SOUND_DIR_RELATIVE.split('/') {
        dir.push(part);
    }
    Some(dir)
}

/// 按文件名取随包默认音效的字节；不在白名单里返回 None。
fn default_bytes(name: &str) -> Option<&'static [u8]> {
    SOUND_FILES
        .iter()
        .find(|(file, _)| *file == name)
        .map(|(_, bytes)| *bytes)
}

/// 创建音效目录，并把**缺失**的文件补成随包默认音效；返回可用的目录。
///
/// 每一环都可能失败（只读安装目录 / 磁盘满 / 无权限），失败一律返回 None —— 调用方据此
/// 回落二进制里的默认音效。⚠️ **已存在的文件绝不被覆盖**：那多半是用户自己换的音。
fn ensure_sound_dir() -> Option<PathBuf> {
    let dir = sound_dir()?;
    if fs::create_dir_all(&dir).is_err() {
        return None;
    }
    for (name, bytes) in SOUND_FILES {
        let path = dir.join(name);
        if !path.is_file() && fs::write(&path, bytes).is_err() {
            return None;
        }
    }
    Some(dir)
}

/// 读一段音效的原始字节：安装目录里的文件优先，读不到回落随包默认。
///
/// 名字不在白名单里直接返回 None（**连路径都不拼**）；盘上的文件存在但读失败 / 是空文件时
/// 同样回落默认值 —— 用户可能拷了一半，那种情况下有声音比没有强。
fn read_sound(name: &str) -> Option<Vec<u8>> {
    if default_bytes(name).is_none() {
        return None;
    }
    if let Some(dir) = ensure_sound_dir() {
        if let Ok(bytes) = fs::read(dir.join(name)) {
            if !bytes.is_empty() {
                return Some(bytes);
            }
        }
    }
    default_bytes(name).map(<[u8]>::to_vec)
}

/// 读一段音效的原始字节，交给前端转成 `blob:` 地址播放。
///
/// 用 `Response` 走**原始字节**而不是 JSON 数组：单段音频几十 KB，编成 `[12,34,...]`
/// 会把 IPC 与解析开销放大好几倍。
///
/// ⚠️ 目录路径不回给前端：提示文案写的是「安装路径下 `data/bgm`」这句固定的话
/// （开发时的绝对路径是 `target/debug/data/bgm`，摆给用户看只会让人困惑）。
#[tauri::command]
pub fn read_light_sound(name: String) -> Result<Response, String> {
    read_sound(&name)
        .map(Response::new)
        .ok_or_else(|| format!("unknown or unreadable light sound: {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 白名单之外的任何名字都取不到字节 —— 这是「不能读任意文件」的那道闸。
    ///
    /// ⚠️ 这条用例刻意不碰磁盘：白名单检查在**拼路径之前**，所以未知名字不会产生任何 I/O。
    #[test]
    fn unknown_names_are_rejected() {
        for name in [
            "",
            "GoError.mp3",
            "goError.mp3.bak",
            "../goError.mp3",
            "..\\..\\secrets.txt",
            "C:\\Windows\\win.ini",
            "bgm/goWarn.mp3",
        ] {
            assert!(default_bytes(name).is_none(), "{name} 不该在表里");
            assert!(read_sound(name).is_none(), "{name} 不该被接受");
        }
    }

    /// 随包默认音效不是占位空文件：三段都得有像样的内容
    /// （否则「装了没声音」要一路查到二进制里才发现是空文件）。
    #[test]
    fn bundled_defaults_are_real_audio_files() {
        for (name, bytes) in SOUND_FILES {
            assert!(bytes.len() > 1024, "{name} 只有 {} 字节，可疑", bytes.len());
            // 三个文件都应是 MP3：以 ID3 标签或帧同步字（0xFF 0xEx）开头。
            let id3 = bytes.starts_with(b"ID3");
            let frame_sync = bytes.len() > 1 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0;
            assert!(id3 || frame_sync, "{name} 不像 MP3");
        }
    }

    /// 文件名与目录契约：与前端 `lib/agent-sound.ts` 的 `LIGHT_SOUND_FILES`、
    /// 设置页说明里那句「安装路径下 data/bgm」必须一致（改名 / 换目录都要一起改）。
    #[test]
    fn file_names_match_the_frontend_contract() {
        let names: Vec<&str> = SOUND_FILES.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, vec!["goError.mp3", "goWarn.mp3", "goSucc.mp3"]);
        // 逐级拼装用的是同一个常量：两段、非空，拼出来就是 `data/bgm`。
        let parts: Vec<&str> = SOUND_DIR_RELATIVE.split('/').collect();
        assert_eq!(parts, vec!["data", "bgm"]);
    }
}
