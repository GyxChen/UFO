//! 追加型 JSONL 的增量行读取。
//!
//! Claude 的 transcript 与 Codex 的 rollout 都是「只追加的 JSONL」，读法完全一样：
//! 按游标只读新增字节、按 `\n` 切行、把写到一半的尾部留到下次。这套逻辑与平台无关，
//! 抽在这里给 token 统计（`tokens.rs`）与 Codex 补漏（`codex_rollout.rs`）共用。

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

/// 单个文件的读取游标。
pub(crate) struct LineCursor {
    /// 已消费到的字节偏移。
    offset: u64,
    /// 上次读取剩下的半行（未以换行结尾），留待下次拼接。
    ///
    /// 按字节保存而非 String：文件正在被追加，末尾可能停在 UTF-8 序列中间，
    /// 提前转成字符串会破坏字符。
    pending: Vec<u8>,
}

impl LineCursor {
    pub(crate) fn new() -> Self {
        Self {
            offset: 0,
            pending: Vec::new(),
        }
    }

    /// 从上次偏移读到底，把每个新增的**完整行**（不含换行符）交给 `on_line`。
    ///
    /// 文件缺失 / 打不开 / 读失败一律静默返回：调用方是常驻 monitor 线程，
    /// 读不到最多是这一轮没数据，绝不能让线程出错。
    ///
    /// 文件被截断或整体替换（长度回退）时从头重读 —— 已由调用方累计的结果保留，
    /// 宁可少算也不要把用户已经看到的数字抹掉。
    pub(crate) fn read_appended(&mut self, path: &Path, mut on_line: impl FnMut(&[u8])) {
        let Ok(metadata) = std::fs::metadata(path) else {
            return;
        };
        let length = metadata.len();

        if length < self.offset {
            self.offset = 0;
            self.pending.clear();
        }
        if length == self.offset {
            return;
        }

        let Ok(mut file) = File::open(path) else {
            return;
        };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut appended = Vec::new();
        if file.read_to_end(&mut appended).is_err() {
            return;
        }
        self.offset += appended.len() as u64;

        // 与上次残留的半行拼接后再切分。
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(&appended);

        let mut start = 0usize;
        for index in 0..data.len() {
            if data[index] == b'\n' {
                on_line(&data[start..index]);
                start = index + 1;
            }
        }
        // 末尾没有换行的部分可能是一行写到一半，留到下次。
        self.pending = data[start..].to_vec();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use crate::modules::agent_status::status::test_support::TempDir;

    /// 收集一次读取得到的完整行（转成字符串便于断言）。
    fn read_lines(cursor: &mut LineCursor, path: &Path) -> Vec<String> {
        let mut lines = Vec::new();
        cursor.read_appended(path, |line| {
            lines.push(String::from_utf8_lossy(line).to_string());
        });
        lines
    }

    /// 建一个已写入内容的文件，返回 (游标, 路径, 临时目录)。
    fn tracked(contents: &str) -> (LineCursor, PathBuf, TempDir) {
        let temp = TempDir::new("lines");
        let path = temp.write("transcript.jsonl", contents);
        (LineCursor::new(), path, temp)
    }

    #[test]
    fn reads_complete_lines_only() {
        let (mut cursor, path, _temp) = tracked("a\nb\nc");
        // 末尾没有换行的 "c" 不算完整行，留到下次。
        assert_eq!(read_lines(&mut cursor, &path), vec!["a", "b"]);
    }

    #[test]
    fn resumes_from_the_previous_offset() {
        let (mut cursor, path, _temp) = tracked("a\n");
        assert_eq!(read_lines(&mut cursor, &path), vec!["a"]);
        // 没有新增内容时不重复读。
        assert!(read_lines(&mut cursor, &path).is_empty());
    }

    #[test]
    fn joins_a_half_written_line_with_the_next_read() {
        let (mut cursor, path, _temp) = tracked("{\"a\":");
        assert!(read_lines(&mut cursor, &path).is_empty());
        // 下半行到了：与上次残留拼接成完整的一行。
        append(&path, "1}\n");
        assert_eq!(read_lines(&mut cursor, &path), vec!["{\"a\":1}"]);
    }

    #[test]
    fn truncation_restarts_from_the_beginning() {
        let (mut cursor, path, _temp) = tracked("first\nsecond\n");
        assert_eq!(read_lines(&mut cursor, &path).len(), 2);
        // 文件被整体替换（长度回退）：从头重读，不丢新内容。
        std::fs::write(&path, "rewritten\n").unwrap();
        assert_eq!(read_lines(&mut cursor, &path), vec!["rewritten"]);
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let temp = TempDir::new("lines-missing");
        let mut cursor = LineCursor::new();
        assert!(read_lines(&mut cursor, &temp.path().join("nope.jsonl")).is_empty());
    }

    #[test]
    fn multibyte_content_survives_a_split() {
        // 半行停在 UTF-8 序列中间时，按字节保存、下次拼接后再解码。
        let (mut cursor, path, _temp) = tracked("中文");
        assert!(read_lines(&mut cursor, &path).is_empty());
        append(&path, "测试\n");
        assert_eq!(read_lines(&mut cursor, &path), vec!["中文测试"]);
    }

    fn append(path: &Path, contents: &str) {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open for append");
        file.write_all(contents.as_bytes()).expect("append");
    }
}
